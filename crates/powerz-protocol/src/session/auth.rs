//! Authentication (0x4C) at level 1 (UID) or 2 (OTP).

use std::time::Duration;

use super::memory::{OTP, UID};
use super::{Clock, Error, Session};
use crate::crypto;
use crate::frame::{self, Reply};
use crate::transport::Transport;

/// Minimum spacing between two auth requests.
const AUTH_SPACING_MS: u64 = 1000;

/// Bytes of the OTP block used as the level-2 credential.
const OTP_CREDENTIAL_LEN: usize = 16;

/// Plaintext of an auth request: `u64 now_ms ‖ credential ‖ u64 random`,
/// padded with 0xFF to 32 bytes.
pub fn auth_plaintext(now_ms: u64, credential: &[u8], random: u64) -> [u8; 32] {
    let mut p = [0xFF; 32];
    let n = credential.len();
    p[0..8].copy_from_slice(&now_ms.to_le_bytes());
    p[8..8 + n].copy_from_slice(credential);
    p[8 + n..16 + n].copy_from_slice(&random.to_le_bytes());
    p
}

/// Whole auth frame: `4C id 00 02` + AES(key 3) of `plaintext`.
pub fn auth_request(id: u8, plaintext: &[u8; 32]) -> Vec<u8> {
    let mut f = vec![0x4C, id, 0x00, 0x02];
    f.extend(crypto::encrypt(&crypto::KEYS[3], plaintext));
    f
}

/// Key-3 variant the meter encrypts its auth reply with: byte 1 is ASCII `X`.
fn auth_reply_key() -> [u8; 16] {
    let mut k = crypto::KEYS[3];
    k[1] = b'X';
    k
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Authenticate at `level` (1 = UID, 2 = OTP); returns the level the
    /// meter reports.
    ///
    /// Requests are spaced at least one second apart. Any failure of the
    /// exchange itself, a malformed reply, a wrong echo or a granted level
    /// of 0 is reported as [`Error::AuthFailed`].
    pub fn authenticate(&mut self, level: u8) -> Result<u8, Error> {
        let credential = match level {
            1 => self.read_block(UID)?,
            2 => {
                let mut otp = self.read_block(OTP)?;
                otp.truncate(OTP_CREDENTIAL_LEN);
                otp
            }
            _ => return Err(Error::AuthFailed),
        };
        self.wait_auth_spacing();
        let now_ms = self.clock.now_ms();
        let random = self.clock.random_u64();
        self.last_auth_ms = Some(now_ms);
        let plaintext = auth_plaintext(now_ms, &credential, random);
        let request = auth_request(self.counter.next(), &plaintext);
        let reply = self.exchange(&request).map_err(|_| Error::AuthFailed)?;
        let header = match frame::classify_reply(&reply) {
            Ok(Reply::AuthReply(h)) => h,
            _ => return Err(Error::AuthFailed),
        };
        if reply.len() != frame::HEADER_LEN + plaintext.len() {
            return Err(Error::AuthFailed);
        }
        // The meter echoes the timestamp and the random value.
        let echo = crypto::decrypt(&auth_reply_key(), &reply[frame::HEADER_LEN..]);
        let rnd = 8 + credential.len();
        if echo[0..8] != plaintext[0..8] || echo[rnd..rnd + 8] != plaintext[rnd..rnd + 8] {
            return Err(Error::AuthFailed);
        }
        let granted = header.data().permi;
        if granted == 0 {
            return Err(Error::AuthFailed);
        }
        self.auth_level = granted;
        Ok(granted)
    }

    /// Sleep until [`AUTH_SPACING_MS`] have passed since the last auth request.
    fn wait_auth_spacing(&mut self) {
        if let Some(last) = self.last_auth_ms {
            let now = self.clock.now_ms();
            let due = last.saturating_add(AUTH_SPACING_MS);
            if now < due {
                self.clock.sleep(Duration::from_millis(due - now));
            }
        }
    }
}
