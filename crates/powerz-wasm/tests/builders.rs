//! FR QFTCWY: request builders.
//!
//! - AC-QFTCWY.1: builders for CONNECT, auth (level, time and random from the
//!   caller), GET_DATA with a mask, queue start at a rate, queue stop, PD
//!   sniffer start / stop / poll and memory read.
//! - AC-QFTCWY.5: each builder's bytes equal the request the native session
//!   sends for the same inputs (recorded from a session over the demo sim).

mod common;

use common::*;
use powerz_protocol::decode::Rate;
use powerz_protocol::decode::pd_packet::CaptureClock;
use powerz_protocol::frame::Attribute;
use powerz_protocol::session::PdOptions;
use powerz_wasm::{
    auth_request, connect_request, get_data_request, memory_read_request, pd_poll_request,
    start_pd_sniffer_request, start_queue_request, stop_pd_sniffer_request, stop_queue_request,
};

#[test]
fn connect_matches_the_native_session() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let sent = last_of(&s, 0x02);
    assert_eq!(connect_request(id_of(&sent)), sent.request);
    assert_eq!(connect_request(1), vec![0x02, 0x01, 0x00, 0x00]);
}

#[test]
fn auth_level_1_matches_the_native_session() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let uid = s.memory_read(UID.0, UID.1).unwrap();
    assert_eq!(s.authenticate(1).unwrap(), 1);
    let sent = last_of(&s, 0x4C);
    let built = auth_request(id_of(&sent), 1, &uid, NOW_MS, RANDOM).unwrap();
    assert_eq!(built, sent.request);
    assert_eq!(built.len(), 4 + 32);
}

#[test]
fn auth_level_2_matches_the_native_session_with_the_whole_otp_block() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let otp = s.memory_read(OTP.0, OTP.1).unwrap();
    assert_eq!(otp.len(), 64);
    assert_eq!(s.authenticate(2).unwrap(), 2);
    let sent = last_of(&s, 0x4C);
    // The caller passes the OTP block as read; the binding takes its
    // credential bytes the way the session does.
    assert_eq!(
        auth_request(id_of(&sent), 2, &otp, NOW_MS, RANDOM).unwrap(),
        sent.request
    );
}

#[test]
fn auth_time_and_random_come_from_the_caller() {
    let uid = [0xA5; 12];
    let a = auth_request(3, 1, &uid, NOW_MS, RANDOM).unwrap();
    assert_ne!(a, auth_request(3, 1, &uid, NOW_MS + 1, RANDOM).unwrap());
    assert_ne!(a, auth_request(3, 1, &uid, NOW_MS, RANDOM ^ 1).unwrap());
    assert_eq!(a, auth_request(3, 1, &uid, NOW_MS, RANDOM).unwrap());
}

#[test]
fn auth_with_an_unknown_level_is_an_invalid_input_error() {
    for level in [0u8, 3, 255] {
        let err = auth_request(1, level, &[0xA5; 12], NOW_MS, RANDOM).unwrap_err();
        assert_eq!(err.kind, "invalid", "level {level}: {}", err.message);
        assert!(!err.message.is_empty());
    }
}

#[test]
fn get_data_with_a_mask_matches_the_native_session() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    for mask in [
        Attribute::ADC,
        Attribute::ADC_QUEUE,
        Attribute::PD_PACKET | Attribute::ADC,
        Attribute::SETTINGS,
    ] {
        s.get_data(mask).unwrap();
        let sent = last_of(&s, 0x0C);
        assert_eq!(
            get_data_request(id_of(&sent), mask.bits()),
            sent.request,
            "mask {:#05x}",
            mask.bits()
        );
    }
}

#[test]
fn queue_start_at_each_rate_matches_the_native_session() {
    for rate in Rate::ALL {
        let (mut s, _) = native_session();
        s.connect().unwrap();
        s.start_stream(rate).unwrap();
        let sent = last_of(&s, 0x0E);
        assert_eq!(
            start_queue_request(id_of(&sent), rate.code()).unwrap(),
            sent.request,
            "{rate:?}"
        );
    }
}

#[test]
fn queue_start_with_an_unknown_rate_code_is_an_invalid_input_error() {
    let err = start_queue_request(1, 9).unwrap_err();
    assert_eq!(err.kind, "invalid", "{}", err.message);
}

#[test]
fn queue_stop_matches_the_native_session() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    s.start_stream(Rate::Sps10).unwrap();
    s.stop_stream().unwrap();
    let sent = last_of(&s, 0x0F);
    assert_eq!(stop_queue_request(id_of(&sent)), sent.request);
}

#[test]
fn pd_sniffer_start_poll_and_stop_match_the_native_session() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    s.start_pd_sniffer().unwrap();
    let start = last_of(&s, 0x10);
    assert_eq!(start_pd_sniffer_request(id_of(&start)), start.request);

    let mut clock = CaptureClock::new();
    for with_adc in [false, true] {
        s.poll_pd_sniffer(PdOptions { with_adc }, &mut clock)
            .unwrap();
        let poll = last_of(&s, 0x0C);
        assert_eq!(
            pd_poll_request(id_of(&poll), with_adc),
            poll.request,
            "with_adc {with_adc}"
        );
    }

    s.stop_pd_sniffer().unwrap();
    let stop = last_of(&s, 0x11);
    assert_eq!(stop_pd_sniffer_request(id_of(&stop)), stop.request);
}

#[test]
fn memory_read_of_each_info_block_matches_the_native_session() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    for (addr, len) in [HARDWARE_INFO, FIRMWARE_INFO, OTP, UID] {
        s.memory_read(addr, len).unwrap();
        let sent = last_of(&s, 0x44);
        assert_eq!(
            memory_read_request(id_of(&sent), addr, len).unwrap(),
            sent.request,
            "{addr:#x}/{len}"
        );
    }
}

#[test]
fn memory_read_matches_the_captured_request_frame() {
    let f = frames("memread_0420.hex");
    assert_eq!(memory_read_request(2, 0x420, 64).unwrap(), f[0]);
    let f = frames("memread_4420.hex");
    assert_eq!(memory_read_request(3, 0x4420, 64).unwrap(), f[0]);
}

#[test]
fn memory_read_outside_the_safe_blocks_is_forbidden_like_the_session() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    assert!(s.memory_read(0x0800_0000, 64).is_err());
    let err = memory_read_request(2, 0x0800_0000, 64).unwrap_err();
    assert_eq!(err.kind, "forbidden", "{}", err.message);
}
