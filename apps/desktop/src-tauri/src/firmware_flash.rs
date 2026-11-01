//! The desktop firmware update (FR V9SK55): re-checks the typed
//! confirmation against the file, then runs the library's flash engine and
//! reports each stage for the progress view.
//!
//! The flash acknowledgement is minted here alone, and only once the file
//! has passed inspection and the confirmation equals its version.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use powerz_protocol::firmware::container::{FirmwareContainer, read_container_file};
use powerz_protocol::firmware::flash::{
    FlashAck, FlashError, FlashGuard, FlashOutcome, FlashProgress, Meter, MeterOpener, flash,
};
use powerz_protocol::session::Clock;
use powerz_protocol::transport;
use powerz_protocol::transport::bulk::{BulkFlashOpener, BulkTransport};
use powerz_sim::SimMeter;
use serde::Serialize;

use crate::app_log::SessionLog;
use crate::ui_error::{ErrorKind, UiError};

/// One stage of the update, as the UI receives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum FlashStage {
    /// Switching the meter into its bootloader.
    Switching,
    /// Erasing the application area.
    Erasing,
    /// Writing the image: bytes written so far of `total`.
    Writing {
        /// Bytes acknowledged so far.
        done: u64,
        /// Image length.
        total: u64,
    },
    /// Waiting for the bootloader's verdict.
    Verifying,
    /// The bootloader accepted the image.
    Finished,
    /// Returning the meter to application mode.
    Returning,
    /// A jump was sent: unplug the meter and plug it back in if it does not
    /// restart by itself. Not a stage of its own; the UI keeps showing the
    /// stage it came in.
    Replug {
        /// The mode the meter should come back in: `bootloader` or
        /// `application`.
        to: &'static str,
        /// The meter's reply to the jump frame (`none`, `ACCEPT`, ...).
        reply: String,
    },
}

impl From<FlashProgress> for FlashStage {
    fn from(p: FlashProgress) -> FlashStage {
        match p {
            FlashProgress::Switching => FlashStage::Switching,
            FlashProgress::Erasing => FlashStage::Erasing,
            FlashProgress::Writing { done, total } => FlashStage::Writing {
                done: done as u64,
                total: total as u64,
            },
            FlashProgress::Verifying => FlashStage::Verifying,
            FlashProgress::Finished => FlashStage::Finished,
            FlashProgress::Returning => FlashStage::Returning,
            FlashProgress::Replug { to, reply } => FlashStage::Replug {
                to: to.name(),
                reply: reply.to_string(),
            },
        }
    }
}

impl FlashStage {
    /// The stage's name in the log; a replug prompt is not a stage.
    fn log_name(&self) -> Option<&'static str> {
        match self {
            FlashStage::Switching => Some("switching"),
            FlashStage::Erasing => Some("erasing"),
            FlashStage::Writing { .. } => Some("writing"),
            FlashStage::Verifying => Some("verifying"),
            FlashStage::Finished => Some("finished"),
            FlashStage::Returning => Some("returning"),
            FlashStage::Replug { .. } => None,
        }
    }
}

/// Why Cancel is refused once the erase has begun (AC-4PJRVJ.1).
pub const ERASE_BEGAN: &str = "The erase has started, so the update can no longer be cancelled.";

/// The stop request of one running update, and the stage it has reached.
/// Cancel is accepted only until the erase starts (AC-4PJRVJ.1).
#[derive(Debug, Default)]
pub struct FlashControl {
    stop: AtomicBool,
    gate: Mutex<FlashGate>,
}

/// One lock around these, so a cancel and the erase's stage cannot cross.
#[derive(Debug, Default)]
struct FlashGate {
    stage: Option<FlashStage>,
    erase_began: bool,
}

impl FlashControl {
    /// The flag the engine polls.
    pub fn stop(&self) -> &AtomicBool {
        &self.stop
    }

    /// Note a stage the engine reported. A replug prompt is not a stage of
    /// its own and changes nothing.
    pub fn observe(&self, stage: &FlashStage) {
        let mut gate = self.gate();
        match stage {
            FlashStage::Replug { .. } => return,
            FlashStage::Switching => {}
            _ => gate.erase_began = true,
        }
        gate.stage = Some(stage.clone());
    }

    /// Ask the update to stop; refused once the erase has begun.
    pub fn cancel(&self) -> Result<(), UiError> {
        let gate = self.gate();
        if gate.erase_began {
            return Err(UiError::usage(ERASE_BEGAN));
        }
        self.stop.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// The latest stage, `None` before the first.
    pub fn stage(&self) -> Option<FlashStage> {
        self.gate().stage.clone()
    }

    fn gate(&self) -> std::sync::MutexGuard<'_, FlashGate> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// How a successful update ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum FlashDone {
    /// The meter is back in application mode and reports this version.
    Verified {
        /// The version read back from the meter.
        version: String,
    },
    /// The image was accepted but the meter stayed in its bootloader.
    PowerCycleNeeded,
}

/// The recovery steps shown with every failed update (AC-V9SK55.5).
pub const RECOVERY_HINT: &str = "Hold the Back button while plugging the meter in to start it in \
bootloader mode, then run the update again.";

/// A flash opener that is handed the acknowledgement before its first open,
/// so a real meter's write-capable pipe can only be opened by an update
/// that passed its checks.
pub trait FlashOpener: MeterOpener {
    /// Take the acknowledgement the opened pipes need.
    fn arm(&mut self, ack: FlashAck);
}

impl FlashOpener for SimMeter {
    fn arm(&mut self, _ack: FlashAck) {}
}

/// The real meter for an update: a [`BulkFlashOpener`] for the meter with
/// `serial`, else the only POWER-Z attached at the first open (followed by
/// its serial afterwards), created only once the update is armed; until
/// then every open is refused.
#[derive(Debug)]
pub struct UsbFlashOpener {
    serial: Option<String>,
    armed: Option<BulkFlashOpener>,
}

impl UsbFlashOpener {
    /// An opener for the meter with `serial` (if the OS reported one).
    pub fn new(serial: Option<String>) -> UsbFlashOpener {
        UsbFlashOpener {
            serial,
            armed: None,
        }
    }
}

impl FlashOpener for UsbFlashOpener {
    fn arm(&mut self, ack: FlashAck) {
        self.armed = Some(BulkFlashOpener::new(self.serial.clone(), ack));
    }
}

impl MeterOpener for UsbFlashOpener {
    type Pipe = FlashGuard<BulkTransport>;

    fn open(&mut self) -> Result<Option<Meter<Self::Pipe>>, transport::Error> {
        match &mut self.armed {
            Some(opener) => opener.open(),
            None => Err(transport::Error::Io(
                "the update was not confirmed".to_owned(),
            )),
        }
    }
}

/// The container in `bytes`, read from `path`, once it passes inspection and
/// `confirmation` equals its version; either refusal is a usage error.
fn confirmed_container<'a>(
    path: &Path,
    bytes: &'a [u8],
    confirmation: &str,
) -> Result<FirmwareContainer<'a>, UiError> {
    let container = FirmwareContainer::parse(bytes)
        .map_err(|e| UiError::usage(format!("{}: {e}", path.display())))?;
    let version = container.firmware_version().trim();
    if confirmation.trim() != version {
        return Err(UiError::usage(format!(
            "Type the file's version, {version}, to confirm the update."
        )));
    }
    Ok(container)
}

/// The refusals of [`flash_file`] that need no meter, for a start to make
/// before it lets go of any session (AC-AXHK2F.2).
pub fn check_confirmation(path: &Path, confirmation: &str) -> Result<(), UiError> {
    let bytes = read_container_file(path).map_err(|e| UiError::usage(e.to_string()))?;
    confirmed_container(path, &bytes, confirmation).map(|_| ())
}

/// Update the meter `opener` finds with the firmware container at `path`.
///
/// Nothing is sent unless the file passes inspection and `confirmation`
/// (what the user typed) equals the file's version; both refusals are usage
/// errors. Each stage goes to `progress`. A failure carries the engine's
/// message and the recovery steps.
pub fn flash_file<O: FlashOpener, C: Clock>(
    path: &Path,
    confirmation: &str,
    opener: &mut O,
    clock: C,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(FlashStage),
) -> Result<FlashDone, UiError> {
    let log = SessionLog::default();
    flash_file_logged(path, confirmation, opener, clock, stop, &log, progress)
}

/// [`flash_file`] with the update in `log` (AC-AXHK2G.3): a line at the
/// start, one per stage entered, and one for how it ended.
pub fn flash_file_logged<O: FlashOpener, C: Clock>(
    path: &Path,
    confirmation: &str,
    opener: &mut O,
    clock: C,
    stop: &AtomicBool,
    log: &SessionLog,
    progress: &mut dyn FnMut(FlashStage),
) -> Result<FlashDone, UiError> {
    log.firmware("start");
    let refused = |error: UiError| {
        log.firmware_failed(&error);
        error
    };
    let bytes = read_container_file(path).map_err(|e| refused(UiError::usage(e.to_string())))?;
    let container = confirmed_container(path, &bytes, confirmation).map_err(refused)?;
    opener.arm(FlashAck::acknowledge());
    let mut entered = None;
    let outcome = flash(
        FlashAck::acknowledge(),
        &container,
        opener,
        clock,
        stop,
        &mut |p| {
            let stage = FlashStage::from(p);
            if let Some(name) = stage.log_name()
                && entered != Some(name)
            {
                entered = Some(name);
                log.firmware(&format!("stage {name}"));
            }
            progress(stage);
        },
    );
    match outcome {
        Ok(outcome) => {
            log.firmware("finished");
            Ok(match outcome {
                FlashOutcome::Verified { version } => FlashDone::Verified { version },
                FlashOutcome::PowerCycleNeeded => FlashDone::PowerCycleNeeded,
            })
        }
        Err(e @ FlashError::Interrupted { .. }) => {
            log.firmware("cancelled");
            Err(flash_error(&e))
        }
        Err(e) => Err(refused(flash_error(&e))),
    }
}

/// A failed update for the UI: what failed, and how to recover.
fn flash_error(e: &FlashError) -> UiError {
    let kind = match e {
        FlashError::Device { .. } => ErrorKind::Disconnected,
        _ => ErrorKind::Protocol,
    };
    UiError {
        kind,
        message: format!("The firmware update failed: {e}."),
        hint: Some(RECOVERY_HINT.to_owned()),
        sessions: Vec::new(),
    }
}
