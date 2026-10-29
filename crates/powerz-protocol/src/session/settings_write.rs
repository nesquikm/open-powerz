//! Changing meter settings: one type-0x0A write between two SETTINGS reads.
//! See `docs/protocol.md` "Settings write".

use super::{Clock, Error, Session, unexpected_reply};
use crate::decode::settings::{MeterSettings, WritableField, settings_write_request};
use crate::frame::{self, Reply};
use crate::transport::{self, Transport};

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
    /// first read only. A REJECT is an error and nothing is retried. A
    /// re-read where a changed field lacks its new value, or any other field
    /// moved, is [`Error::SettingsMismatch`] naming that field. Returns the
    /// settings as read again.
    pub fn write_settings(
        &mut self,
        changes: &[(WritableField, i32)],
    ) -> Result<MeterSettings, Error> {
        let before = self.read_meter_settings()?;
        let request = settings_write_request(self.counter.next(), &before, changes)?;
        let timeout = transport::default_timeout(&request);
        let first = self.transport.transact_settings(&request, timeout)?;
        let reply = self.reread_until_id(first, request[1], timeout)?;
        match frame::classify_reply(&reply)? {
            Reply::Accept(_) => {}
            other => return Err(unexpected_reply(&request, "settings write", other)),
        }
        let after = self.read_meter_settings()?;
        verify(&before, &after, changes)?;
        Ok(after)
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
