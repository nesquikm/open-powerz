//! The simulated bootloader (FR V9SK54, AC .8): mode switches on
//! JUMP_DFU / JUMP_APROM, the 0x42 descriptor, the erase wait, the raw
//! payload writes acked by SYNC and the closing FINISHED, plus injected
//! [`FlashFault`]s at each step. [`SimMeter`] is the engine's
//! [`MeterOpener`] over one simulated meter.
//!
//! Test infrastructure only: it implements the library's flash traits so
//! the engine can be driven without a real device.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::flash::{FlashTransport, Meter, MeterOpener};
use powerz_protocol::transport::discovery::{Model, PID_KM003C};
use powerz_protocol::transport::{Error, Transport};

use crate::SimDevice;
use crate::device::msg;

/// Bootloader reply and request types `[static]`.
const SYNC: u8 = 0x01;
const FINISHED: u8 = 0x07;
const JUMP_APROM: u8 = 0x08;
const JUMP_DFU: u8 = 0x09;
pub(crate) const DESCRIPTOR: u8 = 0x42;
/// Length of the descriptor body after its 4-byte header.
const DESCRIPTOR_LEN: usize = 64;
/// A read during the erase that waits at least this long sees the erase
/// end (the engine's short waits are 10 ms, its final wait 1000 ms).
const ERASE_DONE_WAIT: Duration = Duration::from_millis(100);

/// A failure the simulated bootloader injects, one per flash step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashFault {
    /// The meter ignores JUMP_DFU and stays in application mode.
    JumpDfuIgnored,
    /// CONNECT in bootloader mode gets no reply.
    ConnectNoAccept,
    /// The descriptor is answered with REJECT instead of left unanswered (an
    /// ACCEPT with its id is the real erase-done signal `[device]`).
    DescriptorAnswered,
    /// ACCEPT arrives on the first erase wait, before the erase is done.
    EarlyAcceptDuringErase,
    /// The erase never ends in ACCEPT.
    NoAcceptAfterErase,
    /// Payload write number `write` (1-based) gets no SYNC.
    MissingSync {
        /// The unacknowledged write.
        write: usize,
    },
    /// The bootloader never reports FINISHED.
    NoFinished,
    /// The verify wait gets a reply other than FINISHED.
    WrongVerifyReply,
    /// The meter ignores JUMP_APROM and stays in its bootloader.
    JumpApromIgnored,
    /// The download completes, but the application keeps reporting the
    /// old version: the image did not take.
    ImageNotTaken,
    /// HEAD in bootloader mode is answered with a reply of another type.
    HeadWrongType,
    /// Payload write number `write` (1-based) fails with an I/O error.
    WriteFails {
        /// The failing write.
        write: usize,
    },
    /// The JUMP_DFU raw write fails with an I/O error.
    JumpDfuWriteFails,
    /// After JUMP_APROM the application info block read is rejected.
    InfoReadFailsAfterReturn,
    /// The JUMP_DFU raw write reports `Disconnected` (the meter reset as
    /// it jumped), but the meter still switches to its bootloader.
    JumpDfuDisconnects,
    /// The JUMP_APROM raw write reports `Disconnected` (the meter reset as
    /// it jumped), but the meter still returns to its application.
    JumpApromDisconnects,
}

/// Where the simulated bootloader is in the flash sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    /// Waiting for a descriptor.
    #[default]
    Idle,
    /// Descriptor received; erasing.
    Erasing,
    /// Erase done; taking payload writes.
    Data,
    /// FINISHED sent (or the sequence failed).
    Done,
}

/// State of the simulated bootloader.
#[derive(Debug, Default)]
pub(crate) struct Bootloader {
    phase: Phase,
    /// The last descriptor body received.
    descriptor: Option<Vec<u8>>,
    /// Payload bytes received since that descriptor.
    payload: Vec<u8>,
    /// Payload writes since that descriptor.
    writes: usize,
    /// Reads since that descriptor while erasing (the first one is the
    /// descriptor wait, the erase waits follow).
    erase_reads: usize,
    /// The meter came back to its application through JUMP_APROM.
    returned: bool,
}

impl Bootloader {
    /// Declared payload size from the descriptor (offset 0x08).
    fn size(&self) -> usize {
        self.descriptor
            .as_ref()
            .and_then(|d| d.get(0x08..0x0C))
            .map_or(usize::MAX, |w| {
                u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize
            })
    }

    /// Firmware version named by the descriptor (offset 0x1C, 12 bytes).
    fn version(&self) -> Option<String> {
        let field = self.descriptor.as_ref()?.get(0x1C..0x28)?;
        let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
        Some(String::from_utf8_lossy(&field[..end]).into_owned())
    }
}

impl SimDevice {
    /// Inject `fault` into the simulated bootloader.
    pub fn with_flash_fault(mut self, fault: FlashFault) -> SimDevice {
        self.flash_fault = Some(fault);
        self
    }

    fn fault(&self, f: FlashFault) -> bool {
        self.flash_fault == Some(f)
    }

    /// Make jumps take effect at the next power-up ([`SimMeter::replug`])
    /// instead of live, as JUMP_DFU does on application firmware 2.0.6
    /// `[device]`.
    pub fn with_jump_at_power_up(mut self) -> SimDevice {
        self.jump_at_power_up = true;
        self
    }

    /// Switch to `mode` through a jump: live, or at the next power-up.
    fn jump_to(&mut self, mode: DeviceMode) {
        if self.jump_at_power_up {
            self.power_up_mode = Some(mode);
        } else {
            self.enter(mode);
            // The new firmware's stale frames belong to the next pipe, not
            // to the one the jump was written on.
            self.pending.clear();
        }
    }

    /// Start running `mode` (a fresh pipe).
    fn enter(&mut self, mode: DeviceMode) {
        if mode == DeviceMode::Application && self.mode == DeviceMode::Bootloader {
            self.boot.returned = true;
        }
        self.mode = mode;
        self.reopen();
    }

    /// Unplug the meter and plug it back in: it starts in the mode a jump
    /// left for the next power-up, else in its application (a plain
    /// replug leaves the bootloader `[device]`).
    pub(crate) fn power_cycle(&mut self) {
        let mode = self.power_up_mode.take().unwrap_or(DeviceMode::Application);
        self.enter(mode);
    }

    /// The meter re-enumerated: a fresh pipe with nothing half-done. In
    /// application mode the bulk pipe starts with the stale DISCONNECT.
    pub(crate) fn reopen(&mut self) {
        self.pending.clear();
        if self.mode == DeviceMode::Application && self.persona == crate::Persona::Bulk {
            self.pending
                .push_back(crate::device::STALE_DISCONNECT.to_vec());
        }
        self.connected = false;
        self.auth_level = 0;
        self.streaming = false;
        self.boot.phase = Phase::Idle;
    }

    /// Handle one request in bootloader mode.
    pub(crate) fn bootloader_request(&mut self, req: &[u8], id: u8) -> Vec<Vec<u8>> {
        match req.first().map(|b| b & 0x7F) {
            Some(msg::CONNECT) => {
                if self.fault(FlashFault::ConnectNoAccept) {
                    return Vec::new();
                }
                self.connected = true;
                vec![self.control(msg::ACCEPT, id)]
            }
            Some(msg::HEAD) if self.fault(FlashFault::HeadWrongType) => {
                vec![self.control(msg::ACCEPT, id)]
            }
            Some(msg::HEAD) => vec![self.head(id)],
            Some(DESCRIPTOR) if self.connected && req.len() == 4 + DESCRIPTOR_LEN => {
                self.descriptor(req, id)
            }
            _ => vec![self.control(msg::REJECT, id)],
        }
    }

    /// The 0x42 descriptor: start the erase. Normally unanswered.
    fn descriptor(&mut self, req: &[u8], id: u8) -> Vec<Vec<u8>> {
        self.boot = Bootloader {
            phase: Phase::Erasing,
            descriptor: Some(req[4..].to_vec()),
            payload: Vec::new(),
            writes: 0,
            erase_reads: 0,
            returned: false,
        };
        if self.fault(FlashFault::DescriptorAnswered) {
            vec![self.control(msg::REJECT, id)]
        } else {
            Vec::new()
        }
    }

    /// A read in bootloader mode with nothing pending: the erase wait and
    /// the verify wait.
    pub(crate) fn bootloader_read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        match self.boot.phase {
            Phase::Erasing => {
                if self.fault(FlashFault::NoAcceptAfterErase) {
                    return Err(Error::Timeout);
                }
                self.boot.erase_reads += 1;
                let early =
                    self.boot.erase_reads > 1 && self.fault(FlashFault::EarlyAcceptDuringErase);
                if timeout >= ERASE_DONE_WAIT || early {
                    self.boot.phase = Phase::Data;
                    return Ok(self.control(msg::ACCEPT, 0));
                }
                Err(Error::Timeout)
            }
            Phase::Data if self.boot.payload.len() >= self.boot.size() => {
                self.boot.phase = Phase::Done;
                if self.fault(FlashFault::NoFinished) {
                    return Err(Error::Timeout);
                }
                if self.fault(FlashFault::WrongVerifyReply) {
                    return Ok(self.control(msg::REJECT, 0));
                }
                if !self.fault(FlashFault::ImageNotTaken)
                    && let Some(v) = self.boot.version()
                {
                    self.app_version = v;
                }
                Ok(self.control(FINISHED, 0))
            }
            _ => Err(Error::Timeout),
        }
    }

    /// Whether the application info block read is rejected: after the
    /// return from the bootloader, with [`FlashFault::InfoReadFailsAfterReturn`].
    pub(crate) fn info_read_fails(&self) -> bool {
        self.boot.returned && self.fault(FlashFault::InfoReadFailsAfterReturn)
    }
}

/// Whether `data` is a write-only jump of type `ty`.
fn is_jump(data: &[u8], ty: u8) -> bool {
    data.len() == 4 && data[0] == ty && data[2..4] == [0, 0]
}

/// Raw writes: the jumps switch modes; in the bootloader's data phase each
/// write is payload, acknowledged with SYNC; before it, a 0x42 descriptor
/// starts the erase.
impl FlashTransport for SimDevice {
    fn write_raw(&mut self, data: &[u8], _timeout: Duration) -> Result<(), Error> {
        if let Some(e) = self.blocked() {
            return Err(e);
        }
        if is_jump(data, JUMP_DFU) {
            if self.fault(FlashFault::JumpDfuWriteFails) {
                return Err(Error::Io("simulated JUMP_DFU write failure".into()));
            }
            // Application firmware 2.0.6 REJECTs a JUMP_DFU from a session
            // that has not authenticated at level 1 `[device]`.
            if self.mode == DeviceMode::Application && self.auth_level < 1 {
                let reject = self.control(msg::REJECT, data[1]);
                self.pending.push_back(reject);
                return Ok(());
            }
            if self.mode == DeviceMode::Application && !self.fault(FlashFault::JumpDfuIgnored) {
                self.jump_to(DeviceMode::Bootloader);
            }
            if self.fault(FlashFault::JumpDfuDisconnects) {
                return Err(Error::Disconnected);
            }
        } else if is_jump(data, JUMP_APROM) {
            if self.mode == DeviceMode::Bootloader && !self.fault(FlashFault::JumpApromIgnored) {
                self.jump_to(DeviceMode::Application);
            }
            if self.fault(FlashFault::JumpApromDisconnects) {
                return Err(Error::Disconnected);
            }
        } else if self.mode == DeviceMode::Bootloader && self.boot.phase == Phase::Data {
            self.boot.writes += 1;
            let write = self.boot.writes;
            if self.fault(FlashFault::WriteFails { write }) {
                self.boot.phase = Phase::Done;
                return Err(Error::Io(format!(
                    "simulated failure of payload write {write}"
                )));
            }
            self.boot.payload.extend_from_slice(data);
            if self.fault(FlashFault::MissingSync { write }) {
                self.boot.phase = Phase::Done;
            } else {
                let sync = self.control(SYNC, 0);
                self.pending.push_back(sync);
            }
        } else if self.mode == DeviceMode::Bootloader
            && self.connected
            && data.len() == 4 + DESCRIPTOR_LEN
            && data[0] == DESCRIPTOR
        {
            let replies = self.descriptor(data, data[1]);
            self.pending.extend(replies);
        }
        Ok(())
    }
}

/// A simulated meter as the flash engine's [`MeterOpener`]: every open is a
/// re-enumeration of the same [`SimDevice`]. Clones share the meter. Every
/// frame and raw write sent through its pipes is recorded.
#[derive(Debug, Clone)]
pub struct SimMeter {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Debug)]
struct Shared {
    device: SimDevice,
    writes: Vec<Vec<u8>>,
    /// The user replugs the meter once this many opens have looked at it
    /// since a jump left a mode for the next power-up.
    replug_after: Option<usize>,
    /// Opens since then.
    opens_waiting: usize,
}

impl SimMeter {
    /// Wrap `device`.
    pub fn new(device: SimDevice) -> SimMeter {
        SimMeter {
            shared: Arc::new(Mutex::new(Shared {
                device,
                writes: Vec::new(),
                replug_after: None,
                opens_waiting: 0,
            })),
        }
    }

    /// Stand in for the user: once a jump has left a mode for the next
    /// power-up, replug the meter after `opens` more looks at it.
    pub fn with_replug_after(self, opens: usize) -> SimMeter {
        self.lock().replug_after = Some(opens);
        self
    }

    /// Unplug the meter and plug it back in.
    pub fn replug(&self) {
        self.lock().device.power_cycle();
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What the meter is running now.
    pub fn mode(&self) -> DeviceMode {
        self.lock().device.mode
    }

    /// The application firmware version the info block reports.
    pub fn app_version(&self) -> String {
        self.lock().device.app_version.clone()
    }

    /// Every request and raw write sent so far, in order.
    pub fn writes(&self) -> Vec<Vec<u8>> {
        self.lock().writes.clone()
    }

    /// The last 64-byte descriptor body the bootloader received.
    pub fn received_descriptor(&self) -> Option<Vec<u8>> {
        self.lock().device.boot.descriptor.clone()
    }

    /// The payload bytes received since that descriptor.
    pub fn received_payload(&self) -> Vec<u8> {
        self.lock().device.boot.payload.clone()
    }

    /// Remove the injected flash fault.
    pub fn clear_flash_fault(&self) {
        self.lock().device.flash_fault = None;
    }
}

impl MeterOpener for SimMeter {
    type Pipe = SimMeterPipe;

    fn open(&mut self) -> Result<Option<Meter<SimMeterPipe>>, Error> {
        let mode = {
            let mut s = self.lock();
            if s.device.power_up_mode.is_some()
                && let Some(after) = s.replug_after
            {
                s.opens_waiting += 1;
                if s.opens_waiting >= after {
                    s.opens_waiting = 0;
                    s.device.power_cycle();
                }
            }
            s.device.reopen();
            // Like the real opener, drain stale data (the application's
            // DISCONNECT) at open.
            s.device.pending.clear();
            s.device.mode
        };
        Ok(Some(Meter {
            mode,
            model: Model::from_pid(PID_KM003C),
            pipe: SimMeterPipe {
                meter: self.clone(),
            },
        }))
    }
}

/// The vendor bulk pipe of an opened [`SimMeter`].
#[derive(Debug)]
pub struct SimMeterPipe {
    meter: SimMeter,
}

impl Transport for SimMeterPipe {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        let mut s = self.meter.lock();
        s.writes.push(request.to_vec());
        s.device.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        self.meter.lock().device.read(timeout)
    }
}

impl FlashTransport for SimMeterPipe {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), Error> {
        let mut s = self.meter.lock();
        s.writes.push(data.to_vec());
        s.device.write_raw(data, timeout)
    }
}
