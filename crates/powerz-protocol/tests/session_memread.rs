//! Memory read and device info (FR N6EHQC). The 0x420 / 0x4420 blocks are
//! replayed from session-1 captures; the OTP block and the UID are
//! synthetic and encrypted here with the shipped key slot 0.

mod common;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::session::{self, DeviceInfo, Error, Session};
use powerz_protocol::transport::mock::{MockTransport, Step};

fn session(script: Vec<Step>) -> Session<MockTransport> {
    Session::open(MockTransport::new(script))
}

/// CONNECT (id 1) so that the next ids match the captured memreads (2, 3).
fn connect_step() -> Step {
    Step::transact([0x02, 0x01, 0x00, 0x00], accept(1))
}

// ---- AC-N6EHQC.5: memory read ----

#[test]
fn session_memread_plaintext_builder_matches_layout() {
    assert_eq!(
        session::memory_read_plaintext(0x420, 64).to_vec(),
        memread_plain(0x420, 64)
    );
    assert_eq!(
        session::memory_read_plaintext(UID_ADDR, 12).to_vec(),
        memread_plain(UID_ADDR, 12)
    );
}

#[test]
fn session_memread_request_builder_matches_captured_frames() {
    assert_eq!(
        session::memory_read_request(2, 0x420, 64),
        frames(MEMREAD_0420)[0]
    );
    assert_eq!(
        session::memory_read_request(3, 0x4420, 64),
        frames(MEMREAD_4420)[0]
    );
}

#[test]
fn session_memread_replays_captured_info_block() {
    let mut s = session([vec![connect_step()], captured_memread_steps(MEMREAD_0420)].concat());
    s.connect().unwrap();
    let block = s.memory_read(0x420, 64).unwrap();
    assert_eq!(block.len(), 64);
    assert_eq!(&block[0x10..0x16], b"KM003C");
    assert_eq!(&block[0x1C..0x1F], b"2.5");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_memread_collects_data_over_several_transfers() {
    let f = frames(MEMREAD_4420);
    let mut s = session(vec![
        connect_step(),
        // The captured frames carry id 3; after CONNECT this read is id 2.
        Step::transact(
            memread_request(2, 0x4420, 64),
            memread_confirm(2, 0x4420, 64),
        ),
        Step::read(f[2][..16].to_vec()),
        Step::read(f[2][16..48].to_vec()),
        Step::read(f[2][48..].to_vec()),
    ]);
    s.connect().unwrap();
    let block = s.memory_read(0x4420, 64).unwrap();
    assert_eq!(block, crypto::decrypt(&KEYS[0], &f[2]));
    assert_eq!(&block[0x1C..0x21], b"2.0.6");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_memread_uid_reads_rounded_up_length_and_truncates() {
    let mut s = session(memread_steps(1, UID_ADDR, &SYNTH_UID));
    let uid = s.memory_read(UID_ADDR, 12).unwrap();
    assert_eq!(uid, SYNTH_UID.to_vec());
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_memread_otp_block() {
    let otp = synth_otp();
    let mut s = session(memread_steps(1, OTP_ADDR, &otp));
    assert_eq!(s.memory_read(OTP_ADDR, 64).unwrap(), otp);
}

#[test]
fn session_memread_forbidden_addresses_send_nothing() {
    for (addr, len) in [
        (0x9800_0000u32, 64u32),
        (0x420, 32),
        (0x420, 128),
        (0x4420, 12),
        (OTP_ADDR, 16),
        (UID_ADDR, 64),
        (0x0000_0000, 64),
        (0x4001_0454, 12),
    ] {
        let mut s = session(vec![]);
        assert!(
            matches!(s.memory_read(addr, len), Err(Error::Forbidden { .. })),
            "{addr:#x}/{len} must be forbidden"
        );
        assert_eq!(
            s.transport().written().len(),
            0,
            "{addr:#x}/{len} sent bytes"
        );
    }
}

#[test]
fn session_memread_bad_confirmation_echo_is_error() {
    let mut confirm = memread_confirm(1, UID_ADDR, 12);
    confirm[8] ^= 0x01; // length field differs
    let mut s = session(vec![
        Step::transact(memread_request(1, UID_ADDR, 12), confirm),
        Step::read(crypto::encrypt(&KEYS[0], &SYNTH_UID)),
    ]);
    assert!(s.memory_read(UID_ADDR, 12).is_err());
}

#[test]
fn session_memread_short_confirmation_is_error() {
    let confirm = memread_confirm(1, UID_ADDR, 12)[..12].to_vec();
    let mut s = session(vec![Step::transact(
        memread_request(1, UID_ADDR, 12),
        confirm,
    )]);
    assert!(s.memory_read(UID_ADDR, 12).is_err());
}

// ---- AC-N6EHQC.6: device_info() at level 0 ----

#[test]
fn session_device_info_reads_all_four_blocks_at_level_zero() {
    let script = [
        vec![connect_step()],
        captured_memread_steps(MEMREAD_0420),
        captured_memread_steps(MEMREAD_4420),
        memread_steps(4, OTP_ADDR, &synth_otp()),
        memread_steps(5, UID_ADDR, &SYNTH_UID),
    ]
    .concat();
    let mut s = session(script);
    s.connect().unwrap();
    assert_eq!(s.auth_level(), 0);
    let info = s.device_info().unwrap();
    assert_eq!(
        info,
        DeviceInfo {
            model: "KM003C".into(),
            firmware_version: "2.0.6".into(),
            hardware_version: "2.5".into(),
            serial: SYNTH_SERIAL.into(),
            uid: SYNTH_UID_HEX.into(),
        }
    );
    assert_eq!(s.auth_level(), 0);
    assert_eq!(s.transport().remaining(), 0);
    assert!(
        s.transport().written().iter().all(|f| f[0] != 0x4C),
        "device_info must not authenticate"
    );
}
