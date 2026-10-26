//! The firmware flashing engine (FR V9SK54): writes a verified firmware
//! container onto the meter's application area through the bootloader.
//!
//! This is the only path in the library that can send write-capable frames
//! (the 0x42 descriptor, payload writes, JUMP_APROM / JUMP_DFU). It needs a
//! [`FlashAck`], which only [`FlashAck::acknowledge`] mints, and a
//! [`FlashTransport`] with [`write_raw`](FlashTransport::write_raw), which
//! the shared [`GuardedTransport`] does
//! not implement. The shared guard is unchanged for every other path.
//!
//! The bootloader sequence is `[static]` (see `docs/protocol.md`): CONNECT
//! → ACCEPT (an unanswered first CONNECT is retried once after a recover,
//! as the real bootloader ignores it `[device]`); HEAD (field must be 0); the 0x42 descriptor as a raw write
//! (1000 ms write wait), then a separate 20 ms read that must time out; N−1 reads of 10 ms that must time out and a 1000 ms read
//! that must be ACCEPT; the payload in raw 4080-byte writes, each acked by
//! SYNC; FINISHED within 1000 ms. Nothing else is retried automatically.
//!
//! On application firmware 2.0.6 a jump takes effect at the next power-up,
//! not live `[device]`: after each jump the engine reports the meter's
//! reply, asks for a replug and waits long enough for one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::decode::DeviceInfoBlock;
use crate::decode::head::{self, HeadReply};
use crate::firmware::DeviceMode;
use crate::firmware::container::FirmwareContainer;
use crate::frame;
use crate::session::{Clock, Session};
use crate::transport::discovery::Model;
use crate::transport::{Error as TransportError, GuardedTransport, Transport};

/// First address of the application area; the bootloader lives below it
/// and is never written `[static]`.
pub const APP_START: u32 = 0x4000;
/// Bytes per payload write in the data phase (0xFF0) `[static]`.
pub const BLOCK_LEN: usize = 4080;
/// Highest end address a descriptor may name (512 KiB of flash)
/// `[inferred]`.
pub const MAX_APP_END: u32 = 0x0008_0000;
/// How long to wait for the meter to come back in bootloader mode after
/// JUMP_DFU. The vendor tool waits two seconds `[static]`, but on 2.0.6 the
/// jump takes effect at the next power-up `[device]`, so this leaves time
/// for the user to unplug the meter and plug it back in.
pub const SWITCH_TIMEOUT: Duration = Duration::from_secs(120);
/// How long to wait for the meter to come back in application mode after
/// JUMP_APROM, again with time for a replug `[inferred]`.
pub const RETURN_TIMEOUT: Duration = Duration::from_secs(120);
/// How long to wait for the meter's reply to a jump frame. The vendor tool
/// reads none `[static]`; this one read only records what the meter says.
pub const JUMP_REPLY_WAIT: Duration = Duration::from_millis(500);

/// Pause between two looks for the re-enumerated meter.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// CONNECT reply wait in the bootloader `[static]`.
const CONNECT_WAIT: Duration = Duration::from_millis(1000);
/// HEAD reply wait.
const HEAD_WAIT: Duration = Duration::from_millis(1000);
/// The read after the descriptor write, expected to expire `[static]`.
const DESCRIPTOR_WAIT: Duration = Duration::from_millis(20);
/// Each of the N−1 erase waits, expected to expire `[static]`.
const ERASE_SHORT_WAIT: Duration = Duration::from_millis(10);
/// The final erase wait, which must end in ACCEPT `[static]`.
const ERASE_FINAL_WAIT: Duration = Duration::from_millis(1000);
/// Timeout of one raw write (the descriptor, a payload block, a jump).
const WRITE_WAIT: Duration = Duration::from_millis(1000);
/// SYNC wait after each payload write.
const SYNC_WAIT: Duration = Duration::from_millis(1000);
/// FINISHED wait after the last payload write `[static]`.
const FINISHED_WAIT: Duration = Duration::from_millis(1000);

/// Reply types the bootloader answers with `[static]`.
const SYNC: u8 = 0x01;
const ACCEPT: u8 = 0x05;
const REJECT: u8 = 0x06;
const FINISHED: u8 = 0x07;
/// JUMP_APROM frame type `[static]`.
const JUMP_APROM: u8 = 0x08;
/// JUMP_DFU frame type `[static]`.
const JUMP_DFU: u8 = 0x09;
/// HEAD frame type.
const HEAD: u8 = 0x40;
/// 0x42 descriptor frame type.
const DESCRIPTOR: u8 = 0x42;
/// Length of the descriptor body.
const DESCRIPTOR_LEN: usize = 64;
/// Width of each ASCII field in the descriptor.
const FIELD_LEN: usize = 12;
/// The application info block.
const APP_INFO: (u32, u32) = (0x4420, 64);

/// Proof that the user explicitly asked for a firmware flash.
///
/// This is a type-state marker that prevents accidental use: code cannot
/// reach the flash engine without having called [`FlashAck::acknowledge`]
/// on purpose. It is not a capability against a determined caller, since
/// any code linked against the library can mint one.
#[derive(Debug)]
pub struct FlashAck {
    _private: (),
}

impl FlashAck {
    /// Record the user's explicit acknowledgement that flashing may brick
    /// the application firmware until it is retried.
    pub fn acknowledge() -> FlashAck {
        FlashAck { _private: () }
    }
}

/// A pipe that can also write raw bytes with no reply awaited: the payload
/// writes and the write-only jumps. Only the flash path uses it.
pub trait FlashTransport: Transport {
    /// Write `data` as one transfer and wait for no reply. A write whose
    /// length is a multiple of the packet size is ended with a zero-length
    /// packet by the implementation.
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), TransportError>;
}

/// An opened meter as the flash engine sees it.
#[derive(Debug)]
pub struct Meter<P> {
    /// What it is running.
    pub mode: DeviceMode,
    /// Its model, from the USB product ID.
    pub model: Model,
    /// Its vendor bulk pipe.
    pub pipe: P,
}

/// Finds and opens the meter, again after each re-enumeration.
pub trait MeterOpener {
    /// The pipe type of an opened meter.
    type Pipe: FlashTransport;

    /// Open the meter as the device list shows it now; `Ok(None)` while no
    /// meter is attached.
    fn open(&mut self) -> Result<Option<Meter<Self::Pipe>>, TransportError>;
}

/// The strict guard of the flash path: only CONNECT, HEAD, the application
/// info read, the jumps, a descriptor inside the application area and,
/// after it, payload writes reach the pipe. Anything else is refused with
/// [`TransportError::Forbidden`] before a byte is written.
///
/// A descriptor passes only when it starts at [`APP_START`], its end is
/// start + size, its size is not zero and its end is at most
/// [`MAX_APP_END`]: nothing below 0x4000, where the bootloader lives, can
/// be named.
///
/// Payload writes are bounded once armed: each raw write is at most
/// [`BLOCK_LEN`] bytes, and their running total may not pass the
/// descriptor's size + [`BLOCK_LEN`] (room for the block padding). A
/// JUMP_APROM write or a new CONNECT disarms the guard; a new valid
/// descriptor re-arms it with a fresh budget. Every refusal happens before
/// anything reaches the wrapped pipe.
///
/// The shared [`GuardedTransport`] is not a [`FlashTransport`], so it
/// cannot be handed to the flash path:
///
/// ```compile_fail
/// use powerz_protocol::firmware::flash::FlashTransport;
/// use powerz_protocol::transport::GuardedTransport;
///
/// fn needs_flash<P: FlashTransport>(_pipe: P) {}
///
/// fn try_it<T: powerz_protocol::transport::Transport>(pipe: GuardedTransport<T>) {
///     needs_flash(pipe);
/// }
/// ```
#[derive(Debug)]
pub struct FlashGuard<T> {
    inner: T,
    /// Set once a valid descriptor has been sent: payload writes may follow,
    /// up to this many bytes in total (descriptor size + one block).
    armed: Option<usize>,
}

impl<T> FlashGuard<T> {
    /// Wrap `inner`.
    pub fn new(inner: T) -> FlashGuard<T> {
        FlashGuard { inner, armed: None }
    }

    /// The wrapped transport.
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

/// The frame type of `bytes` for a refusal (0 for an empty buffer).
fn forbidden(bytes: &[u8]) -> TransportError {
    TransportError::Forbidden {
        ty: frame::frame_type(bytes).unwrap_or(0),
    }
}

/// The declared size of `request` when it is a 0x42 descriptor inside the
/// application area.
fn valid_descriptor(request: &[u8]) -> Option<u32> {
    if request.len() != 4 + DESCRIPTOR_LEN || request[0] != DESCRIPTOR || request[2..4] != [0, 4] {
        return None;
    }
    let word = |o: usize| {
        u32::from_le_bytes([
            request[4 + o],
            request[5 + o],
            request[6 + o],
            request[7 + o],
        ])
    };
    let (start, end, size) = (word(0x00), word(0x04), word(0x08));
    if start == APP_START && size != 0 && start.checked_add(size) == Some(end) && end <= MAX_APP_END
    {
        Some(size)
    } else {
        None
    }
}

/// Whether `request` is one of the fixed frames of the flash set: CONNECT,
/// HEAD or the application info block read.
fn allowed_fixed(request: &[u8]) -> bool {
    let Some(&id) = request.get(1) else {
        return false;
    };
    request == frame::connect(id)
        || request == head::request(id)
        || request == crate::session::memory_read_request(id, APP_INFO.0, APP_INFO.1)
}

/// Whether `data` is a write-only JUMP_DFU or JUMP_APROM frame.
fn is_jump(data: &[u8]) -> bool {
    data.len() == 4 && (data[0] == JUMP_APROM || data[0] == JUMP_DFU) && data[2..4] == [0, 0]
}

impl<T: FlashTransport> Transport for FlashGuard<T> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        if allowed_fixed(request) {
            if request.first() == Some(&frame::msg::CONNECT) {
                self.armed = None;
            }
        } else {
            return Err(forbidden(request));
        }
        self.inner.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        self.inner.recover()
    }
}

impl<T: FlashTransport> FlashTransport for FlashGuard<T> {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), TransportError> {
        if is_jump(data) {
            if data[0] == JUMP_APROM {
                self.armed = None;
            }
            return self.inner.write_raw(data, timeout);
        }
        if let Some(size) = valid_descriptor(data) {
            self.armed = Some(size as usize + BLOCK_LEN);
            return self.inner.write_raw(data, timeout);
        }
        let Some(budget) = self.armed else {
            return Err(forbidden(data));
        };
        if data.is_empty() || data.len() > BLOCK_LEN || data.len() > budget {
            return Err(forbidden(data));
        }
        // Debit before writing: a failed write may still have sent bytes.
        self.armed = Some(budget - data.len());
        self.inner.write_raw(data, timeout)
    }
}

/// A stage of the flash, for progress display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlashProgress {
    /// Switching the meter into its bootloader.
    Switching,
    /// Erasing the application area.
    Erasing,
    /// Writing the payload.
    Writing {
        /// Bytes acknowledged so far.
        done: usize,
        /// Payload length.
        total: usize,
    },
    /// Waiting for the bootloader's verdict.
    Verifying,
    /// The bootloader accepted the image.
    Finished,
    /// Returning the meter to application mode.
    Returning,
    /// A jump to `to` was sent and answered with `reply`: unplug the meter
    /// and plug it back in if it does not restart by itself. Sent after
    /// [`Switching`](Self::Switching) and after [`Returning`](Self::Returning).
    Replug {
        /// The mode the meter should come back in.
        to: DeviceMode,
        /// The meter's reply to the jump frame.
        reply: JumpReply,
    },
}

/// How a successful flash ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlashOutcome {
    /// The meter is back in application mode and reports this version.
    Verified {
        /// Application firmware version from the info block.
        version: String,
    },
    /// The image was accepted, but the meter stayed in its bootloader:
    /// unplug it and plug it back in.
    PowerCycleNeeded,
}

/// Why a flash stopped. Each step has its own variant; [`code`](Self::code)
/// gives the vendor error number.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FlashError {
    /// The meter is not in bootloader mode.
    #[error("the meter is not in bootloader mode ({seen})")]
    NotInBootloader {
        /// What was seen instead.
        seen: String,
    },
    /// The file does not fit this meter, or the meter is not one we know.
    #[error("product error: {detail}")]
    Product {
        /// What did not match.
        detail: String,
    },
    /// CONNECT or HEAD got no usable answer.
    #[error("read error: {detail}")]
    Read {
        /// What went wrong.
        detail: String,
    },
    /// HEAD reported a non-zero work-mode field (only seen in bootloader
    /// mode, so the message always carries the recovery text).
    #[error("work mode error: HEAD field is {field:#x}, expected 0 ({reply}); {RECOVERY}")]
    WorkMode {
        /// The HEAD field.
        field: u32,
        /// The raw HEAD reply, as `HEAD reply <spaced hex>`.
        reply: String,
    },
    /// The descriptor could not be written or was answered.
    #[error("write error: {detail}")]
    Write {
        /// What went wrong.
        detail: String,
    },
    /// The erase wait did not end in ACCEPT.
    #[error("erase error: {detail}")]
    Erase {
        /// What went wrong.
        detail: String,
    },
    /// A payload write failed or was not acknowledged with SYNC.
    #[error("download error at write {write}: {detail}")]
    Download {
        /// The failing write, 1-based.
        write: usize,
        /// What went wrong.
        detail: String,
    },
    /// The bootloader did not report FINISHED.
    #[error("verify error: {detail}")]
    Verify {
        /// What went wrong.
        detail: String,
    },
    /// The meter could not be found or opened.
    #[error("device error: {detail}")]
    Device {
        /// What went wrong.
        detail: String,
    },
    /// The user stopped the flash.
    #[error("interrupted: {detail}")]
    Interrupted {
        /// Where it stopped.
        detail: String,
    },
    /// A [`jump`] to the mode the meter is already in; nothing was sent.
    #[error("the meter is already in {} mode; nothing was sent", mode.name())]
    AlreadyInMode {
        /// The mode it is in.
        mode: DeviceMode,
    },
    /// A [`jump`] from a mode that is neither application nor bootloader;
    /// nothing was sent.
    #[error("the meter's mode is unknown, so no jump is chosen; nothing was sent")]
    UnknownMode,
}

impl FlashError {
    /// The vendor error number of this failure `[static]`.
    pub fn code(&self) -> i32 {
        match self {
            FlashError::Product { .. } => -100,
            FlashError::Verify { .. } => -103,
            FlashError::Erase { .. } => -105,
            FlashError::Download { .. } => -106,
            FlashError::Write { .. } => -107,
            FlashError::Read { .. } => -108,
            // The vendor app has no stop of its own `[inferred]`; a user stop
            // shares its device/abort code.
            FlashError::NotInBootloader { .. }
            | FlashError::Device { .. }
            | FlashError::Interrupted { .. }
            | FlashError::AlreadyInMode { .. }
            | FlashError::UnknownMode => -110,
            FlashError::WorkMode { .. } => -111,
        }
    }
}

/// Flash `container` onto the meter that `opener` finds.
///
/// Runs the bootloader sequence, then sends JUMP_APROM and reads the
/// application info block to confirm the new version. Nothing is retried.
/// `stop` is the user's request to stop the flash. It is honoured only at
/// safe boundaries: before JUMP_DFU, before the descriptor and after each
/// payload write's SYNC read (never between a write and its SYNC). Once the
/// meter is in its bootloader a stop sends nothing more and leaves it there.
pub fn flash<O: MeterOpener, C: Clock>(
    _ack: FlashAck,
    container: &FirmwareContainer<'_>,
    opener: &mut O,
    mut clock: C,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(FlashProgress),
) -> Result<FlashOutcome, FlashError> {
    // Refuse a file the guard would refuse, before anything write-capable
    // (JUMP_DFU, the descriptor) is sent.
    check_container(container)?;
    let meter = opener.open().map_err(device)?.ok_or(FlashError::Device {
        detail: "no meter found".into(),
    })?;
    if stopped(stop) {
        let e = interrupted("before the update started");
        return Err(match meter.mode {
            DeviceMode::Bootloader => in_bootloader(e),
            _ => e,
        });
    }
    let meter = match meter.mode {
        DeviceMode::Bootloader => meter,
        DeviceMode::Application => {
            progress(FlashProgress::Switching);
            switch_to_bootloader(meter, container, opener, &mut clock, progress)?
        }
        DeviceMode::Unknown => {
            return Err(FlashError::NotInBootloader {
                seen: meter.mode.name().into(),
            });
        }
    };
    // From here the meter is in bootloader mode. A failure stops the flash
    // there: nothing more is sent (no retry, no JUMP_APROM) and the error
    // says how to retry.
    check_model(meter.model, container).map_err(in_bootloader)?;
    let mut ids = Ids(0);
    // Every frame of the flash goes through the strict flash guard.
    let mut pipe = FlashGuard::new(meter.pipe);
    download(&mut pipe, container, &mut clock, &mut ids, stop, progress).map_err(in_bootloader)?;
    progress(FlashProgress::Returning);
    // A failed jump leaves the meter in its bootloader.
    let reply = send_jump(&mut pipe, JUMP_APROM, ids.next()).map_err(in_bootloader)?;
    progress(FlashProgress::Replug {
        to: DeviceMode::Application,
        reply,
    });
    drop(pipe);
    let mut app = match wait_for(opener, &mut clock, DeviceMode::Application, RETURN_TIMEOUT) {
        Ok(app) => app,
        Err(None) => return Ok(FlashOutcome::PowerCycleNeeded),
        // FINISHED has already confirmed the image, so say so plainly.
        Err(Some(e)) => {
            return Err(FlashError::Device {
                detail: format!(
                    "the image was written and verified, but the meter could not be found \
                     again because {e}; unplug it and plug it back in, then check its version"
                ),
            });
        }
    };
    let verify = |detail: String| FlashError::Verify {
        detail: format!("reading the application info block: {detail}"),
    };
    let info = read_app_info(&mut app.pipe, &mut clock).map_err(verify)?;
    let info = info.ok_or_else(|| verify("the block is blank".into()))?;
    let (seen, wanted) = (info.version().trim(), container.firmware_version().trim());
    if seen != wanted {
        return Err(FlashError::Verify {
            detail: format!(
                "the meter reports version {seen} after the update but the file is {wanted}; \
                 the image did not take, run the update again"
            ),
        });
    }
    Ok(FlashOutcome::Verified {
        version: seen.to_owned(),
    })
}

/// Whether the user asked to stop.
fn stopped(stop: &AtomicBool) -> bool {
    stop.load(Ordering::SeqCst)
}

/// The error for a user stop at `point`.
fn interrupted(point: &str) -> FlashError {
    FlashError::Interrupted {
        detail: format!("stopped by the user {point}"),
    }
}

/// How to recover from a failure in bootloader mode, where the meter is
/// left: there is no automatic retry.
const RECOVERY: &str = "the meter stays in bootloader mode; to retry, run the update again, \
     or if the meter does not show its bootloader, hold Back while plugging it in";

/// Add the [`RECOVERY`] text to an error raised while the meter is in
/// bootloader mode.
fn in_bootloader(e: FlashError) -> FlashError {
    let with = |detail: String| format!("{detail}; {RECOVERY}");
    match e {
        FlashError::Product { detail } => FlashError::Product {
            detail: with(detail),
        },
        FlashError::Read { detail } => FlashError::Read {
            detail: with(detail),
        },
        FlashError::Write { detail } => FlashError::Write {
            detail: with(detail),
        },
        FlashError::Erase { detail } => FlashError::Erase {
            detail: with(detail),
        },
        FlashError::Download { write, detail } => FlashError::Download {
            write,
            detail: with(detail),
        },
        FlashError::Verify { detail } => FlashError::Verify {
            detail: with(detail),
        },
        FlashError::Device { detail } => FlashError::Device {
            detail: with(detail),
        },
        FlashError::Interrupted { detail } => FlashError::Interrupted {
            detail: with(detail),
        },
        FlashError::NotInBootloader { seen } => FlashError::NotInBootloader { seen },
        // Its message carries the recovery text already.
        e @ FlashError::WorkMode { .. } => e,
        // Raised by `jump` only, before anything is sent.
        e @ (FlashError::AlreadyInMode { .. } | FlashError::UnknownMode) => e,
    }
}

/// What to do when the meter does not reach its bootloader on its own.
pub const BACK_BUTTON_HINT: &str = "hold the Back button while plugging the meter in to start it \
     in bootloader mode, then retry";

/// From application mode: check the meter's model from its application
/// info block, send JUMP_DFU and wait for the meter to come back in
/// bootloader mode.
fn switch_to_bootloader<O: MeterOpener, C: Clock>(
    mut meter: Meter<O::Pipe>,
    container: &FirmwareContainer<'_>,
    opener: &mut O,
    clock: &mut C,
    progress: &mut dyn FnMut(FlashProgress),
) -> Result<Meter<O::Pipe>, FlashError> {
    let info = read_app_info(&mut meter.pipe, &mut *clock).map_err(|detail| FlashError::Read {
        detail: format!("reading the application info block: {detail}"),
    })?;
    let Some(info) = info else {
        return Err(FlashError::Product {
            detail: "the application info block is blank".into(),
        });
    };
    container
        .check_model(info.model())
        .map_err(|e| FlashError::Product {
            detail: e.to_string(),
        })?;
    let target = JumpTarget::Bootloader;
    let mut sent = |reply| {
        progress(FlashProgress::Replug {
            to: DeviceMode::Bootloader,
            reply,
        })
    };
    let seen = match jump_and_wait(meter.pipe, target, opener, clock, &mut sent)? {
        Waited::Reached(boot) => return Ok(boot),
        Waited::Seen(Some(mode)) => format!("{} mode", mode.name()),
        Waited::Seen(None) => "no meter".into(),
        Waited::OpenError(e) => format!("the meter could not be opened: {e}"),
    };
    Err(FlashError::NotInBootloader {
        seen: format!("{seen} after {}; {BACK_BUTTON_HINT}", target.frame_name()),
    })
}

/// How the wait after a jump ended.
enum Waited<P> {
    /// The meter came back in the target mode.
    Reached(Meter<P>),
    /// It did not; a last look shows this mode (`None`: no meter attached).
    Seen(Option<DeviceMode>),
    /// It did not, and opening it kept failing with this error (not "no
    /// device yet"); the last such error.
    OpenError(TransportError),
}

/// Send the jump frame for `target` through the strict flash guard, hand
/// the meter's reply to `sent`, wait for the meter to come back in `target`
/// mode (for [`SWITCH_TIMEOUT`] or [`RETURN_TIMEOUT`]) and, if it does not,
/// look once more at what is attached.
fn jump_and_wait<O: MeterOpener, C: Clock>(
    pipe: O::Pipe,
    target: JumpTarget,
    opener: &mut O,
    clock: &mut C,
    sent: &mut dyn FnMut(JumpReply),
) -> Result<Waited<O::Pipe>, FlashError> {
    let (ty, limit) = match target {
        JumpTarget::Bootloader => (JUMP_DFU, SWITCH_TIMEOUT),
        JumpTarget::Application => (JUMP_APROM, RETURN_TIMEOUT),
    };
    let mut pipe = FlashGuard::new(pipe);
    let reply = send_jump(&mut pipe, ty, 0x01)?;
    // Release the pipe before the meter re-enumerates.
    drop(pipe);
    sent(reply);
    Ok(match wait_for(opener, clock, target.mode(), limit) {
        Ok(m) => Waited::Reached(m),
        Err(Some(e)) => Waited::OpenError(e),
        Err(None) => match opener.open() {
            Ok(m) => Waited::Seen(m.map(|m| m.mode)),
            Err(e) if no_device_yet(&e) => Waited::Seen(None),
            Err(e) => Waited::OpenError(e),
        },
    })
}

/// Send the write-only jump frame `ty` (JUMP_DFU or JUMP_APROM) with
/// transaction id `id` as one raw write, then read the meter's reply once
/// for [`JUMP_REPLY_WAIT`]. The meter may reset as it jumps, so a write
/// that sees it vanish ([`TransportError::Disconnected`]) counts as sent
/// (and nothing is read); any other failure is a device error naming the
/// jump.
fn send_jump<P: FlashTransport>(pipe: &mut P, ty: u8, id: u8) -> Result<JumpReply, FlashError> {
    match pipe.write_raw(&[ty, id, 0x00, 0x00], WRITE_WAIT) {
        Ok(()) => Ok(JumpReply::from_read(pipe.read(JUMP_REPLY_WAIT))),
        Err(TransportError::Disconnected) => Ok(JumpReply::Disconnected),
        Err(e) => Err(FlashError::Device {
            detail: format!("{}: {e}", jump_name(ty)),
        }),
    }
}

/// What the meter answered to a jump frame (the vendor tool reads nothing
/// back `[static]`, so any answer is a finding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpReply {
    /// ACCEPT (0x05).
    Accept,
    /// REJECT (0x06).
    Reject,
    /// A reply of another type (bits 0..6 of its first byte).
    Other(u8),
    /// Nothing within [`JUMP_REPLY_WAIT`].
    NoReply,
    /// The meter vanished from the bus (it reset as it jumped).
    Disconnected,
}

impl JumpReply {
    /// The reply as one read after the jump saw it. A read error other than
    /// a disconnect counts as no reply.
    fn from_read(r: Result<Vec<u8>, TransportError>) -> JumpReply {
        match r {
            Ok(reply) => match reply_type(&reply) {
                Some(ACCEPT) => JumpReply::Accept,
                Some(REJECT) => JumpReply::Reject,
                Some(ty) => JumpReply::Other(ty),
                None => JumpReply::NoReply,
            },
            Err(TransportError::Disconnected) => JumpReply::Disconnected,
            Err(_) => JumpReply::NoReply,
        }
    }
}

impl std::fmt::Display for JumpReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JumpReply::Accept => f.write_str("ACCEPT"),
            JumpReply::Reject => f.write_str("REJECT"),
            JumpReply::Other(ty) => write!(f, "type {ty:#04x}"),
            JumpReply::NoReply => f.write_str("none"),
            JumpReply::Disconnected => f.write_str("none, the meter disconnected"),
        }
    }
}

/// A jump [`jump`] has sent, reported before it waits for the meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpSent {
    /// The mode the meter was in.
    pub from: DeviceMode,
    /// The mode it was asked to switch to.
    pub target: JumpTarget,
    /// Its reply to the jump frame.
    pub reply: JumpReply,
}

/// The name of jump frame type `ty`.
fn jump_name(ty: u8) -> &'static str {
    if ty == JUMP_DFU {
        JumpTarget::Bootloader.frame_name()
    } else {
        JumpTarget::Application.frame_name()
    }
}

/// The mode a [`jump`] asks the meter to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpTarget {
    /// The bootloader, with JUMP_DFU (from application mode).
    Bootloader,
    /// The application firmware, with JUMP_APROM (from bootloader mode).
    Application,
}

impl JumpTarget {
    /// The device mode this target names.
    pub fn mode(self) -> DeviceMode {
        match self {
            JumpTarget::Bootloader => DeviceMode::Bootloader,
            JumpTarget::Application => DeviceMode::Application,
        }
    }

    /// The name of the jump frame that reaches this target: `JUMP_DFU` for
    /// the bootloader, `JUMP_APROM` for the application.
    pub fn frame_name(self) -> &'static str {
        match self {
            JumpTarget::Bootloader => "JUMP_DFU",
            JumpTarget::Application => "JUMP_APROM",
        }
    }
}

/// How a [`jump`] that was sent ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpOutcome {
    /// The meter came back in the target mode.
    Reached(DeviceMode),
    /// The meter did not come back in the target mode within the wait;
    /// `seen` is the mode it shows afterwards (`None`: no meter attached).
    NotReached {
        /// The mode seen after the wait.
        seen: Option<DeviceMode>,
    },
}

/// Switch the meter `opener` finds to `target` with one write-only jump
/// frame, then report the mode it comes back in.
///
/// From application mode only JUMP_DFU is sent, from bootloader mode only
/// JUMP_APROM, as a raw write through the strict [`FlashGuard`]; nothing
/// else (no CONNECT, HEAD, descriptor or info read). The meter's reply is
/// read once and handed to `sent` with the mode it was in, before the
/// wait, so the caller can ask for a replug. A meter already in the target
/// mode ([`FlashError::AlreadyInMode`]), in an unknown mode
/// ([`FlashError::UnknownMode`]) or a `stop` already requested sends
/// nothing. After the jump the meter is looked for on clock time, for
/// [`SWITCH_TIMEOUT`] (to the bootloader) or [`RETURN_TIMEOUT`] (to the
/// application).
pub fn jump<O: MeterOpener, C: Clock>(
    _ack: FlashAck,
    target: JumpTarget,
    opener: &mut O,
    mut clock: C,
    stop: &AtomicBool,
    sent: &mut dyn FnMut(&JumpSent),
) -> Result<JumpOutcome, FlashError> {
    let meter = opener.open().map_err(device)?.ok_or(FlashError::Device {
        detail: "no meter found".into(),
    })?;
    if stopped(stop) {
        return Err(interrupted("before the jump"));
    }
    match meter.mode {
        DeviceMode::Unknown => return Err(FlashError::UnknownMode),
        mode if mode == target.mode() => return Err(FlashError::AlreadyInMode { mode }),
        _ => {}
    }
    let from = meter.mode;
    let mut report = |reply| {
        sent(&JumpSent {
            from,
            target,
            reply,
        })
    };
    match jump_and_wait(meter.pipe, target, opener, &mut clock, &mut report)? {
        Waited::Reached(m) => Ok(JumpOutcome::Reached(m.mode)),
        Waited::Seen(seen) => Ok(JumpOutcome::NotReached { seen }),
        Waited::OpenError(e) => Err(FlashError::Device {
            detail: format!(
                "{} was sent, but the meter could not be opened afterwards: {e}",
                target.frame_name()
            ),
        }),
    }
}

/// Borrows a pipe or a clock for a session that would otherwise consume it.
struct ByRef<'a, T>(&'a mut T);

impl<T: Transport> Transport for ByRef<'_, T> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.0.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.0.read(timeout)
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        self.0.recover()
    }
}

impl<T: FlashTransport> FlashTransport for ByRef<'_, T> {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), TransportError> {
        self.0.write_raw(data, timeout)
    }
}

impl<C: Clock> Clock for ByRef<'_, C> {
    fn now_ms(&mut self) -> u64 {
        self.0.now_ms()
    }

    fn random_u64(&mut self) -> u64 {
        self.0.random_u64()
    }

    fn sleep(&mut self, duration: Duration) {
        self.0.sleep(duration)
    }
}

/// Read the application info block (CONNECT, then the 0x4420 read) over
/// the shared read-only guard inside the strict flash guard; `None` for a
/// blank block. Used before JUMP_DFU (model check) and after JUMP_APROM
/// (version check).
fn read_app_info<P: FlashTransport, C: Clock>(
    pipe: &mut P,
    clock: &mut C,
) -> Result<Option<DeviceInfoBlock>, String> {
    let guarded = GuardedTransport::new(FlashGuard::new(ByRef(pipe)));
    let mut session = Session::with_clock(guarded, ByRef(clock));
    session.connect().map_err(|e| e.to_string())?;
    let block = session
        .memory_read(APP_INFO.0, APP_INFO.1)
        .map_err(|e| e.to_string())?;
    DeviceInfoBlock::decode(&block).map_err(|e| e.to_string())
}

/// Rolling transaction ids of the flash frames.
struct Ids(u8);

impl Ids {
    fn next(&mut self) -> u8 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }
}

fn device(e: TransportError) -> FlashError {
    FlashError::Device {
        detail: e.to_string(),
    }
}

/// The model name of a bootloader-mode meter, from its USB product ID.
fn check_model(model: Model, container: &FirmwareContainer<'_>) -> Result<(), FlashError> {
    let meter = match model {
        Model::Km003c => "KM003C",
        Model::Km002c => "KM002C",
        Model::Unknown(pid) => {
            return Err(FlashError::Product {
                detail: format!("unknown USB product ID {pid:#06x}"),
            });
        }
    };
    if container.model().eq_ignore_ascii_case(meter) {
        Ok(())
    } else {
        Err(FlashError::Product {
            detail: format!(
                "the file is for {}, the meter is {meter}",
                container.model()
            ),
        })
    }
}

/// Whether the descriptor and payload of `container` fit the application
/// area: the size is not zero, 0x4000 + size is at most [`MAX_APP_END`],
/// and the stored payload is at most size + one block (the guard's budget).
fn check_container(container: &FirmwareContainer<'_>) -> Result<(), FlashError> {
    let size = container.size();
    if size == 0 {
        return Err(FlashError::Product {
            detail: "the file declares size 0".into(),
        });
    }
    match APP_START.checked_add(size) {
        Some(end) if end <= MAX_APP_END => {}
        _ => {
            return Err(FlashError::Product {
                detail: format!(
                    "the file's size {size} does not fit the application area \
                     ({APP_START:#x}..{MAX_APP_END:#x})"
                ),
            });
        }
    }
    let stored = container.payload().len();
    if stored > size as usize + BLOCK_LEN {
        return Err(FlashError::Product {
            detail: format!(
                "the stored payload ({stored} bytes) is longer than the declared size \
                 {size} + one block"
            ),
        });
    }
    Ok(())
}

/// Poll `opener` until the meter is in `mode`, for up to `limit` of clock
/// time (measured, so slow opens count toward the wait). If the wait ends
/// without it, the error is the last opener error that is not "no device
/// yet" (`None`: there was none).
fn wait_for<O: MeterOpener, C: Clock>(
    opener: &mut O,
    clock: &mut C,
    mode: DeviceMode,
    limit: Duration,
) -> Result<Meter<O::Pipe>, Option<TransportError>> {
    let start = clock.now_ms();
    let limit_ms = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
    let mut last_error = None;
    while clock.now_ms().saturating_sub(start) < limit_ms {
        clock.sleep(POLL_INTERVAL);
        // A meter that is re-enumerating may fail to open; look again.
        match opener.open() {
            Ok(Some(m)) if m.mode == mode => return Ok(m),
            // A later successful open supersedes an earlier opener error:
            // the final observation wins.
            Ok(_) => last_error = None,
            Err(e) if no_device_yet(&e) => {}
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error)
}

/// Whether an opener error only means the meter is not back yet (it is
/// re-enumerating).
fn no_device_yet(e: &TransportError) -> bool {
    matches!(e, TransportError::Disconnected)
}

/// Message type of a reply, if it has one.
fn reply_type(reply: &[u8]) -> Option<u8> {
    reply.first().map(|b| b & 0x7F)
}

fn describe(r: &Result<Vec<u8>, TransportError>) -> String {
    match r {
        Ok(reply) => match reply_type(reply) {
            Some(ty) => format!("unexpected reply type {ty:#04x}"),
            None => "empty reply".into(),
        },
        Err(e) => e.to_string(),
    }
}

/// A HEAD reply as `HEAD reply <spaced lowercase hex>`, for error messages.
fn head_hex(reply: &[u8]) -> String {
    format!("HEAD reply {}", head::hex(reply, " "))
}

/// Whether `r` is a reply of type `ty`.
fn is(r: &Result<Vec<u8>, TransportError>, ty: u8) -> bool {
    matches!(r, Ok(reply) if reply_type(reply) == Some(ty))
}

/// The 0x42 descriptor frame for `container` (§6.2 `[static]`).
fn descriptor(
    id: u8,
    container: &FirmwareContainer<'_>,
    random: u32,
) -> Result<Vec<u8>, FlashError> {
    let size = container.size();
    let end = APP_START.checked_add(size).ok_or(FlashError::Write {
        detail: format!("size {size} overflows the address space"),
    })?;
    let mut d = [0u8; DESCRIPTOR_LEN];
    d[0x00..0x04].copy_from_slice(&APP_START.to_le_bytes());
    d[0x04..0x08].copy_from_slice(&end.to_le_bytes());
    d[0x08..0x0C].copy_from_slice(&size.to_le_bytes());
    d[0x0C..0x10].copy_from_slice(&container.crc().to_le_bytes());
    for (off, s) in [
        (0x10, container.model()),
        (0x1C, container.firmware_version()),
        (0x28, container.date()),
    ] {
        let b = s.as_bytes();
        let n = b.len().min(FIELD_LEN);
        d[off..off + n].copy_from_slice(&b[..n]);
    }
    d[0x34..0x38].copy_from_slice(&random.to_le_bytes());
    let mut f = vec![DESCRIPTOR, id, 0x00, 0x04];
    f.extend_from_slice(&d);
    Ok(f)
}

/// The bootloader sequence, from CONNECT to FINISHED.
fn download<P: FlashTransport, C: Clock>(
    pipe: &mut P,
    container: &FirmwareContainer<'_>,
    clock: &mut C,
    ids: &mut Ids,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(FlashProgress),
) -> Result<(), FlashError> {
    // CONNECT → ACCEPT (else −108). The bootloader leaves the first CONNECT
    // after an open unanswered `[device]`; like the session's connect, an
    // unanswered one is retried once after a recover (no device command).
    let mut r = pipe.transact(&frame::connect(ids.next()), CONNECT_WAIT);
    if matches!(r, Err(TransportError::Timeout)) && pipe.recover().is_ok() {
        r = pipe.transact(&frame::connect(ids.next()), CONNECT_WAIT);
    }
    if !is(&r, ACCEPT) {
        return Err(FlashError::Read {
            detail: format!("CONNECT: {}", describe(&r)),
        });
    }

    // HEAD: type 0x40 (else −100), echoing the request id (else −108, a reply
    // we could not read as ours), field 0 (else −111). Fail closed: a reply
    // that cannot be tied to this request (any other id, 0 included) is never
    // trusted, whatever its field says.
    let head_id = ids.next();
    let r = pipe.transact(&head::request(head_id), HEAD_WAIT);
    let reply = match r {
        Ok(reply) if reply_type(&reply) == Some(HEAD) && reply.get(1) == Some(&head_id) => reply,
        Ok(reply) if reply_type(&reply) == Some(HEAD) => {
            return Err(FlashError::Read {
                detail: format!(
                    "HEAD: the reply id is not the request id {head_id:#04x}; {}",
                    head_hex(&reply)
                ),
            });
        }
        Ok(reply) => {
            return Err(FlashError::Product {
                detail: format!(
                    "HEAD: {}; {}",
                    describe(&Ok(reply.clone())),
                    head_hex(&reply)
                ),
            });
        }
        Err(e) => {
            return Err(FlashError::Read {
                detail: format!("HEAD: {e}"),
            });
        }
    };
    let field = HeadReply::decode(&reply)
        .map_err(|e| FlashError::Product {
            detail: format!("HEAD: {e}; {}", head_hex(&reply)),
        })?
        .old_firmware_field();
    if field != 0 {
        return Err(FlashError::WorkMode {
            field,
            reply: head_hex(&reply),
        });
    }

    if stopped(stop) {
        return Err(interrupted("before the descriptor"));
    }
    progress(FlashProgress::Erasing);
    // The descriptor: a raw write, then a short read that must time out
    // (else −107).
    let desc = descriptor(ids.next(), container, clock.random_u64() as u32)?;
    pipe.write_raw(&desc, WRITE_WAIT)
        .map_err(|e| FlashError::Write {
            detail: format!("descriptor: {e}"),
        })?;
    match pipe.read(DESCRIPTOR_WAIT) {
        Err(TransportError::Timeout) => {}
        r => {
            return Err(FlashError::Write {
                detail: format!("descriptor: {}", describe(&r)),
            });
        }
    }

    // Erase wait: N−1 short reads that time out, then ACCEPT (else −105).
    // [inferred] N counts whole blocks of the payload as streamed.
    let payload = container.payload();
    let blocks = payload.len() / BLOCK_LEN;
    for _ in 1..blocks {
        match pipe.read(ERASE_SHORT_WAIT) {
            Err(TransportError::Timeout) => {}
            r => {
                return Err(FlashError::Erase {
                    detail: format!("during the erase wait: {}", describe(&r)),
                });
            }
        }
    }
    let r = pipe.read(ERASE_FINAL_WAIT);
    if !is(&r, ACCEPT) {
        return Err(FlashError::Erase {
            detail: format!("no ACCEPT after the erase: {}", describe(&r)),
        });
    }

    // Data phase: raw writes of the stored bytes, each acked by SYNC.
    let total = payload.len();
    let mut done = 0;
    progress(FlashProgress::Writing { done, total });
    for (k, chunk) in payload.chunks(BLOCK_LEN).enumerate() {
        let write = k + 1;
        pipe.write_raw(chunk, WRITE_WAIT)
            .map_err(|e| FlashError::Download {
                write,
                detail: e.to_string(),
            })?;
        let r = pipe.read(SYNC_WAIT);
        if !is(&r, SYNC) {
            return Err(FlashError::Download {
                write,
                detail: format!("no SYNC: {}", describe(&r)),
            });
        }
        done += chunk.len();
        progress(FlashProgress::Writing { done, total });
        if stopped(stop) {
            return Err(interrupted(&format!("after write {write}")));
        }
    }

    // Verify: FINISHED (else −103).
    progress(FlashProgress::Verifying);
    let r = pipe.read(FINISHED_WAIT);
    if !is(&r, FINISHED) {
        return Err(FlashError::Verify {
            detail: format!("no FINISHED: {}", describe(&r)),
        });
    }
    progress(FlashProgress::Finished);
    Ok(())
}
