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
const KEPT: [&str; 4] = ["auto_rotate", "voltage_cal", "current_cal", "offset_cal"];

impl<T: Transport, C: Clock> Session<T, C> {
    /// Read SETTINGS twice, send one settings write carrying `changes` over
    /// the values read, then read SETTINGS again. \[static\]
    ///
    /// Two reads that differ in any of the 180 bytes are
    /// [`Error::SettingsReadsDiffer`]: nothing is written or read again. An
    /// out-of-range value is [`Error::SettingsWrite`], sent after the two
    /// reads only. An ACCEPT or a FINISHED counts as sent; so does no
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
        // A write sends every display key at its read value, and the block
        // check can pass a corrupted read: a second read must agree on all
        // 180 bytes. A difference refuses the write; it is never retried.
        if self.read_settings()?.raw() != block_before.raw() {
            return Err(Error::SettingsReadsDiffer);
        }
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::crypto;
    use crate::frame::Attribute;
    use crate::transport::GuardedTransport;

    /// The captured FW 2.0.6 SETTINGS reply: 8 header bytes, then 180 bytes.
    const CAPTURED: &str = include_str!("../../tests/vectors/settings_188.hex");
    const HEADER: usize = 8;

    fn captured_frame() -> Vec<u8> {
        let hex: String = CAPTURED
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .flat_map(|l| l.chars().filter(|c| c.is_ascii_hexdigit()))
            .collect();
        hex.as_bytes()
            .chunks(2)
            .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
            .collect()
    }

    fn captured() -> Vec<u8> {
        captured_frame()[HEADER..].to_vec()
    }

    /// `edit` applied to the captured block, then both CRCs made right: the
    /// corruption passes the block checks.
    fn passing(edit: impl Fn(&mut [u8])) -> Vec<u8> {
        let mut p = captured();
        edit(&mut p);
        let a = crypto::crc32(&p[0x00..0x5C]);
        p[0x5C..0x60].copy_from_slice(&a.to_le_bytes());
        let b = crypto::crc32(&p[0x60..0xB0]);
        p[0xB0..0xB4].copy_from_slice(&b.to_le_bytes());
        p
    }

    /// Brightness (7 bits at bit 3 of the word at 0x00) set to `code`.
    fn brightness(p: &mut [u8], code: u32) {
        let word = u32::from_le_bytes(p[..4].try_into().unwrap());
        let word = (word & !(0x7F << 3)) | (code << 3);
        p[..4].copy_from_slice(&word.to_le_bytes());
    }

    /// A meter whose SETTINGS reads answer `blocks` in turn, for ever, and
    /// that takes a settings write. `None` is a read that gets no reply.
    struct Reads {
        blocks: Vec<Option<Vec<u8>>>,
        reads: usize,
        sent: Vec<Vec<u8>>,
    }

    impl Transport for Reads {
        fn transact(
            &mut self,
            request: &[u8],
            _timeout: Duration,
        ) -> Result<Vec<u8>, crate::transport::Error> {
            self.sent.push(request.to_vec());
            assert!(self.sent.len() <= 8, "sent without end: {:02x?}", self.sent);
            if request[0] & 0x7F == 0x0A {
                return Ok(vec![0x07, request[1], 0, 0]);
            }
            let block = self.blocks[self.reads % self.blocks.len()].clone();
            self.reads += 1;
            let payload = block.ok_or(crate::transport::Error::Timeout)?;
            let mut reply = captured_frame()[..HEADER].to_vec();
            reply[1] = request[1];
            reply.extend(payload);
            Ok(reply)
        }

        fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, crate::transport::Error> {
            Err(crate::transport::Error::Timeout)
        }
    }

    fn write_over(blocks: Vec<Option<Vec<u8>>>) -> (Result<SettingsWritten, Error>, Vec<Vec<u8>>) {
        let meter = Reads {
            blocks,
            reads: 0,
            sent: Vec::new(),
        };
        let mut session = Session::open(GuardedTransport::new(meter));
        let result = session.write_settings_checked(&[(WritableField::Brightness, 80)]);
        let sent = session.transport().inner().sent.clone();
        (result, sent)
    }

    fn settings_read(id: u8) -> Vec<u8> {
        crate::frame::get_data(id, Attribute::SETTINGS).to_vec()
    }

    #[test]
    fn settings_write_refuses_when_two_reads_differ() {
        let reference = passing(|_| {});
        let cases = [
            ("a display field", passing(|p| brightness(p, 90))),
            ("a byte no field decodes", passing(|p| p[0x08] ^= 0x01)),
            ("a byte of block B", passing(|p| p[0x70] ^= 0x01)),
        ];
        for (what, other) in cases {
            for blocks in [
                vec![Some(reference.clone()), Some(other.clone())],
                vec![Some(other.clone()), Some(reference.clone())],
            ] {
                let (result, sent) = write_over(blocks);
                let err = match result {
                    Ok(w) => panic!("{what}: the write went through: {w:?}"),
                    Err(e) => e,
                };
                let text = err.to_string().to_lowercase();
                assert!(
                    text.contains("settings") && text.contains("differ"),
                    "{what}: not a clear refusal: {text}"
                );
                assert_eq!(
                    sent,
                    vec![settings_read(1), settings_read(2)],
                    "{what}: exactly two SETTINGS reads and no write, nothing retried"
                );
            }
        }
    }

    #[test]
    fn a_second_read_that_gets_no_reply_sends_no_write() {
        let (result, sent) = write_over(vec![Some(captured()), None]);
        assert_eq!(
            result.as_ref().map(|_| ()).map_err(ToString::to_string),
            Err(Error::Transport(crate::transport::Error::Timeout).to_string())
        );
        assert_eq!(sent, vec![settings_read(1), settings_read(2)]);
    }

    #[test]
    fn two_matching_reads_are_followed_by_the_write_and_the_read_back() {
        let before = captured();
        let after = passing(|p| brightness(p, 80));
        let (result, sent) = write_over(vec![Some(before.clone()), Some(before), Some(after)]);
        let written = result.expect("the two reads match");
        assert_eq!(written.settings.brightness.value, 80);
        let types: Vec<u8> = sent.iter().map(|f| f[0] & 0x7F).collect();
        assert_eq!(types, vec![0x0C, 0x0C, 0x0A, 0x0C], "{sent:02x?}");
    }
}
