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
/// padded with 0xFF to 32 bytes. Only the first 16 credential bytes fit and
/// are used (the UID is 12 bytes, the OTP credential 16).
pub fn auth_plaintext(now_ms: u64, credential: &[u8], random: u64) -> [u8; 32] {
    let mut p = [0xFF; 32];
    let credential = &credential[..credential.len().min(OTP_CREDENTIAL_LEN)];
    let n = credential.len();
    p[0..8].copy_from_slice(&now_ms.to_le_bytes());
    p[8..8 + n].copy_from_slice(credential);
    p[8 + n..16 + n].copy_from_slice(&random.to_le_bytes());
    p
}

/// The credential bytes of the block read for auth `level`: the whole UID
/// for level 1, the first 16 OTP bytes for level 2; `None` for any other
/// level.
pub fn auth_credential(level: u8, block: &[u8]) -> Option<&[u8]> {
    match level {
        1 => Some(block),
        2 => Some(&block[..block.len().min(OTP_CREDENTIAL_LEN)]),
        _ => None,
    }
}

/// How long to wait before the next auth request: the rest of
/// [`AUTH_SPACING_MS`] since `last`, never more (the wall clock can step back).
fn spacing_sleep(last_ms: u64, now_ms: u64) -> Duration {
    let due = last_ms.saturating_add(AUTH_SPACING_MS);
    Duration::from_millis(due.saturating_sub(now_ms).min(AUTH_SPACING_MS))
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

/// The level an auth `reply` grants for a request built from `plaintext`
/// over a credential of `credential_len` bytes. `None` when the reply is not
/// an auth reply of the right length, its echo of the timestamp or the
/// random value is wrong, or it grants level 0.
pub fn auth_reply_level(reply: &[u8], plaintext: &[u8; 32], credential_len: usize) -> Option<u8> {
    let header = match frame::classify_reply(reply) {
        Ok(Reply::AuthReply(h)) => h,
        _ => return None,
    };
    if reply.len() != frame::HEADER_LEN + plaintext.len() {
        return None;
    }
    // The meter echoes the timestamp and the random value.
    let echo = crypto::decrypt(&auth_reply_key(), &reply[frame::HEADER_LEN..]);
    // Same clamp as `auth_plaintext`, so the offsets always agree.
    let rnd = 8 + credential_len.min(OTP_CREDENTIAL_LEN);
    if echo[0..8] != plaintext[0..8] || echo[rnd..rnd + 8] != plaintext[rnd..rnd + 8] {
        return None;
    }
    let granted = header.data().permi;
    (granted != 0).then_some(granted)
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Authenticate at `level` (1 = UID, 2 = OTP); returns the level the
    /// meter reports.
    ///
    /// Requests are spaced at least one second apart. Any failure of the
    /// exchange itself, a malformed reply, a wrong echo or a granted level
    /// of 0 is reported as [`Error::AuthFailed`].
    pub fn authenticate(&mut self, level: u8) -> Result<u8, Error> {
        let block = match level {
            1 => self.read_block(UID)?,
            2 => self.read_block(OTP)?,
            _ => return Err(Error::AuthFailed),
        };
        let credential = auth_credential(level, &block).ok_or(Error::AuthFailed)?;
        self.wait_auth_spacing();
        let now_ms = self.clock.now_ms();
        let random = self.clock.random_u64();
        self.last_auth_ms = Some(now_ms);
        let plaintext = auth_plaintext(now_ms, credential, random);
        let request = auth_request(self.counter.next(), &plaintext);
        let reply = self.exchange(&request).map_err(|_| Error::AuthFailed)?;
        let granted =
            auth_reply_level(&reply, &plaintext, credential.len()).ok_or(Error::AuthFailed)?;
        self.auth_level = granted;
        Ok(granted)
    }

    /// Sleep until [`AUTH_SPACING_MS`] have passed since the last auth request.
    fn wait_auth_spacing(&mut self) {
        if let Some(last) = self.last_auth_ms {
            let wait = spacing_sleep(last, self.clock.now_ms());
            if !wait.is_zero() {
                self.clock.sleep(wait);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_spacing_sleep_is_clamped_when_the_clock_steps_back() {
        assert_eq!(spacing_sleep(10_000, 10_400), Duration::from_millis(600));
        assert_eq!(spacing_sleep(10_000, 11_000), Duration::ZERO);
        // Wall clock stepped back by an hour: never sleep longer than 1 s.
        assert_eq!(
            spacing_sleep(10_000_000, 6_400_000),
            Duration::from_millis(1000)
        );
    }

    /// AC-QFTCWX.3: the random value is injected, not drawn inside the
    /// builder, so the browser's crypto source (or a test) can supply it.
    #[test]
    fn auth_plaintext_carries_the_injected_random_value() {
        let uid = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC,
        ];
        let random = 0x0123_4567_89AB_CDEF_u64;
        let p = auth_plaintext(1_790_000_000_000, &uid, random);
        let mut want = [0xFF_u8; 32];
        want[0..8].copy_from_slice(&1_790_000_000_000_u64.to_le_bytes());
        want[8..20].copy_from_slice(&uid);
        want[20..28].copy_from_slice(&random.to_le_bytes());
        assert_eq!(p, want);
        assert_ne!(
            auth_plaintext(1_790_000_000_000, &uid, random ^ 1),
            p,
            "a different random gives a different plaintext"
        );
    }

    /// AC-QFTCWX.3: a session draws the auth random from its clock; on
    /// wasm32 the system clock reads it from getrandom's web backend.
    #[test]
    fn system_clock_random_values_differ() {
        let mut c = crate::session::SystemClock;
        let a = c.random_u64();
        let b = c.random_u64();
        assert_ne!(a, b, "two OS random draws");
    }

    #[test]
    fn auth_plaintext_with_an_overlong_credential_does_not_panic() {
        let p = auth_plaintext(1, &[0xAB; 20], 2);
        assert_eq!(&p[8..24], &[0xAB; 16]);
        assert_eq!(&p[24..32], &2u64.to_le_bytes());
    }
}
