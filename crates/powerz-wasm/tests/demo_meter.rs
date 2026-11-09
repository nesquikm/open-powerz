//! FR QFTCWY: the demo meter.
//!
//! - AC-QFTCWY.3: a demo meter wraps the simulator's bulk persona on a
//!   caller-driven clock (`transact(bytes) → bytes`, `advance(ms)`); a
//!   session driven only through the exported builders, decoders and this
//!   demo meter authenticates, streams queue samples and captures PD
//!   messages.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::transport::Transport;
use powerz_sim::SimClock;
use powerz_wasm::{
    DemoMeter, PdDecoder, auth_request, connect_request, decode_accept, decode_auth_reply,
    decode_device_info, decode_memory_read, decode_queue, get_data_request, memory_read_request,
    pd_poll_request, start_pd_sniffer_request, start_queue_request, stop_pd_sniffer_request,
    stop_queue_request,
};

/// Sequence ids for a hand-driven session.
struct Ids(u8);

impl Ids {
    fn next(&mut self) -> u8 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }
}

fn demo() -> DemoMeter {
    DemoMeter::new("load-5v-1a").expect("a known demo scenario")
}

#[test]
fn an_unknown_scenario_is_an_invalid_input_error() {
    let err = DemoMeter::new("no-such-scenario").unwrap_err();
    assert_eq!(err.kind, "invalid", "{}", err.message);
}

#[test]
fn the_first_connect_gets_an_accept_not_the_stale_disconnect() {
    let mut m = demo();
    let reply = m.transact(&connect_request(1));
    assert_eq!(reply, vec![0x05, 0x01, 0x00, 0x00]);
}

/// The demo meter answers like the desktop demo device (the simulator's bulk
/// persona with the PD negotiation) for the same requests at the same times.
#[test]
fn the_demo_meter_answers_exactly_as_the_desktop_demo_sim() {
    let clock = SimClock::new(0);
    let mut sim = demo_sim(&clock);
    // The stale DISCONNECT that waits right after open is not a reply.
    while sim.read(Duration::ZERO).is_ok() {}
    let mut m = demo();
    let uid = (UID.0, UID.1);
    let requests: Vec<(u32, Vec<u8>)> = vec![
        (0, connect_request(1)),
        (0, memory_read_request(2, uid.0, uid.1).unwrap()),
        (0, memory_read_request(3, OTP.0, OTP.1).unwrap()),
        (0, get_data_request(4, 0x001)),
        (0, start_pd_sniffer_request(5)),
        (400, pd_poll_request(6, true)),
        (400, pd_poll_request(7, false)),
        (0, stop_pd_sniffer_request(8)),
        (250, get_data_request(9, 0x002)),
    ];
    for (i, (wait, req)) in requests.iter().enumerate() {
        if *wait > 0 {
            clock.advance(Duration::from_millis(u64::from(*wait)));
            m.advance(*wait);
        }
        let mut want = sim.transact(req, Duration::from_secs(1)).unwrap();
        while let Ok(more) = sim.read(Duration::ZERO) {
            want.extend(more);
        }
        assert_eq!(m.transact(req), want, "request #{i} {:02x?}", &req[..4]);
    }
}

#[test]
fn a_session_driven_through_the_binding_authenticates_streams_and_captures_pd() {
    let mut m = demo();
    let mut ids = Ids(0);

    // Connect.
    decode_accept(&m.transact(&connect_request(ids.next()))).unwrap();

    // Device info from the four blocks.
    let mut read = |m: &mut DemoMeter, (addr, len): (u32, u32)| {
        let req = memory_read_request(ids.next(), addr, len).unwrap();
        decode_memory_read(&m.transact(&req), addr, len).unwrap()
    };
    let hw = read(&mut m, HARDWARE_INFO);
    let fw = read(&mut m, FIRMWARE_INFO);
    let otp = read(&mut m, OTP);
    let uid = read(&mut m, UID);
    let info = json(&decode_device_info(&hw, &fw, &otp, &uid).unwrap());
    assert!(
        info["model"].as_str().is_some_and(|s| !s.is_empty()),
        "{info}"
    );
    assert_eq!(info["uid"].as_str().map(str::len), Some(24), "{info}");

    // Auth at level 1 with the caller's time and random value.
    let now = NOW_MS;
    let random = 0x5EED_0000_0000_0001;
    let req = auth_request(ids.next(), 1, &uid, now, random).unwrap();
    let level = decode_auth_reply(&m.transact(&req), &uid, now, random).unwrap();
    assert_eq!(level, 1);

    // Queue at 50 SPS: one second of samples.
    decode_accept(&m.transact(&start_queue_request(ids.next(), 2).unwrap())).unwrap();
    m.advance(1_000);
    let samples =
        json(&decode_queue(&m.transact(&get_data_request(ids.next(), 0x002)), 2).unwrap());
    let samples = samples.as_array().expect("an array of samples");
    assert!(
        samples.len() >= 40,
        "{} samples in 1 s at 50 SPS",
        samples.len()
    );
    for s in samples {
        assert!((f(s, "vbus_v") - 5.0).abs() < 0.5, "VBUS ≈ 5 V: {s}");
        assert!((f(s, "ibus_a") - 1.0).abs() < 0.5, "IBUS ≈ 1 A: {s}");
    }
    let times: Vec<u64> = samples
        .iter()
        .map(|s| s["time_ms"].as_u64().unwrap())
        .collect();
    let mut distinct = times.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), times.len(), "one timestamp per sample");
    decode_accept(&m.transact(&stop_queue_request(ids.next()))).unwrap();

    // PD capture through the negotiation script.
    decode_accept(&m.transact(&start_pd_sniffer_request(ids.next()))).unwrap();
    let mut decoder = PdDecoder::new();
    let mut names = Vec::new();
    let mut status = 0;
    for _ in 0..40 {
        m.advance(100);
        let poll = json(
            &decoder
                .decode(&m.transact(&pd_poll_request(ids.next(), false)))
                .unwrap(),
        );
        status += poll["status"].as_array().unwrap().len();
        for row in poll["rows"].as_array().unwrap() {
            if row["kind"] == "message" {
                names.push(row["name"].as_str().unwrap().to_owned());
            }
        }
    }
    decode_accept(&m.transact(&stop_pd_sniffer_request(ids.next()))).unwrap();
    assert!(status > 0, "status samples arrived");
    for want in ["Source_Capabilities", "Request", "Accept", "PS_RDY"] {
        assert!(
            names.iter().any(|n| n == want),
            "{want} captured; got {names:?}"
        );
    }
}

#[test]
fn transact_on_garbage_never_panics() {
    let mut m = demo();
    for req in [
        &[][..],
        &[0xFF][..],
        &[0x0C, 0x01][..],
        &[0x44, 0x01, 0x01, 0x01][..],
    ] {
        let _ = m.transact(req);
    }
    // The meter still answers afterwards.
    decode_accept(&m.transact(&connect_request(9))).unwrap();
}
