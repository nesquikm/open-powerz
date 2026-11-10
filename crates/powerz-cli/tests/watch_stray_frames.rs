//! FR 3B70S8: `powerz watch` over the CDC transport while another program
//! streams from the meter over USB. The default (polled) watch must take
//! only the reply carrying its request's id and skip the stream frames,
//! echoed starts, text lines and replies to other ids around it
//! (AC-3B70S8.2); `stream --source cdc` keeps giving up once the meter
//! echoed the start more than the echo limit (AC-3B70S8.4).

mod stray_port;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_cli::commands::stream_cdc;
use powerz_cli::{StreamOptions, WatchOptions, watch};
use powerz_protocol::decode::Rate;
use stray_port::{StrayPort, T0, session, value};

fn text(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).into_owned()
}

/// Run the default watch for `refreshes` refreshes; returns (result, stdout).
fn run_watch(port: &StrayPort, refreshes: u32) -> (Result<(), powerz_cli::CliError>, String) {
    let (mut s, clock) = session(port);
    let stop = AtomicBool::new(false);
    let mut n = 0;
    let mut out = Vec::new();
    let opts = WatchOptions {
        interval: Duration::from_millis(200),
        tty: false,
    };
    let r = watch(
        &mut s,
        &opts,
        &stop,
        &mut |d| {
            clock.advance(d);
            n += 1;
            if n == refreshes {
                stop.store(true, Ordering::SeqCst);
            }
        },
        &mut out,
        &mut Vec::new(),
    );
    (r, text(&out))
}

// ---- AC-3B70S8.2: the default watch skips stray frames ----

#[test]
fn the_scripted_port_serves_a_plain_watch() {
    // Control: without stray frames the harness gives four fresh lines.
    let port = StrayPort::new(false, false);
    let (r, out) = run_watch(&port, 4);
    assert_eq!(r, Ok(()), "{out}");
    assert_eq!(out.lines().count(), 4, "{out}");
}

#[test]
fn the_default_watch_keeps_printing_fresh_readings_between_stray_frames() {
    let port = StrayPort::new(true, false);
    let (r, out) = run_watch(&port, 5);
    assert_eq!(r, Ok(()), "watch stopped early:\n{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "one line per refresh:\n{out}");
    for (n, l) in lines.iter().enumerate() {
        let want = StrayPort::vbus_v(u32::try_from(n).expect("small"));
        let got = value(l, "VBUS");
        assert!(
            (got - want).abs() < 0.0005,
            "refresh {n} shows VBUS {got}, its own reply says {want}: {l}"
        );
    }
    assert_eq!(port.get_data_requests(), 5, "one request per refresh");
}

// ---- AC-3B70S8.4: stream --source cdc still gives up ----

#[test]
fn the_cdc_stream_still_gives_up_after_the_echo_limit() {
    let port = StrayPort::new(false, true);
    let (mut s, _clock) = session(&port);
    let stop = AtomicBool::new(false);
    let (mut csv, mut err) = (Vec::new(), Vec::new());
    let opts = StreamOptions {
        rate: Rate::Sps50,
        duration: None,
        start_unix_ms: T0,
    };
    let e = stream_cdc(&mut s, &opts, &stop, &mut csv, &mut err)
        .expect_err("a meter that only echoes the start never streams");
    assert!(e.to_string().contains("keeps echoing"), "{e}");
    let starts = port
        .written()
        .iter()
        .filter(|w| w.as_slice() == [0x02, 0x02])
        .count();
    assert_eq!(starts, 4, "the start and three more after echoes");
    assert_eq!(
        port.get_data_requests(),
        0,
        "no snapshot fallback in stream"
    );
}
