//! FR QFTCWY: reply decoders.
//!
//! - AC-QFTCWY.2: decoders for the connect / auth reply, ADC snapshot, ADC
//!   queue records (V, I, CC1, CC2, D+, D−, timestamps), PD poll (status
//!   samples and decoded PD messages in the desktop PD table's row shape) and
//!   device info; malformed or short input is an error with a kind, never a
//!   panic.
//! - AC-QFTCWY.5: each decoder agrees with the native decoder on recorded
//!   vectors and on the replies a native session read from the demo sim.
//!
//! Decoded values are checked through their serde form, the shape the
//! binding hands to JS.

mod common;

use std::collections::BTreeSet;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::pd_packet::{CaptureClock, PdChunk, PdRecordKind};
use powerz_protocol::decode::{AdcSnapshot, QueueRecord, Rate};
use powerz_protocol::pd;
use powerz_protocol::session::PdOptions;
use powerz_wasm::{
    PdDecoder, decode_accept, decode_adc, decode_auth_reply, decode_device_info,
    decode_memory_read, decode_queue,
};

// ---- connect / control replies ----

#[test]
fn accept_decodes_the_sessions_connect_reply() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let e = last_of(&s, 0x02);
    decode_accept(&e.reply()).unwrap();
}

#[test]
fn a_reject_is_a_rejected_error() {
    let err = decode_accept(&[0x06, 0x01, 0x00, 0x00]).unwrap_err();
    assert_eq!(err.kind, "rejected", "{}", err.message);
}

#[test]
fn a_short_or_wrong_control_reply_is_a_malformed_error() {
    for bad in [&[][..], &[0x05][..], &[0x05, 0x01, 0x00][..]] {
        let err = decode_accept(bad).unwrap_err();
        assert_eq!(err.kind, "malformed", "{bad:02x?}: {}", err.message);
        assert!(!err.message.is_empty());
    }
    // A PUT_DATA where an ACCEPT was expected is not an accept.
    assert!(decode_accept(&vector("adc_52.hex")).is_err());
}

// ---- auth reply ----

#[test]
fn auth_reply_gives_the_level_the_native_session_got() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let uid = s.memory_read(UID.0, UID.1).unwrap();
    let level = s.authenticate(1).unwrap();
    let e = last_of(&s, 0x4C);
    assert_eq!(
        decode_auth_reply(&e.reply(), &uid, NOW_MS, RANDOM).unwrap(),
        level
    );
}

#[test]
fn auth_reply_with_a_wrong_echo_is_an_auth_failed_error() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let uid = s.memory_read(UID.0, UID.1).unwrap();
    s.authenticate(1).unwrap();
    let reply = last_of(&s, 0x4C).reply();
    let err = decode_auth_reply(&reply, &uid, NOW_MS, RANDOM ^ 1).unwrap_err();
    assert_eq!(err.kind, "auth_failed", "{}", err.message);
    let err = decode_auth_reply(&reply, &uid, NOW_MS + 1, RANDOM).unwrap_err();
    assert_eq!(err.kind, "auth_failed", "{}", err.message);
}

#[test]
fn a_short_auth_reply_is_an_error_not_a_panic() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let uid = s.memory_read(UID.0, UID.1).unwrap();
    s.authenticate(1).unwrap();
    let reply = last_of(&s, 0x4C).reply();
    for n in [0, 1, 3, 4, 10, reply.len() - 1] {
        let err = decode_auth_reply(&reply[..n], &uid, NOW_MS, RANDOM).unwrap_err();
        assert!(!err.kind.is_empty(), "cut at {n}");
    }
}

// ---- ADC snapshot ----

fn assert_adc_matches(v: &serde_json::Value, a: &AdcSnapshot) {
    assert_close(f(v, "vbus_v"), a.vbus_v(), "vbus_v");
    assert_close(f(v, "ibus_a"), a.ibus_a(), "ibus_a");
    assert_close(f(v, "vbus_avg_v"), a.vbus_avg_v(), "vbus_avg_v");
    assert_close(f(v, "ibus_avg_a"), a.ibus_avg_a(), "ibus_avg_a");
    assert_close(f(v, "power_w"), a.power_w(), "power_w");
    assert_close(f(v, "cc1_v"), a.cc1_v(), "cc1_v");
    assert_close(f(v, "cc2_v"), a.cc2_v(), "cc2_v");
    assert_close(f(v, "dp_v"), a.dp_v(), "dp_v");
    assert_close(f(v, "dm_v"), a.dm_v(), "dm_v");
    match a.temp_c() {
        Some(t) => assert_close(f(v, "temp_c"), t, "temp_c"),
        None => assert!(v["temp_c"].is_null()),
    }
}

#[test]
fn adc_snapshot_agrees_with_the_native_decoder_on_the_recorded_vector() {
    let frame = vector("adc_52.hex");
    let native = AdcSnapshot::decode(&frame[8..]).unwrap();
    let v = json(&decode_adc(&frame).unwrap());
    assert_adc_matches(&v, &native);
}

#[test]
fn adc_snapshot_agrees_with_the_sessions_read_adc() {
    let (mut s, clock) = native_session();
    s.connect().unwrap();
    clock.advance(std::time::Duration::from_millis(500));
    let native = s.read_adc().unwrap();
    let e = last_of(&s, 0x0C);
    assert_adc_matches(&json(&decode_adc(&e.reply()).unwrap()), &native);
}

#[test]
fn adc_snapshot_is_found_in_a_chained_adc_settings_reply() {
    let frame = vector("adc_settings_236.hex");
    let native = AdcSnapshot::decode(&frame[8..52]).unwrap();
    assert_adc_matches(&json(&decode_adc(&frame).unwrap()), &native);
}

#[test]
fn malformed_adc_replies_are_errors_with_a_kind() {
    let frame = vector("adc_52.hex");
    for n in [0, 3, 20, 51] {
        let err = decode_adc(&frame[..n]).unwrap_err();
        assert_eq!(err.kind, "malformed", "cut at {n}: {}", err.message);
    }
    // A bare header, or a section header with no payload: no snapshot.
    for n in [4, 8] {
        let err = decode_adc(&frame[..n]).unwrap_err();
        assert!(!err.kind.is_empty(), "cut at {n}");
    }
    // A reply without an ADC section.
    let err = decode_adc(&vector("settings_188.hex")).unwrap_err();
    assert!(!err.kind.is_empty());
}

// ---- ADC queue ----

fn assert_queue_matches(v: &serde_json::Value, r: &QueueRecord) {
    assert_eq!(
        v["time_ms"].as_u64(),
        Some(u64::from(r.time_ms())),
        "time_ms"
    );
    assert_close(f(v, "vbus_v"), r.vbus_v(), "vbus_v");
    assert_close(f(v, "ibus_a"), r.ibus_a(), "ibus_a");
    assert_close(f(v, "cc1_v"), r.cc1_v(), "cc1_v");
    assert_close(f(v, "cc2_v"), r.cc2_v(), "cc2_v");
    assert_close(f(v, "dp_v"), r.dp_v(), "dp_v");
    assert_close(f(v, "dm_v"), r.dm_v(), "dm_v");
}

#[test]
fn queue_records_agree_with_the_native_decoder_on_the_recorded_vector() {
    let frame = vector("queue_1268.hex");
    for rate in [Rate::Sps10, Rate::Sps1000] {
        let native: Vec<QueueRecord> = frame[8..]
            .as_chunks::<20>()
            .0
            .iter()
            .map(|c| QueueRecord::decode(c, rate).unwrap())
            .collect();
        assert_eq!(native.len(), 63);
        let got = json(&decode_queue(&frame, rate.code()).unwrap());
        let got = got.as_array().expect("an array of samples");
        assert_eq!(got.len(), 63, "{rate:?}");
        for (g, n) in got.iter().zip(&native) {
            assert_queue_matches(g, n);
        }
    }
}

#[test]
fn queue_records_agree_with_the_sessions_poll_stream() {
    let (mut s, clock) = native_session();
    s.connect().unwrap();
    s.start_stream(Rate::Sps50).unwrap();
    clock.advance(std::time::Duration::from_millis(1_000));
    let poll = s.poll_stream().unwrap();
    assert!(poll.records.len() >= 10, "{} records", poll.records.len());
    let e = last_of(&s, 0x0C);
    let got = json(&decode_queue(&e.reply(), Rate::Sps50.code()).unwrap());
    let mut got = got.as_array().expect("an array of samples").clone();
    got.sort_by_key(|v| v["time_ms"].as_u64());
    assert_eq!(got.len(), poll.records.len());
    for (g, n) in got.iter().zip(&poll.records) {
        assert_queue_matches(g, n);
    }
}

#[test]
fn malformed_queue_input_is_an_error_with_a_kind() {
    let frame = vector("queue_1268.hex");
    for n in [0, 2, 7] {
        let err = decode_queue(&frame[..n], 1).unwrap_err();
        assert_eq!(err.kind, "malformed", "cut at {n}: {}", err.message);
    }
    let err = decode_queue(&frame, 9).unwrap_err();
    assert_eq!(err.kind, "invalid", "{}", err.message);
}

// ---- PD poll ----

/// The desktop packet table's row fields (`PdRow` in the desktop backend).
const ROW_KEYS: [&str; 13] = [
    "index",
    "elapsed_ms",
    "session_ms",
    "kind",
    "sop",
    "name",
    "message_id",
    "direction",
    "objects",
    "spec_revision",
    "vbus_v",
    "ibus_a",
    "summary",
];

/// The desktop chart's PD status sample fields (`PdStatusSample`).
const STATUS_KEYS: [&str; 5] = ["session_ms", "vbus_v", "ibus_a", "cc1_v", "cc2_v"];

fn keys(v: &serde_json::Value) -> BTreeSet<String> {
    v.as_object()
        .unwrap_or_else(|| panic!("an object: {v}"))
        .keys()
        .cloned()
        .collect()
}

fn pd_payload(frame: &[u8]) -> &[u8] {
    // One PD_PACKET section: main header, section header, payload.
    &frame[8..]
}

/// What the native decoders make of one PD_PACKET payload: the status, and
/// per record (kind, name, message id, summary, elapsed ms).
fn native_rows(payload: &[u8]) -> (serde_json::Value, Vec<serde_json::Value>) {
    let chunk = PdChunk::parse(payload).unwrap();
    let st = chunk.status;
    let status = serde_json::json!({
        "session_ms": st.time_ms,
        "vbus_v": f64::from(st.vbus_mv) / 1000.0,
        "ibus_a": f64::from(st.ibus_ma) / 1000.0,
        "cc1_v": f64::from(st.cc1_mv) / 1000.0,
        "cc2_v": f64::from(st.cc2_mv) / 1000.0,
    });
    let mut capture = pd::Capture::new();
    let mut clock = CaptureClock::new();
    let rows = chunk
        .records
        .iter()
        .map(|r| {
            let elapsed = clock.elapsed_ms(r.time_ms);
            let decoded = r.pd_input().and_then(|i| capture.feed(i));
            match (&r.kind, decoded) {
                (PdRecordKind::Message(_) | PdRecordKind::BigMessage(_), Some(d)) => {
                    serde_json::json!({
                        "kind": "message",
                        "name": d.header.message_type.name(),
                        "message_id": d.header.message_id,
                        "summary": d.summary(),
                        "elapsed_ms": elapsed,
                    })
                }
                _ => serde_json::json!({ "kind": "event", "elapsed_ms": elapsed }),
            }
        })
        .collect();
    (status, rows)
}

fn assert_pd_poll_matches(got: &serde_json::Value, payload: &[u8]) {
    let (status, rows) = native_rows(payload);
    let got_status = got["status"].as_array().expect("status is an array");
    assert_eq!(got_status.len(), 1);
    assert_eq!(
        keys(&got_status[0]),
        STATUS_KEYS.iter().map(|k| k.to_string()).collect()
    );
    for k in STATUS_KEYS {
        assert_close(f(&got_status[0], k), f(&status, k), k);
    }
    let got_rows = got["rows"].as_array().expect("rows is an array");
    assert_eq!(got_rows.len(), rows.len());
    for (i, (g, n)) in got_rows.iter().zip(&rows).enumerate() {
        assert_eq!(keys(g), ROW_KEYS.iter().map(|k| k.to_string()).collect());
        assert_eq!(g["kind"], n["kind"], "row {i}");
        assert_eq!(g["elapsed_ms"], n["elapsed_ms"], "row {i}");
        assert_close(f(g, "vbus_v"), f(&status, "vbus_v"), "row vbus_v");
        assert_close(f(g, "ibus_a"), f(&status, "ibus_a"), "row ibus_a");
        if n["kind"] == "message" {
            assert_eq!(g["name"], n["name"], "row {i}");
            assert_eq!(g["message_id"], n["message_id"], "row {i}");
            assert_eq!(g["summary"], n["summary"], "row {i}");
            assert!(g["sop"].is_string(), "row {i} has a SOP");
            assert!(g["direction"].is_string(), "row {i} has a direction");
        } else {
            assert!(g["sop"].is_null() && g["message_id"].is_null(), "row {i}");
        }
    }
}

#[test]
fn pd_poll_agrees_with_the_native_decoders_on_recorded_vectors() {
    for name in [
        "pd_srccap_144.hex",
        "pd_eprcaps_260.hex",
        "pd_attach_26.hex",
        "pd_psrdy_36.hex",
        "pd_eprmode_60.hex",
        "pd_hardreset_26.hex",
        "pd_detach_334.hex",
    ] {
        let frame = vector(name);
        let mut decoder = PdDecoder::new();
        let got = json(&decoder.decode(&frame).unwrap());
        assert!(got["adc"].is_null(), "{name}: no ADC section");
        assert_pd_poll_matches(&got, pd_payload(&frame));
    }
}

#[test]
fn pd_poll_row_indices_keep_counting_across_polls() {
    let frame = vector("pd_srccap_144.hex");
    let mut decoder = PdDecoder::new();
    let first = json(&decoder.decode(&frame).unwrap());
    let second = json(&decoder.decode(&frame).unwrap());
    let n = first["rows"].as_array().unwrap().len() as u64;
    assert!(n > 0);
    let idx = |v: &serde_json::Value| -> Vec<u64> {
        v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["index"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(idx(&first), (0..n).collect::<Vec<_>>());
    assert_eq!(idx(&second), (n..2 * n).collect::<Vec<_>>());
}

#[test]
fn pd_poll_with_adc_carries_the_snapshot() {
    let frame = vector("adc_settings_pdstatus_242.hex");
    let mut decoder = PdDecoder::new();
    let got = decoder.decode(&frame);
    // This capture chains ADC with other sections; whatever else it holds,
    // the ADC snapshot comes out like the native decoder's.
    let got = json(&got.unwrap());
    let native = AdcSnapshot::decode(&frame[8..52]).unwrap();
    assert_adc_matches(&got["adc"], &native);
}

#[test]
fn pd_poll_agrees_with_the_sessions_sniffer_on_the_demo_sim() {
    let (mut s, clock) = native_session();
    s.connect().unwrap();
    s.start_pd_sniffer().unwrap();
    let mut native_clock = CaptureClock::new();
    let mut decoder = PdDecoder::new();
    let mut messages = 0;
    for _ in 0..30 {
        clock.advance(std::time::Duration::from_millis(100));
        let native = s
            .poll_pd_sniffer(PdOptions { with_adc: false }, &mut native_clock)
            .unwrap();
        let e = last_of(&s, 0x0C);
        let got = json(&decoder.decode(&e.reply()).unwrap());
        let rows = got["rows"].as_array().unwrap();
        assert_eq!(rows.len(), native.records.len());
        for (g, n) in rows.iter().zip(&native.records) {
            assert_eq!(g["elapsed_ms"].as_u64(), Some(n.elapsed_ms));
        }
        messages += rows.iter().filter(|r| r["kind"] == "message").count();
    }
    assert!(messages > 0, "the demo negotiation produced PD messages");
}

#[test]
fn malformed_pd_input_is_an_error_with_a_kind() {
    let frame = vector("pd_srccap_144.hex");
    let mut decoder = PdDecoder::new();
    for n in [0, 1, 6] {
        let err = decoder.decode(&frame[..n]).unwrap_err();
        assert_eq!(err.kind, "malformed", "cut at {n}: {}", err.message);
    }
    // A cut inside the section: an error or skipped records, never a panic.
    for n in 8..frame.len() {
        let _ = decoder.decode(&frame[..n]);
    }
}

// ---- memory read and device info ----

#[test]
fn memory_read_reply_agrees_with_the_captured_info_block() {
    let f = frames("memread_0420.hex");
    let reply: Vec<u8> = f[1..].concat();
    let block = decode_memory_read(&reply, 0x420, 64).unwrap();
    let native = crypto::decrypt(&KEYS[0], &f[2..].concat());
    assert_eq!(block, native[..64]);
    assert_eq!(&block[0x10..0x16], b"KM003C");
}

#[test]
fn memory_read_reply_agrees_with_the_sessions_read_on_the_demo_sim() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    for (addr, len) in [HARDWARE_INFO, FIRMWARE_INFO, OTP, UID] {
        let native = s.memory_read(addr, len).unwrap();
        let e = last_of(&s, 0x44);
        assert_eq!(
            decode_memory_read(&e.reply(), addr, len).unwrap(),
            native,
            "{addr:#x}/{len}"
        );
    }
}

#[test]
fn malformed_memory_read_replies_are_errors_with_a_kind() {
    let f = frames("memread_0420.hex");
    let reply: Vec<u8> = f[1..].concat();
    for n in [0, 4, 19, 20, 21, reply.len() - 1] {
        let err = decode_memory_read(&reply[..n], 0x420, 64).unwrap_err();
        assert_eq!(err.kind, "malformed", "cut at {n}: {}", err.message);
    }
    // A confirmation for another address.
    assert!(decode_memory_read(&reply, 0x4420, 64).is_err());
    let err = decode_memory_read(&[0x06, 0x02, 0x00, 0x00], 0x420, 64).unwrap_err();
    assert_eq!(err.kind, "rejected", "{}", err.message);
}

#[test]
fn device_info_agrees_with_the_sessions_device_info() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let hw = s.memory_read(HARDWARE_INFO.0, HARDWARE_INFO.1).unwrap();
    let fw = s.memory_read(FIRMWARE_INFO.0, FIRMWARE_INFO.1).unwrap();
    let otp = s.memory_read(OTP.0, OTP.1).unwrap();
    let uid = s.memory_read(UID.0, UID.1).unwrap();
    let native = s.device_info().unwrap();
    let v = json(&decode_device_info(&hw, &fw, &otp, &uid).unwrap());
    assert_eq!(v["model"], native.model.as_str());
    assert_eq!(v["firmware_version"], native.firmware_version.as_str());
    assert_eq!(v["hardware_version"], native.hardware_version.as_str());
    assert_eq!(v["serial"], native.serial.as_str());
    assert_eq!(v["uid"], native.uid.as_str());
}

#[test]
fn short_device_info_blocks_are_errors_with_a_kind() {
    let (mut s, _) = native_session();
    s.connect().unwrap();
    let hw = s.memory_read(HARDWARE_INFO.0, HARDWARE_INFO.1).unwrap();
    let fw = s.memory_read(FIRMWARE_INFO.0, FIRMWARE_INFO.1).unwrap();
    let otp = s.memory_read(OTP.0, OTP.1).unwrap();
    let uid = s.memory_read(UID.0, UID.1).unwrap();
    for (h, f_, o, u) in [
        (&[][..], &fw[..], &otp[..], &uid[..]),
        (&hw[..], &fw[..10], &otp[..], &uid[..]),
        (&hw[..], &fw[..], &otp[..3], &uid[..]),
        (&hw[..], &fw[..], &otp[..], &uid[..5]),
    ] {
        let err = decode_device_info(h, f_, o, u).unwrap_err();
        assert_eq!(err.kind, "malformed", "{}", err.message);
    }
}
