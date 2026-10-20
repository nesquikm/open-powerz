//! Authentication (FR N6EHQC). Every 0x4C exchange is synthetic: the
//! simulated meter (`common::AuthDevice`) decrypts our request with key 3 and
//! echoes it under the reply key, the way firmware 2.0.6 does. Credentials are
//! the made-up UID and OTP block from `common`.

mod common;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::session::{self, Error, Session};

const T0: u64 = 1_790_000_000_000;

fn session(dev: AuthDevice) -> (Session<AuthDevice, FakeClock>, FakeClock) {
    let clock = FakeClock::new(T0);
    (Session::with_clock(dev, clock.clone()), clock)
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

// ---- AC-N6EHQC.7: plaintext and frame builders ----

#[test]
fn session_auth_plaintext_level1_layout() {
    let p = session::auth_plaintext(0x1122_3344_5566_7788, &SYNTH_UID, 0x99AA_BBCC_DDEE_FF00);
    let mut want = 0x1122_3344_5566_7788u64.to_le_bytes().to_vec();
    want.extend(SYNTH_UID);
    want.extend(0x99AA_BBCC_DDEE_FF00u64.to_le_bytes());
    want.extend([0xFF; 4]);
    assert_eq!(p.to_vec(), want);
}

#[test]
fn session_auth_plaintext_level2_layout_fills_32_bytes() {
    let otp = synth_otp();
    let p = session::auth_plaintext(5, &otp[..16], 6);
    let mut want = 5u64.to_le_bytes().to_vec();
    want.extend(&otp[..16]);
    want.extend(6u64.to_le_bytes());
    assert_eq!(p.to_vec(), want);
}

#[test]
fn session_auth_request_frame_is_header_plus_key3_ciphertext() {
    let p = session::auth_plaintext(1, &SYNTH_UID, 2);
    let f = session::auth_request(9, &p);
    assert_eq!(&f[..4], &[0x4C, 0x09, 0x00, 0x02]);
    assert_eq!(f[4..].to_vec(), crypto::encrypt(&KEYS[3], &p));
}

// ---- AC-N6EHQC.7: level 1 and level 2 over a scripted session ----

#[test]
fn session_auth_level1_uses_uid_and_records_level() {
    let dev = AuthDevice::new(memread_steps(1, UID_ADDR, &SYNTH_UID), 1);
    let (mut s, clock) = session(dev);
    assert_eq!(s.authenticate(1).unwrap(), 1);
    assert_eq!(s.auth_level(), 1);

    let dev = s.transport();
    assert_eq!(dev.inner.remaining(), 0);
    assert_eq!(dev.auth_frames.len(), 1);
    assert_eq!(&dev.auth_frames[0][..4], &[0x4C, 0x02, 0x00, 0x02]);
    let p = &dev.auth_plaintexts[0];
    let t = u64_at(p, 0);
    assert!(
        (T0..=clock.0.borrow().now_ms).contains(&t),
        "time {t} not from the clock"
    );
    assert_eq!(&p[8..20], &SYNTH_UID);
    let rnd = u64_at(p, 20);
    assert!(
        clock.0.borrow().randoms.contains(&rnd),
        "random not from the clock"
    );
    assert_eq!(&p[28..], &[0xFF; 4]);
}

#[test]
fn session_auth_level2_uses_first_16_otp_bytes_and_records_level() {
    let otp = synth_otp();
    let dev = AuthDevice::new(memread_steps(1, OTP_ADDR, &otp), 2);
    let (mut s, clock) = session(dev);
    assert_eq!(s.authenticate(2).unwrap(), 2);
    assert_eq!(s.auth_level(), 2);

    let dev = s.transport();
    assert_eq!(dev.inner.remaining(), 0);
    assert_eq!(&dev.auth_frames[0][..4], &[0x4C, 0x02, 0x00, 0x02]);
    let p = &dev.auth_plaintexts[0];
    assert_eq!(&p[8..24], &otp[..16]);
    assert!(clock.0.borrow().randoms.contains(&u64_at(p, 24)));
}

#[test]
fn session_auth_reply_with_id_zero_is_matched() {
    // AuthDevice always answers with id 0; a request id far from 0 proves the
    // 0x4C reply is matched by id 0, not by the request id.
    let mut script = Vec::new();
    for id in 1..=4u8 {
        script.push(powerz_protocol::transport::mock::Step::transact(
            get_data(id, 0x001),
            with_id(vector(ADC_52), id),
        ));
    }
    script.extend(memread_steps(5, UID_ADDR, &SYNTH_UID));
    let (mut s, _) = session(AuthDevice::new(script, 1));
    for _ in 0..4 {
        s.read_adc().unwrap();
    }
    assert_eq!(s.authenticate(1).unwrap(), 1);
    assert_eq!(s.transport().auth_frames[0][1], 6);
}

fn failing(mode: AuthMode) -> Error {
    let dev = AuthDevice::new(memread_steps(1, UID_ADDR, &SYNTH_UID), 1).with_mode(mode);
    let (mut s, _) = session(dev);
    let err = s.authenticate(1).unwrap_err();
    assert_eq!(s.auth_level(), 0, "{mode:?} must not raise the level");
    err
}

#[test]
fn session_auth_wrong_random_echo_fails() {
    assert_eq!(failing(AuthMode::WrongRandom), Error::AuthFailed);
}

#[test]
fn session_auth_wrong_time_echo_fails() {
    assert_eq!(failing(AuthMode::WrongTime), Error::AuthFailed);
}

#[test]
fn session_auth_reply_under_plain_key3_fails() {
    assert_eq!(failing(AuthMode::WrongKey), Error::AuthFailed);
}

#[test]
fn session_auth_reject_fails() {
    assert_eq!(failing(AuthMode::Reject), Error::AuthFailed);
}

#[test]
fn session_auth_no_reply_fails() {
    assert_eq!(failing(AuthMode::Timeout), Error::AuthFailed);
}

#[test]
fn session_consecutive_auth_attempts_are_one_second_apart() {
    let otp = synth_otp();
    let script = [
        memread_steps(1, UID_ADDR, &SYNTH_UID),
        memread_steps(3, OTP_ADDR, &otp),
    ]
    .concat();
    let (mut s, clock) = session(AuthDevice::new(script, 1));
    assert_eq!(s.authenticate(1).unwrap(), 1);
    s.transport_mut().level = 2;
    assert_eq!(s.authenticate(2).unwrap(), 2);
    let dev = s.transport();
    assert_eq!(dev.inner.remaining(), 0);
    assert_eq!(dev.auth_frames.len(), 2);
    assert_eq!(dev.auth_frames[1][1], 4);
    let t1 = u64_at(&dev.auth_plaintexts[0], 0);
    let t2 = u64_at(&dev.auth_plaintexts[1], 0);
    assert!(
        t2 >= t1 + 1000,
        "auth attempts {t1} and {t2} less than 1 s apart"
    );
    assert!(clock.total_slept().as_millis() >= 1000);
}
