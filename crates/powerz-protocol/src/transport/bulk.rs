//! Vendor bulk transport on `nusb` (the default path).

use std::time::Duration;

use nusb::Endpoint;
use nusb::MaybeFuture;
use nusb::transfer::{Buffer, Bulk, In, Out, TransferError};

use super::discovery::{self, DeviceEntry};
use super::select::{EndpointDesc, InterfaceDesc, select_interfaces};
use super::{Error, GuardedTransport, Transport, map_io_error};
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
                    && d.product_id() == device.pid
                    && d.bus_id() == device.bus
                    && d.device_address() == device.address
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
        };
        drain_stale(&mut t, DRAIN_TIMEOUT)?;
        Ok(t)
    }

    fn write(&mut self, data: &[u8], timeout: Duration) -> Result<(), Error> {
        let c = self
            .out_ep
            .transfer_blocking(Buffer::from(data.to_vec()), timeout);
        c.status.map_err(transfer_error)?;
        if needs_zlp(data.len(), self.max_packet) {
            let c = self.out_ep.transfer_blocking(Buffer::new(0), timeout);
            c.status.map_err(transfer_error)?;
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
        c.status.map_err(transfer_error)?;
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
    map_io_error(&std::io::Error::from(e), std::env::consts::OS)
}

fn transfer_error(e: TransferError) -> Error {
    match e {
        TransferError::Cancelled => Error::Timeout,
        TransferError::Disconnected => Error::Disconnected,
        other => Error::Io(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::{MockTransport, Step};

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
