//! The desktop firmware update (FR V9SK55): re-checks the typed
//! confirmation against the file, then runs the library's flash engine and
//! reports each stage for the progress view.
//!
//! The flash acknowledgement is minted here alone, and only once the file
//! has passed inspection and the confirmation equals its version.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use powerz_protocol::firmware::container::{FirmwareContainer, read_container_file};
use powerz_protocol::firmware::flash::{
    FlashAck, FlashError, FlashGuard, FlashOutcome, FlashProgress, Meter, MeterOpener, flash,
};
use powerz_protocol::session::Clock;
use powerz_protocol::transport;
use powerz_protocol::transport::bulk::{BulkFlashOpener, BulkTransport};
use powerz_sim::SimMeter;
use serde::Serialize;

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
    let bytes = read_container_file(path).map_err(|e| UiError::usage(e.to_string()))?;
    let container = FirmwareContainer::parse(&bytes)
        .map_err(|e| UiError::usage(format!("{}: {e}", path.display())))?;
    let version = container.firmware_version().trim();
    if confirmation.trim() != version {
        return Err(UiError::usage(format!(
            "Type the file's version, {version}, to confirm the update."
        )));
    }
    opener.arm(FlashAck::acknowledge());
    let outcome = flash(
        FlashAck::acknowledge(),
        &container,
        opener,
        clock,
        stop,
        &mut |p| progress(p.into()),
    )
    .map_err(|e| flash_error(&e))?;
    Ok(match outcome {
        FlashOutcome::Verified { version } => FlashDone::Verified { version },
        FlashOutcome::PowerCycleNeeded => FlashDone::PowerCycleNeeded,
    })
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
    }
}
