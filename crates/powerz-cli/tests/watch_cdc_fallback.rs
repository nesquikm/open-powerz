//! FR 3B70S8 AC-3B70S8.3: `powerz watch --source cdc` on a meter that keeps
//! echoing the ADCNEW start (it does as long as another program reads it
//! over USB `[device]`) prints one note to stderr once the echoes pass the
//! limit, then goes on with ADC snapshots on the same port, in the polled
//! watch's line format, until stopped.

mod stray_port;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_cli::commands::watch_cdc;
use powerz_cli::{WatchOptions, watch};
use stray_port::{StrayPort, session, shape, value};

/// Echoed starts `watch --source cdc` and `stream --source cdc` accept.
const ECHO_LIMIT: usize = 3;

fn text(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).into_owned()
}

/// Run `watch --source cdc` for `refreshes` refreshes on `port`; returns
/// (result, stdout, stderr).
fn run_cdc(port: &StrayPort, refreshes: u32) -> (Result<(), powerz_cli::CliError>, String, String) {
    let (mut s, clock) = session(port);
    let stop = AtomicBool::new(false);
    let mut n = 0;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let opts = WatchOptions {
        interval: Duration::from_millis(200),
        tty: false,
    };
    let r = watch_cdc(
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
        &mut err,
    );
    (r, text(&out), text(&err))
}

#[test]
fn a_cdc_watch_past_the_echo_limit_falls_back_to_snapshots() {
    let port = StrayPort::new(false, true);
    let (r, out, err) = run_cdc(&port, 5);
    assert_eq!(r, Ok(()), "watch gave up:\nstdout:\n{out}\nstderr:\n{err}");

    let starts = port
        .written()
        .iter()
        .filter(|w| w.len() == 2 && w[0] == 0x02)
        .count();
    assert!(
        starts > ECHO_LIMIT,
        "the start was echoed more than the limit: {starts}"
    );

    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "one line per refresh:\n{out}");
    assert_eq!(
        port.get_data_requests(),
        5,
        "each refresh reads one ADC snapshot on the same port"
    );
    for (n, l) in lines.iter().enumerate() {
        let want = StrayPort::vbus_v(u32::try_from(n).expect("small"));
        assert!((value(l, "VBUS") - want).abs() < 0.0005, "refresh {n}: {l}");
    }
}

#[test]
fn the_fallback_prints_one_note_to_stderr() {
    let port = StrayPort::new(false, true);
    let (r, out, err) = run_cdc(&port, 6);
    assert_eq!(r, Ok(()), "stdout:\n{out}\nstderr:\n{err}");
    let notes: Vec<&str> = err.lines().collect();
    assert_eq!(notes.len(), 1, "exactly one note:\n{err}");
    let note = notes[0].to_lowercase();
    assert!(note.contains("echo"), "{note}");
    assert!(note.contains("snapshot"), "{note}");
    assert!(
        !out.contains("echo"),
        "the note goes to stderr only:\n{out}"
    );
}

#[test]
fn the_fallback_lines_match_the_polled_watch_format() {
    let port = StrayPort::new(false, true);
    let (r, out, err) = run_cdc(&port, 3);
    assert_eq!(r, Ok(()), "{err}");

    // The polled watch on a port that answers the same snapshots.
    let polled_port = StrayPort::new(false, false);
    let (mut s, clock) = session(&polled_port);
    let stop = AtomicBool::new(false);
    let mut polled = Vec::new();
    let opts = WatchOptions {
        interval: Duration::from_millis(200),
        tty: false,
    };
    watch(
        &mut s,
        &opts,
        &stop,
        &mut |d| {
            clock.advance(d);
            stop.store(true, Ordering::SeqCst);
        },
        &mut polled,
        &mut Vec::new(),
    )
    .expect("the polled watch");
    let polled = text(&polled);
    let polled_line = polled.lines().next().expect("a polled line");

    for l in out.lines() {
        assert_eq!(
            shape(l),
            shape(polled_line),
            "\ncdc:    {l}\npolled: {polled_line}"
        );
        // Snapshots carry the temperature, unlike pushed records.
        value(l, "Temp");
    }
}

#[test]
fn a_cdc_watch_that_streams_writes_nothing_to_stderr() {
    // The simulator streams normally: no note, no fallback.
    use powerz_protocol::session::Session;
    use powerz_sim::{Persona, Scenario, SimClock, SimDevice};
    let clock = SimClock::new(stray_port::T0);
    let dev = SimDevice::with_clock(Persona::Cdc, Scenario::Load5v1a, clock.clone());
    let mut s = Session::with_clock(dev, clock.clone());
    let stop = AtomicBool::new(false);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let opts = WatchOptions {
        interval: Duration::from_millis(200),
        tty: false,
    };
    let mut n = 0;
    watch_cdc(
        &mut s,
        &opts,
        &stop,
        &mut |d| {
            clock.advance(d);
            n += 1;
            if n == 3 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        &mut out,
        &mut err,
    )
    .expect("the stream watch");
    assert_eq!(text(&out).lines().count(), 3);
    assert!(err.is_empty(), "{}", text(&err));
}
