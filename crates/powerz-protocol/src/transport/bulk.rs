//! Vendor bulk transport on `nusb` (the default path).

use std::time::{Duration, Instant};

use nusb::Endpoint;
use nusb::MaybeFuture;
use nusb::transfer::{Buffer, Bulk, In, Out, TransferError};

use super::discovery::{self, DeviceEntry};
use super::select::{EndpointDesc, InterfaceDesc, select_interfaces};
use super::{Error, GuardedTransport, Transport, gone_at_open, map_io_error};
use crate::firmware::flash::{FlashAck, FlashGuard, FlashTransport, Meter, MeterOpener};

/// Bulk max packet size of the meter's endpoints.
pub const MAX_PACKET: usize = 64;
/// Largest reply read as one IN transfer.
pub const MAX_REPLY: usize = 4096;

/// True when an OUT write of `len` bytes must be followed by a zero-length
/// packet: `len` is a non-zero multiple of `max_packet`.
pub fn needs_zlp(len: usize, max_packet: usize) -> bool {
    max_packet != 0 && len != 0 && len.is_multiple_of(max_packet)
}

/// Read and discard stale IN transfers until one times out; returns how many
/// were discarded. Any error other than [`Error::Timeout`] is returned.
pub fn drain_stale<T: Transport + ?Sized>(t: &mut T, timeout: Duration) -> Result<usize, Error> {
    let mut discarded = 0;
    loop {
        match t.read(timeout) {
            Ok(_) => discarded += 1,
            Err(Error::Timeout) => return Ok(discarded),
            Err(e) => return Err(e),
        }
    }
}

/// How long one drain read waits for stale data at open.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(50);

/// Vendor bulk transport over the class-0xFF interface.
#[derive(Debug)]
pub struct BulkTransport {
    out_ep: Endpoint<Bulk, Out>,
    in_ep: Endpoint<Bulk, In>,
    max_packet: usize,
    /// The meter this pipe was opened on.
    device: DeviceEntry,
}

impl BulkTransport {
    /// Open `device`, claim its vendor interface (detaching a kernel driver
    /// first) and drain stale IN data.
    pub fn open(device: &DeviceEntry) -> Result<GuardedTransport<BulkTransport>, Error> {
        Ok(GuardedTransport::new(BulkTransport::open_raw(device)?))
    }

    /// Open `device` like [`open`](Self::open), but behind the strict flash
    /// guard instead of the shared one: the pipe the flash engine's
    /// [`MeterOpener`] hands out.
    ///
    /// This pipe can carry JUMP_DFU, so it needs the user's explicit
    /// [`FlashAck`]. The raw pipe never leaves this module: the flash
    /// engine wraps what an opener hands it in its own [`FlashGuard`], so on
    /// that path this guard is a second, identical line of defence.
    ///
    /// Without the acknowledgement it does not compile:
    ///
    /// ```compile_fail
    /// use powerz_protocol::transport::bulk::BulkTransport;
    /// use powerz_protocol::transport::discovery::DeviceEntry;
    ///
    /// fn try_it(device: &DeviceEntry) {
    ///     let _ = BulkTransport::open_for_flash(device);
    /// }
    /// ```
    ///
    /// With it, it does (not run here: it would open a real meter):
    ///
    /// ```no_run
    /// use powerz_protocol::firmware::flash::FlashAck;
    /// use powerz_protocol::transport::bulk::BulkTransport;
    /// use powerz_protocol::transport::discovery::DeviceEntry;
    ///
    /// fn try_it(device: &DeviceEntry, ack: &FlashAck) {
    ///     let _ = BulkTransport::open_for_flash(device, ack);
    /// }
    /// ```
    pub fn open_for_flash(
        device: &DeviceEntry,
        _ack: &FlashAck,
    ) -> Result<FlashGuard<BulkTransport>, Error> {
        Ok(FlashGuard::new(BulkTransport::open_raw(device)?))
    }

    /// Open `device` with no guard; never leaves this module unwrapped.
    fn open_raw(device: &DeviceEntry) -> Result<BulkTransport, Error> {
        let info = nusb::list_devices()
            .wait()
            .map_err(nusb_error)?
            .find(|d| {
                d.vendor_id() == crate::VID
                    && device.identifies(
                        d.product_id(),
                        d.bus_id(),
                        d.device_address(),
                        d.serial_number(),
                    )
            })
            .ok_or(Error::NotFound)?;
        let dev = info.open().wait().map_err(nusb_error)?;
        let pipes = {
            let config = dev
                .active_configuration()
                .map_err(|e| Error::Io(e.to_string()))?;
            let interfaces: Vec<InterfaceDesc> = config
                .interface_alt_settings()
                .filter(|i| i.alternate_setting() == 0)
                .map(|i| InterfaceDesc {
                    number: i.interface_number(),
                    alt_setting: i.alternate_setting(),
                    class: i.class(),
                    subclass: i.subclass(),
                    protocol: i.protocol(),
                    endpoints: i
                        .endpoints()
                        .map(|e| EndpointDesc {
                            address: e.address(),
                            attributes: e.attributes(),
                            max_packet_size: e.max_packet_size_raw(),
                        })
                        .collect(),
                })
                .collect();
            select_interfaces(&interfaces).bulk.ok_or(Error::NotFound)?
        };
        #[cfg(target_os = "linux")]
        let iface = dev.detach_and_claim_interface(pipes.interface).wait();
        #[cfg(not(target_os = "linux"))]
        let iface = dev.claim_interface(pipes.interface).wait();
        let iface = iface.map_err(nusb_error)?;
        let out_ep = iface
            .endpoint::<Bulk, Out>(pipes.out_ep)
            .map_err(nusb_error)?;
        let in_ep = iface
            .endpoint::<Bulk, In>(pipes.in_ep)
            .map_err(nusb_error)?;
        let max_packet = out_ep.max_packet_size();
        let mut t = BulkTransport {
            out_ep,
            in_ep,
            max_packet,
            device: device.clone(),
        };
        drain_stale(&mut t, DRAIN_TIMEOUT)?;
        Ok(t)
    }

    /// The meter this pipe was opened on is still listed on USB.
    fn listed(&self) -> Option<bool> {
        let listed = discovery::list_devices().ok()?;
        Some(discovery::still_listed(&self.device, &listed))
    }

    /// The transport error for a transfer on this pipe that failed after
    /// waiting `wait`.
    fn failed(&self, e: TransferError, wait: Duration) -> Error {
        let start = Instant::now();
        let listed = within_budget(|| start.elapsed(), || self.listed());
        settle_failed(e, wait, listed, || std::thread::sleep(GONE_PAUSE))
    }

    fn write(&mut self, data: &[u8], timeout: Duration) -> Result<(), Error> {
        let c = self
            .out_ep
            .transfer_blocking(Buffer::from(data.to_vec()), timeout);
        c.status.map_err(|e| self.failed(e, timeout))?;
        if needs_zlp(data.len(), self.max_packet) {
            let c = self.out_ep.transfer_blocking(Buffer::new(0), timeout);
            c.status.map_err(|e| self.failed(e, timeout))?;
        }
        Ok(())
    }
}

impl Transport for BulkTransport {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        self.write(request, timeout)?;
        self.read(timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        let c = self
            .in_ep
            .transfer_blocking(Buffer::new(MAX_REPLY), timeout);
        c.status.map_err(|e| self.failed(e, timeout))?;
        let mut data = c.buffer.into_vec();
        data.truncate(c.actual_len);
        Ok(data)
    }

    /// Refused before sending: the KM003C ignores the settings write over
    /// bulk (2026-10-29) \[device\]; it goes over the CDC port.
    fn transact_settings(&mut self, _request: &[u8], _timeout: Duration) -> Result<Vec<u8>, Error> {
        Err(Error::SettingsNeedCdc)
    }

    /// Clear the halt on both bulk endpoints (a standard USB control request,
    /// not a meter command), then drain stale IN data.
    fn recover(&mut self) -> Result<(), Error> {
        self.out_ep.clear_halt().wait().map_err(nusb_error)?;
        self.in_ep.clear_halt().wait().map_err(nusb_error)?;
        drain_stale(self, DRAIN_TIMEOUT)?;
        Ok(())
    }
}

/// Raw writes for the flash path only: [`BulkTransport`] leaves this module
/// wrapped in the shared guard (which is not a [`FlashTransport`]) or in the
/// strict [`FlashGuard`].
impl FlashTransport for BulkTransport {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), Error> {
        self.write(data, timeout)
    }
}

/// The flash engine's [`MeterOpener`] for a real meter: each open lists the
/// devices again (the meter re-enumerates between modes), picks the meter
/// with [`discovery::follow_flashed_meter`] (by its serial, latched from the
/// first meter opened when none was chosen) and opens its bulk pipe behind
/// the strict flash guard. An ambiguous choice is an [`Error::Io`] naming the
/// candidates.
#[derive(Debug)]
pub struct BulkFlashOpener {
    serial: Option<String>,
    ack: FlashAck,
}

impl BulkFlashOpener {
    /// An opener for the meter with `serial` (else the only POWER-Z
    /// attached at the first open, followed by its serial afterwards);
    /// `ack` is the acknowledgement the opened pipes need.
    pub fn new(serial: Option<String>, ack: FlashAck) -> BulkFlashOpener {
        BulkFlashOpener { serial, ack }
    }
}

impl MeterOpener for BulkFlashOpener {
    type Pipe = FlashGuard<BulkTransport>;

    fn open(&mut self) -> Result<Option<Meter<Self::Pipe>>, Error> {
        let devices = discovery::list_devices()?;
        let Some(device) = discovery::follow_flashed_meter(&devices, &mut self.serial)
            .map_err(|e| Error::Io(e.to_string()))?
        else {
            return Ok(None);
        };
        Ok(Some(Meter {
            mode: device.mode(),
            model: device.model,
            pipe: BulkTransport::open_for_flash(device, &self.ack)?,
        }))
    }
}

fn nusb_error(e: nusb::Error) -> Error {
    let os = std::env::consts::OS;
    // The `io::Error` nusb converts to does not carry its OS code.
    let code = e.os_error().and_then(|code| i32::try_from(code).ok());
    if code.is_some_and(|code| gone_at_open(code, os)) {
        return Error::Disconnected;
    }
    map_io_error(&std::io::Error::from(e), os)
}

/// How often, and how far apart, a failed transfer looks for the meter on USB.
const GONE_LOOKS: usize = 5;
const GONE_PAUSE: Duration = Duration::from_millis(50);
/// Shortest wait whose cancelled transfer looks for the meter on USB. The
/// flash erase path expects its 10 ms and 20 ms reads to time out, and a look
/// there would change its wire timing.
pub(crate) const GONE_CHECK_MIN_WAIT: Duration = Duration::from_millis(100);

/// Most time the looks may take in all, so a bus that lists slowly still
/// settles an unplug inside the 1 s of AC-AXHK2J.3.
pub(crate) const GONE_BUDGET: Duration = Duration::from_millis(800);

/// `listed`, until `elapsed` passes [`GONE_BUDGET`]: after that it answers
/// `None`, which proves nothing and ends the looks.
fn within_budget(
    elapsed: impl Fn() -> Duration,
    mut listed: impl FnMut() -> Option<bool>,
) -> impl FnMut() -> Option<bool> {
    move || {
        if elapsed() < GONE_BUDGET {
            listed()
        } else {
            None
        }
    }
}

/// The final error for a transfer that failed after waiting `wait`: a
/// cancelled transfer that waited less than [`GONE_CHECK_MIN_WAIT`] is a plain
/// [`Error::Timeout`], everything else goes through [`settle_gone`].
fn settle_failed(
    e: TransferError,
    wait: Duration,
    listed: impl FnMut() -> Option<bool>,
    pause: impl FnMut(),
) -> Error {
    match transfer_error(e) {
        Failure::Cancelled if wait < GONE_CHECK_MIN_WAIT => Error::Timeout,
        failure => settle_gone(failure, listed, pause),
    }
}

/// [`Error::Disconnected`] when an [`Error::Io`] or a cancelled transfer came
/// with the meter leaving USB: macOS can end a transfer on a pulled cable with
/// a status nusb does not map to its disconnect error, and the listing can lag
/// that transfer. `listed` says whether the meter is listed (`None` when USB
/// cannot be listed, which proves nothing). A cancelled transfer of a meter
/// that did not leave is an [`Error::Timeout`].
fn settle_gone(
    failure: impl Into<Failure>,
    mut listed: impl FnMut() -> Option<bool>,
    mut pause: impl FnMut(),
) -> Error {
    let unsettled = match failure.into() {
        Failure::Cancelled => Error::Timeout,
        Failure::Other(err @ Error::Io(_)) => err,
        Failure::Other(other) => return other,
    };
    for look in 1..=GONE_LOOKS {
        match listed() {
            Some(false) => return Error::Disconnected,
            None => return unsettled,
            Some(true) if look < GONE_LOOKS => pause(),
            Some(true) => {}
        }
    }
    unsettled
}

/// A failed transfer before [`settle_gone`] has looked for the meter. Cancelled
/// stays in here: callers only ever see what it settles into.
enum Failure {
    /// The transfer ended cancelled: a timeout, unless the meter left USB.
    Cancelled,
    /// Any other failure.
    Other(Error),
}

impl From<Error> for Failure {
    fn from(err: Error) -> Failure {
        Failure::Other(err)
    }
}

fn transfer_error(e: TransferError) -> Failure {
    match e {
        TransferError::Cancelled => Failure::Cancelled,
        TransferError::Disconnected => Error::Disconnected.into(),
        other => Error::Io(other.to_string()).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::{MockTransport, Step};

    fn pause() -> impl FnMut() {
        || {}
    }

    #[test]
    fn looks_stop_when_the_budget_is_spent() {
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut looks = 0;
        let listed = within_budget(
            || elapsed.get(),
            || {
                looks += 1;
                elapsed.set(elapsed.get() + GONE_BUDGET / 2 + Duration::from_millis(1));
                Some(true)
            },
        );
        let got = settle_gone(Failure::Cancelled, listed, pause());
        assert_eq!(got, Error::Timeout);
        assert_eq!(looks, 2, "two slow listings spend the budget");
    }

    #[test]
    fn a_budget_not_spent_changes_nothing() {
        let mut seen = [true, false].into_iter();
        let listed = within_budget(|| Duration::ZERO, || seen.next());
        assert_eq!(
            settle_gone(Failure::Cancelled, listed, pause()),
            Error::Disconnected
        );
    }

    #[test]
    fn io_failure_of_a_meter_gone_from_usb_is_an_unplug() {
        let e = Error::Io("unknown (error 0xe00002ed)".into());
        assert_eq!(settle_gone(e, || Some(false), pause()), Error::Disconnected);
    }

    #[test]
    fn io_failure_waits_for_the_system_to_drop_the_meter() {
        let mut listed = [true, true, false].into_iter();
        let e = Error::Io("endpoint stalled".into());
        let got = settle_gone(e, || listed.next(), pause());
        assert_eq!(got, Error::Disconnected);
    }

    #[test]
    fn io_failure_of_a_meter_still_listed_stays_an_io_failure() {
        let e = Error::Io("endpoint stalled".into());
        let mut looks = 0;
        let got = settle_gone(
            e.clone(),
            || {
                looks += 1;
                Some(true)
            },
            pause(),
        );
        assert_eq!(got, e);
        assert_eq!(looks, GONE_LOOKS);
    }

    #[test]
    fn io_failure_stays_when_usb_cannot_be_listed() {
        let e = Error::Io("endpoint stalled".into());
        assert_eq!(settle_gone(e.clone(), || None, pause()), e);
    }

    #[test]
    fn other_errors_do_not_look_at_usb() {
        let got = settle_gone(Error::Timeout, || panic!("listed USB"), pause());
        assert_eq!(got, Error::Timeout);
    }

    // ---- AC-AXHK2J.3: a cancelled transfer goes through the presence check ----

    /// How a transfer that ended cancelled settles for this listing.
    fn cancelled(listed: impl FnMut() -> Option<bool>) -> Error {
        settle_gone(transfer_error(TransferError::Cancelled), listed, pause())
    }

    #[test]
    fn a_cancelled_transfer_of_a_meter_gone_from_usb_is_an_unplug() {
        assert_eq!(cancelled(|| Some(false)), Error::Disconnected);
    }

    #[test]
    fn a_cancelled_transfer_waits_for_the_system_to_drop_the_meter() {
        let mut listed = [true, true, false].into_iter();
        assert_eq!(cancelled(|| listed.next()), Error::Disconnected);
    }

    #[test]
    fn a_cancelled_transfer_settles_as_an_unplug_within_a_second() {
        let looks = std::cell::Cell::new(0);
        let waited = std::cell::Cell::new(Duration::ZERO);
        let got = settle_gone(
            transfer_error(TransferError::Cancelled),
            || {
                looks.set(looks.get() + 1);
                // The listing drops the meter only on the last look.
                Some(looks.get() < GONE_LOOKS)
            },
            || waited.set(waited.get() + GONE_PAUSE),
        );
        assert_eq!(got, Error::Disconnected);
        assert!(
            waited.get() < Duration::from_secs(1),
            "settling took {:?}",
            waited.get()
        );
    }

    #[test]
    fn a_cancelled_transfer_of_a_meter_still_listed_stays_a_timeout() {
        assert_eq!(cancelled(|| Some(true)), Error::Timeout);
    }

    #[test]
    fn a_cancelled_transfer_stays_a_timeout_when_usb_cannot_be_listed() {
        assert_eq!(cancelled(|| None), Error::Timeout);
    }

    // ---- AC-AXHK2J.8: a short wait never looks at USB ----

    /// How a failed transfer that waited `wait` settles, with how many times
    /// it listed USB and paused. The meter is listed on the first look and
    /// gone on the second, so a check that runs shows up in all three results.
    fn settled(e: TransferError, wait: Duration) -> (Error, usize, usize) {
        let mut looks = 0;
        let mut pauses = 0;
        let got = settle_failed(
            e,
            wait,
            || {
                looks += 1;
                Some(looks < 2)
            },
            || pauses += 1,
        );
        (got, looks, pauses)
    }

    fn ms(wait: u64) -> Duration {
        Duration::from_millis(wait)
    }

    #[test]
    fn a_cancelled_10_ms_wait_is_a_plain_timeout_without_a_look_or_a_pause() {
        assert_eq!(
            settled(TransferError::Cancelled, ms(10)),
            (Error::Timeout, 0, 0)
        );
    }

    #[test]
    fn a_cancelled_20_ms_wait_is_a_plain_timeout_without_a_look_or_a_pause() {
        assert_eq!(
            settled(TransferError::Cancelled, ms(20)),
            (Error::Timeout, 0, 0)
        );
    }

    #[test]
    fn a_cancelled_99_ms_wait_skips_the_presence_check() {
        assert_eq!(
            settled(TransferError::Cancelled, ms(99)),
            (Error::Timeout, 0, 0)
        );
    }

    #[test]
    fn a_cancelled_100_ms_wait_runs_the_presence_check() {
        assert_eq!(
            settled(TransferError::Cancelled, ms(100)),
            (Error::Disconnected, 2, 1)
        );
    }

    #[test]
    fn the_presence_check_starts_at_100_ms() {
        assert_eq!(GONE_CHECK_MIN_WAIT, ms(100));
    }

    #[test]
    fn a_short_wait_still_looks_at_usb_for_an_io_failure() {
        assert_eq!(
            settled(TransferError::Fault, ms(10)),
            (Error::Disconnected, 2, 1)
        );
    }

    const T: Duration = Duration::from_millis(50);

    // ---- AC-SJGK7E.4: ZLP planning and drain-at-open ----

    #[test]
    fn zlp_only_after_non_zero_multiples_of_64() {
        for len in [64, 128, 192, 4096] {
            assert!(needs_zlp(len, MAX_PACKET), "len {len}");
        }
        for len in [0, 1, 4, 63, 65, 127, 129] {
            assert!(!needs_zlp(len, MAX_PACKET), "len {len}");
        }
    }

    #[test]
    fn zlp_follows_the_given_packet_size() {
        assert!(needs_zlp(32, 32));
        assert!(!needs_zlp(32, 64));
    }

    #[test]
    fn drain_discards_stale_disconnect_then_next_transact_is_fresh() {
        let req = [0x0C, 0x01, 0x02, 0x00];
        let mut m = MockTransport::new(vec![
            Step::read(vec![0x03, 0x05, 0x00, 0x00]),
            Step::read_err(Error::Timeout),
            Step::transact(req, vec![0x41, 0x01, 0x80, 0x02]),
        ]);
        assert_eq!(drain_stale(&mut m, T), Ok(1));
        assert_eq!(m.transact(&req, T).unwrap(), vec![0x41, 0x01, 0x80, 0x02]);
        assert_eq!(m.remaining(), 0);
    }

    #[test]
    fn drain_with_nothing_stale_discards_zero() {
        let mut m = MockTransport::new(vec![Step::read_err(Error::Timeout)]);
        assert_eq!(drain_stale(&mut m, T), Ok(0));
        assert!(m.written().is_empty());
    }

    #[test]
    fn drain_discards_several_stale_transfers() {
        let mut m = MockTransport::new(vec![
            Step::read(vec![0x03, 0x05, 0x00, 0x00]),
            Step::read(vec![0x41, 0x06, 0x00, 0x00]),
            Step::read_err(Error::Timeout),
        ]);
        assert_eq!(drain_stale(&mut m, T), Ok(2));
        assert_eq!(m.remaining(), 0);
    }

    #[test]
    fn drain_propagates_disconnect() {
        let mut m = MockTransport::new(vec![Step::read_err(Error::Disconnected)]);
        assert_eq!(drain_stale(&mut m, T), Err(Error::Disconnected));
    }
}
