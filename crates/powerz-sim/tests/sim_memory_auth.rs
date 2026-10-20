//! AC-G0W08Z.4: memory reads of the four allowed blocks return synthetic
//! contents encrypted with key 0 in the confirmation-plus-data shape; any
//! other address gets REJECT (and, like FW 2.0.6 on bulk, so does a memory
//! read before CONNECT).
//! AC-G0W08Z.5: auth at level 1 (UID) and 2 (OTP) through `authenticate`;
//! a wrong credential gets level 0; 0x0E/0x0F/0x10/0x11 are REJECTed below
//! level 1, and 0x0E while a stream runs is REJECTed.

mod common;

use common::*;
use powerz_protocol::decode::{DeviceInfoBlock, OtpBlock, Rate, Uid};
use powerz_protocol::frame;
use powerz_protocol::session::{Error, memory_read_plaintext, memory_read_request};
use powerz_protocol::transport::Transport;

// ---- AC-G0W08Z.4 ----

#[test]
fn device_info_reports_the_synthetic_identity() {
    let (mut s, _clock) = connected(Scenario::Idle);
    let info = s.device_info().unwrap();
    assert_eq!(info.model, "KM003C");
    assert_eq!(info.firmware_version, "2.0.6");
    assert_eq!(info.serial, "123456");
    assert_eq!(info.serial, powerz_sim::SIM_SERIAL);
    assert_eq!(info.uid.len(), 24, "12-byte UID as hex: {}", info.uid);
    assert!(
        info.uid
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "lowercase hex UID: {}",
        info.uid
    );
}

#[test]
fn memory_reads_use_the_confirmation_plus_data_shape() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    for (i, (addr, len)) in ALLOWED.into_iter().enumerate() {
        let id = 0x20 + i as u8;
        let confirm = d.transact(&memory_read_request(id, addr, len), T).unwrap();
        assert_eq!(confirm.len(), 20, "{addr:#x}: {confirm:02x?}");
        assert_eq!(&confirm[..4], &[0xC4, id, 0x01, 0x01], "{addr:#x} header");
        assert_eq!(
            &confirm[4..],
            &memory_read_plaintext(addr, len)[..16],
            "{addr:#x}: confirmation echoes the plaintext"
        );
        let data = d.read(T).unwrap();
        assert_eq!(
            data.len(),
            (len as usize).div_ceil(16) * 16,
            "{addr:#x}: data is one header-less transfer"
        );
    }
}

#[test]
fn memory_blocks_decrypt_with_key_0_to_synthetic_contents() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let fw = DeviceInfoBlock::decode(&raw_memread(&mut d, 0x30, FW_INFO))
        .unwrap()
        .expect("firmware block present");
    assert_eq!(fw.model(), "KM003C");
    assert_eq!(fw.version(), "2.0.6");
    let hw = DeviceInfoBlock::decode(&raw_memread(&mut d, 0x31, HW_INFO))
        .unwrap()
        .expect("hardware block present");
    assert_eq!(hw.model(), "KM003C");
    let otp = OtpBlock::decode(&raw_memread(&mut d, 0x32, OTP)).unwrap();
    assert_eq!(otp.serial(), "123456");
    let uid = Uid::decode(&raw_memread(&mut d, 0x33, UID)).unwrap();
    assert_eq!(uid.to_string().len(), 24);
}

#[test]
fn memory_read_of_any_other_block_is_rejected() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    for (i, (addr, len)) in [
        (0x0800_0000u32, 64u32),
        (0x0000_0420, 32),
        (0x4001_0450, 16),
        (0x9800_0000, 64),
    ]
    .into_iter()
    .enumerate()
    {
        let id = 0x40 + i as u8;
        let reply = d.transact(&memory_read_request(id, addr, len), T).unwrap();
        assert_eq!(reply, vec![0x06, id, 0x00, 0x00], "{addr:#x}/{len}");
    }
    // The device still answers normally afterwards.
    let ok = raw_memread(&mut d, 0x50, UID);
    assert_eq!(ok.len(), 12);
}

#[test]
fn bulk_memory_read_before_connect_is_rejected() {
    let clock = clock();
    let mut s = session(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    assert_eq!(s.device_info(), Err(Error::Rejected { request_type: 0x44 }));
    s.connect().unwrap();
    assert_eq!(s.device_info().unwrap().serial, "123456");
}

// ---- AC-G0W08Z.5 ----

#[test]
fn authenticate_level_1_and_level_2_succeed() {
    let (mut s, _clock) = connected(Scenario::Idle);
    assert_eq!(s.authenticate(1), Ok(1));
    assert_eq!(s.auth_level(), 1);
    assert_eq!(s.authenticate(2), Ok(2));
    assert_eq!(s.auth_level(), 2);
}

#[test]
fn wrong_credential_is_answered_with_level_0() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let mut uid = credential(&mut d, 1);
    uid[0] ^= 0xFF;
    let reply = raw_auth(&mut d, 9, &uid);
    assert_eq!(ty(&reply), 0x4C, "{reply:02x?}");
    assert_eq!(permi(&reply), 0, "wrong UID must give level 0");
    let mut otp = credential(&mut d, 2);
    otp[15] ^= 0x01;
    let reply = raw_auth(&mut d, 10, &otp);
    assert_eq!(permi(&reply), 0, "wrong OTP must give level 0");
    // Still level 0: the queue start is refused.
    let r = d.transact(&frame::start_queue(11, 2), T).unwrap();
    assert_eq!(ty(&r), 0x06, "0x0E after failed auth: {r:02x?}");
}

#[test]
fn gated_commands_are_rejected_below_level_1() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    for (i, req) in [
        frame::start_queue(0x10, 2).to_vec(),
        frame::stop_queue(0x11).to_vec(),
        control(0x10, 0x12, 0x02),
        control(0x11, 0x13, 0x00),
    ]
    .into_iter()
    .enumerate()
    {
        let reply = d.transact(&req, T).unwrap();
        assert_eq!(reply, vec![0x06, 0x10 + i as u8, 0x00, 0x00], "{req:02x?}");
    }
}

#[test]
fn gated_commands_are_accepted_at_level_1_and_start_is_rejected_while_running() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let uid = credential(&mut d, 1);
    assert_eq!(permi(&raw_auth(&mut d, 2, &uid)), 1);
    let accept = |r: &[u8], what: &str| assert_eq!(ty(r), 0x05, "{what}: {r:02x?}");
    let reject = |r: &[u8], what: &str| assert_eq!(ty(r), 0x06, "{what}: {r:02x?}");

    accept(&d.transact(&control(0x10, 3, 0x02), T).unwrap(), "0x10");
    accept(&d.transact(&control(0x11, 4, 0x00), T).unwrap(), "0x11");
    accept(&d.transact(&frame::start_queue(5, 1), T).unwrap(), "0x0E");
    reject(
        &d.transact(&frame::start_queue(6, 2), T).unwrap(),
        "0x0E while running",
    );
    accept(&d.transact(&frame::stop_queue(7), T).unwrap(), "0x0F");
    accept(
        &d.transact(&frame::start_queue(8, 2), T).unwrap(),
        "0x0E after stop",
    );
}

#[test]
fn session_start_stream_authenticates_and_a_raw_restart_is_rejected() {
    let (mut s, _clock) = connected(Scenario::Idle);
    s.start_stream(Rate::Sps50).unwrap();
    assert_eq!(s.auth_level(), 1);
    let r = s
        .transport_mut()
        .transact(&frame::start_queue(0x70, 3), T)
        .unwrap();
    assert_eq!(r[0] & 0x7F, 0x06, "0x0E while a stream runs: {r:02x?}");
    s.stop_stream().unwrap();
    s.start_stream(Rate::Sps1000).unwrap();
}
