//! Changing meter settings: one type-0x0A write between two SETTINGS reads.
//! See `docs/protocol.md` "Settings write".

use super::{Clock, Error, Session, unexpected_reply};
use crate::decode::settings::{MeterSettings, WritableField, settings_write_request};
use crate::frame::{self, Reply};
use crate::transport::{self, Transport};

/// What a settings write left on the meter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsWritten {
    /// The settings as read again.
    pub settings: MeterSettings,
    /// Offsets of the bytes no field decodes that the write moved.
    pub undecoded_changed: Vec<usize>,
}

impl SettingsWritten {
    /// The difference in the undecoded bytes in plain words, if any. It does
    /// not fail the write: nothing says those bytes should have stayed.
    pub fn warnings(&self) -> Vec<String> {
        let Some(first) = self.undecoded_changed.first() else {
            return Vec::new();
        };
        let n = self.undecoded_changed.len();
        let bytes = if n == 1 { "byte" } else { "bytes" };
        vec![format!(
            "{n} {bytes} of the meter's settings that the app does not decode \
             changed as well (first at offset {first:#04x})."
        )]
    }
}

/// Fields that are never written; a write must leave them as read.
const KEPT: [&str; 5] = [
    "startup_logo",
    "auto_rotate",
    "voltage_cal",
    "current_cal",
    "offset_cal",
];

impl<T: Transport, C: Clock> Session<T, C> {
    /// Read SETTINGS, send one settings write carrying `changes` over the
    /// values just read, then read SETTINGS again. \[static\]
    ///
    /// An out-of-range value is [`Error::SettingsWrite`], sent after the
    /// first read only. An ACCEPT or a FINISHED counts as sent; so does no
    /// reply at all, since the vendor app never reads one \[static\]: then
    /// the read-back alone decides. A REJECT or any other reply is an error,
    /// and nothing is retried. A
    /// re-read where a changed field lacks its new value, or any other field
    /// moved, is [`Error::SettingsMismatch`] naming that field. Returns the
    /// settings as read again.
    pub fn write_settings(
        &mut self,
        changes: &[(WritableField, i32)],
    ) -> Result<MeterSettings, Error> {
        Ok(self.write_settings_checked(changes)?.settings)
    }

    /// [`Session::write_settings`], also reporting the bytes no field decodes
    /// that differ between the two reads. The same frames go out.
    pub fn write_settings_checked(
        &mut self,
        changes: &[(WritableField, i32)],
    ) -> Result<SettingsWritten, Error> {
        let block_before = self.read_settings()?;
        let before = block_before.meter_settings()?;
        let request = settings_write_request(self.counter.next(), &before, changes)?;
        let timeout = transport::default_timeout(&request);
        let reply = self
            .transport
            .transact_settings(&request, timeout)
            .map_err(Error::from)
            .and_then(|first| self.reread_until_id(first, request[1], timeout, |_| false));
        match reply {
            Ok(reply) => match frame::classify_reply(&reply)? {
                Reply::Accept(_) | Reply::Finished(_) => {}
                other => return Err(unexpected_reply(&request, "settings write", other)),
            },
            // No reply: the read-back below decides.
            Err(Error::Transport(transport::Error::Timeout)) => {}
            Err(e) => return Err(e),
        }
        let block_after = self.read_settings()?;
        let after = block_after.meter_settings()?;
        verify(&before, &after, changes)?;
        Ok(SettingsWritten {
            settings: after,
            undecoded_changed: block_before.undecoded_diff(&block_after),
        })
    }
}

/// Every writable field at its new (or read) code, every other field as read.
fn verify(
    before: &MeterSettings,
    after: &MeterSettings,
    changes: &[(WritableField, i32)],
) -> Result<(), Error> {
    for f in WritableField::ALL {
        let expected = f.code_after(before, changes);
        let actual = f.code(after);
        if actual != expected {
            return Err(Error::SettingsMismatch {
                field: f.name(),
                expected: expected.to_string(),
                actual: actual.to_string(),
            });
        }
    }
    let (rows_before, rows_after) = (before.rows(), after.rows());
    for key in KEPT {
        let value = |rows: &[crate::decode::settings::Row]| {
            rows.iter()
                .find(|r| r.key == key)
                .map(|r| r.value.clone())
                .unwrap_or_default()
        };
        let (expected, actual) = (value(&rows_before), value(&rows_after));
        if actual != expected {
            return Err(Error::SettingsMismatch {
                field: key,
                expected,
                actual,
            });
        }
    }
    Ok(())
}
