//! Changing meter settings in the browser (FR W8SHX9): one type-0x0A write
//! between two SETTINGS reads and a read-back, as the native
//! `Session::write_settings_checked` sends it over the meter's serial port.
//! Sans-IO: the page moves the frames, this state machine builds and checks
//! them. Errors carry the desktop app's wording.

use std::collections::BTreeMap;

use powerz_protocol::decode::settings::{
    Settings, SettingsWriteError, WritableField, settings_write_request,
};
use powerz_protocol::frame::{self, Attribute, Reply};
use powerz_protocol::session::{self, settings_written};
use powerz_protocol::transport::{cdc_frame, guard};
use wasm_bindgen::prelude::*;

use crate::BindError;
use crate::decoders::to_js;
use crate::meter_reads::{SettingsView, section_payload, settings_request};

/// Length of the first complete frame at the start of `buf` under the CDC
/// framing; `undefined` while more bytes are needed.
#[wasm_bindgen(js_name = cdcFrameLength)]
pub fn cdc_frame_length(buf: &[u8]) -> Option<u32> {
    cdc_frame::frame_complete(buf).and_then(|n| u32::try_from(n).ok())
}

fn usage(message: impl Into<String>) -> BindError {
    BindError::new("usage", message)
}

fn out_of_order(what: &str) -> BindError {
    BindError::invalid(format!("{what} is out of order in a settings write"))
}

/// A protocol-crate error of a settings write in the desktop app's words.
fn session_error(e: &session::Error) -> BindError {
    use session::Error as S;
    match e {
        S::SettingsReadsDiffer => BindError::with_hint(
            "protocol",
            "Two reads of the meter's settings differ, so nothing was written.",
            "Check the cable, then apply again.",
        ),
        S::SettingsMismatch {
            field,
            expected,
            actual,
        } => BindError::with_hint(
            "protocol",
            format!(
                "The meter's settings do not match after the change: {field} reads {actual}, expected {expected}."
            ),
            "Read the settings again; nothing was retried.",
        ),
        S::SettingsWrite(e) => write_error(e),
        S::Decode(e) => BindError::new("malformed", e.to_string()),
        other => BindError::new("protocol", other.to_string()),
    }
}

fn write_error(e: &SettingsWriteError) -> BindError {
    match e {
        SettingsWriteError::OutOfRange { .. } => {
            usage(format!("That setting value is out of range: {e}."))
        }
        SettingsWriteError::MeterValueOutOfRange { field, value, .. } => BindError::with_hint(
            "protocol",
            format!("The meter holds {field} = {value}, which this app cannot write back."),
            format!("Change {field} too, then apply again."),
        ),
    }
}

/// Where a write stands.
#[derive(Debug)]
enum Step {
    /// Waiting for the first SETTINGS read.
    FirstRead,
    /// Waiting for the second SETTINGS read.
    SecondRead(Settings),
    /// Two matching reads: the write may be built.
    Ready(Settings),
    /// The write was built; waiting for its reply (or its absence).
    Sent(Settings),
    /// The write went out; waiting for the read-back.
    ReadBack(Settings),
    /// The read-back arrived; `result` checks it.
    Done(Settings, Settings),
    /// Refused or failed: nothing more is built.
    Over,
}

/// One settings write: `readRequest`/`takeRead` twice, `writeRequest`,
/// `takeWriteReply`, `readRequest`/`takeRead` once more, then `result`.
/// Calls out of order are `invalid`.
#[wasm_bindgen]
#[derive(Debug)]
pub struct SettingsWrite {
    changes: Vec<(WritableField, i32)>,
    step: Step,
}

impl SettingsWrite {
    /// A write of `changes` (`{key: code}`), checked as the desktop app
    /// checks them: none, an unknown or read-only key, or an out-of-range
    /// value is a `usage` error.
    pub fn from_changes(changes: &BTreeMap<String, i32>) -> Result<SettingsWrite, BindError> {
        if changes.is_empty() {
            return Err(usage("No setting was changed."));
        }
        let mut parsed = Vec::with_capacity(changes.len());
        for (key, &value) in changes {
            let field = WritableField::from_name(key)
                .ok_or_else(|| usage(format!("The setting {key} cannot be changed.")))?;
            let range = field.range();
            if !range.contains(&value) {
                return Err(usage(format!(
                    "{key} = {value} is out of range ({}..={}).",
                    range.start(),
                    range.end()
                )));
            }
            parsed.push((field, value));
        }
        Ok(SettingsWrite {
            changes: parsed,
            step: Step::FirstRead,
        })
    }

    /// The settings as read again, with the warnings of the write; a
    /// change that did not take names the field.
    pub fn result(&self) -> Result<SettingsView, BindError> {
        let Step::Done(before, after) = &self.step else {
            return Err(out_of_order("the result"));
        };
        let written =
            settings_written(before, after, &self.changes).map_err(|e| session_error(&e))?;
        Ok(SettingsView {
            fields: written.settings.rows(),
            warnings: written.warnings(),
        })
    }
}

fn decode_block(reply: &[u8]) -> Result<Settings, BindError> {
    let block = Settings::decode(section_payload(reply, Attribute::SETTINGS, "SETTINGS")?)?;
    // As the native write: a block whose fields do not decode stops it.
    block.meter_settings()?;
    Ok(block)
}

#[wasm_bindgen]
impl SettingsWrite {
    /// [`SettingsWrite::from_changes`] for JS: `changes` is `{key: code}`.
    #[wasm_bindgen(constructor)]
    pub fn new(changes: JsValue) -> Result<SettingsWrite, BindError> {
        let changes: BTreeMap<String, i32> = serde_wasm_bindgen::from_value(changes)
            .map_err(|e| BindError::invalid(format!("not a settings change: {e}")))?;
        SettingsWrite::from_changes(&changes)
    }

    /// GET_DATA SETTINGS with request id `id`.
    #[wasm_bindgen(js_name = readRequest)]
    pub fn read_request(&self, id: u8) -> Vec<u8> {
        settings_request(id)
    }

    /// Take a SETTINGS reply: the first, the second (which must match the
    /// first in all 180 bytes, else the write is refused) or the read-back.
    #[wasm_bindgen(js_name = takeRead)]
    pub fn take_read(&mut self, reply: &[u8]) -> Result<(), BindError> {
        let block = decode_block(reply);
        let step = std::mem::replace(&mut self.step, Step::Over);
        self.step = match (step, block) {
            (Step::FirstRead, Ok(block)) => Step::SecondRead(block),
            (Step::SecondRead(first), Ok(block)) => {
                if block.raw() != first.raw() {
                    return Err(session_error(&session::Error::SettingsReadsDiffer));
                }
                Step::Ready(first)
            }
            (Step::ReadBack(before), Ok(after)) => Step::Done(before, after),
            (step @ (Step::FirstRead | Step::SecondRead(_) | Step::ReadBack(_)), Err(e)) => {
                self.step = step;
                return Err(e);
            }
            (step, _) => {
                self.step = step;
                return Err(out_of_order("a SETTINGS read"));
            }
        };
        Ok(())
    }

    /// The type-0x0A settings write with request id `id`, built from the
    /// values read and the changes; only after two matching reads.
    #[wasm_bindgen(js_name = writeRequest)]
    pub fn write_request(&mut self, id: u8) -> Result<Vec<u8>, BindError> {
        let Step::Ready(before) = &self.step else {
            return Err(out_of_order("the write"));
        };
        let current = before.meter_settings()?;
        let request =
            settings_write_request(id, &current, &self.changes).map_err(|e| write_error(&e))?;
        guard::check_settings_write(&request)
            .map_err(|e| BindError::new("forbidden", e.to_string()))?;
        if let Step::Ready(before) = std::mem::replace(&mut self.step, Step::Over) {
            self.step = Step::Sent(before);
        }
        Ok(request)
    }

    /// Take the write's reply: none (no reply in time), ACCEPT or FINISHED
    /// lets the read-back decide; a REJECT or any other reply fails it.
    #[wasm_bindgen(js_name = takeWriteReply)]
    pub fn take_write_reply(&mut self, reply: Option<Vec<u8>>) -> Result<(), BindError> {
        let before = match std::mem::replace(&mut self.step, Step::Over) {
            Step::Sent(before) => before,
            step => {
                self.step = step;
                return Err(out_of_order("the write reply"));
            }
        };
        if let Some(reply) = reply {
            match frame::classify_reply(&reply)? {
                Reply::Accept(_) | Reply::Finished(_) => {}
                Reply::Reject(_) => {
                    return Err(BindError::with_hint(
                        "protocol",
                        "The meter refused the request.",
                        "Updating the meter's firmware may help.",
                    ));
                }
                // Any other reply: plain words, as the desktop app's protocol error.
                _ => {
                    return Err(BindError::new(
                        "protocol",
                        "The meter sent an unexpected reply to the settings change.",
                    ));
                }
            }
        }
        self.step = Step::ReadBack(before);
        Ok(())
    }

    /// [`SettingsWrite::result`] for JS: `{ fields, warnings }`.
    #[wasm_bindgen(js_name = result)]
    pub fn result_js(&self) -> Result<JsValue, BindError> {
        to_js(&self.result()?)
    }
}
