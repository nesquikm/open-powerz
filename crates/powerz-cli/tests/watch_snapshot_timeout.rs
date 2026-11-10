//! FR NVN6M7: a snapshot request the meter leaves unanswered (it does while
//! busy with another session) must not end `powerz watch`. The refresh draws
//! nothing, the first such miss of a watch prints one note on stderr, and
//! the next refresh reads a fresh snapshot (AC-NVN6M7.1, AC-NVN6M7.2), in the
//! plain watch and in `watch --source cdc` after its fallback to snapshots
//! (AC-NVN6M7.3). A disconnect or any other failure still ends the watch with
//! exit code 5 (AC-NVN6M7.4).

mod common;
mod stray_port;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{ADC_A, Adc, adc_step, get_data};
use powerz_cli::commands::watch_cdc;
use powerz_cli::{CliError, WatchOptions, watch};
use powerz_protocol::transport::Error as TransportError;
use powerz_protocol::transport::mock::{MockTransport, Step};
use stray_port::{StrayPort, value};

const INTERVAL: Duration = Duration::from_millis(250);

fn text(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).into_owned()
}

fn adc(vbus_uv: i32) -> Adc {
    Adc { vbus_uv, ..ADC_A }
}

/// GET_DATA(ADC) with id `id` that the meter never answers.
fn timeout_step(id: u8) -> Step {
    Step::transact_err(get_data(id, 0x001), TransportError::Timeout)
}

/// What one run of the plain watch left behind.
struct Run {
    result: Result<(), CliError>,
    out: String,
    err: String,
    sleeps: Vec<Duration>,
    remaining: usize,
}

/// Run the plain watch on `script`; `sleep` sets the stop flag on its
/// `refreshes`th call (Ctrl-C).
fn run_plain(script: Vec<Step>, refreshes: usize) -> Run {
    let mut s = common::session(MockTransport::new(script));
    let stop = AtomicBool::new(false);
    let mut sleeps = Vec::new();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let opts = WatchOptions {
        interval: INTERVAL,
        tty: false,
    };
    let result = watch(
        &mut s,
        &opts,
        &stop,
        &mut |d| {
            sleeps.push(d);
            if sleeps.len() == refreshes {
                stop.store(true, Ordering::SeqCst);
            }
        },
        &mut out,
        &mut err,
    );
    let remaining = s.transport().remaining();
    Run {
        result,
        out: text(&out),
        err: text(&err),
        sleeps,
        remaining,
    }
}

/// Lines that are not blank.
fn lines(s: &str) -> Vec<&str> {
    s.lines().filter(|l| !l.trim().is_empty()).collect()
}

/// A note about a missed snapshot: it names the time-out.
fn is_timeout_note(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("timed out") || l.contains("time-out") || l.contains("timeout")
}

// ---- AC-NVN6M7.1: a timed-out snapshot skips one refresh ----

#[test]
fn nvn6m7_a_timed_out_snapshot_skips_the_refresh_and_the_watch_goes_on() {
    let script = vec![
        adc_step(1, adc(5_111_000)),
        timeout_step(2),
        adc_step(3, adc(9_222_000)),
        adc_step(4, adc(12_333_000)),
    ];
    let run = run_plain(script, 4);
    assert_eq!(
        run.result,
        Ok(()),
        "one missed snapshot must not end the watch:\nstdout:\n{}\nstderr:\n{}",
        run.out,
        run.err
    );
    assert_eq!(run.remaining, 0, "every refresh sends its own request");
    assert_eq!(
        run.sleeps,
        vec![INTERVAL; 4],
        "the skipped refresh still waits one interval"
    );
    let drawn = lines(&run.out);
    assert_eq!(
        drawn.len(),
        3,
        "nothing is drawn for the missed refresh:\n{}",
        run.out
    );
    assert!(drawn[0].contains("5.111"), "{}", drawn[0]);
    assert!(
        drawn[1].contains("9.222"),
        "the next refresh shows the fresh reading: {}",
        drawn[1]
    );
    assert!(drawn[2].contains("12.333"), "{}", drawn[2]);
}

#[test]
fn nvn6m7_a_watch_whose_first_snapshot_times_out_still_shows_the_next() {
    let script = vec![timeout_step(1), adc_step(2, adc(9_222_000))];
    let run = run_plain(script, 2);
    assert_eq!(run.result, Ok(()), "stderr:\n{}", run.err);
    let drawn = lines(&run.out);
    assert_eq!(drawn.len(), 1, "{}", run.out);
    assert!(drawn[0].contains("9.222"), "{}", drawn[0]);
}

// ---- AC-NVN6M7.2: the first time-out gets one note on stderr ----

#[test]
fn nvn6m7_the_first_timeout_prints_one_note_and_later_ones_nothing() {
    let script = vec![
        adc_step(1, adc(5_111_000)),
        timeout_step(2),
        adc_step(3, adc(9_222_000)),
        timeout_step(4),
        timeout_step(5),
        adc_step(6, adc(12_333_000)),
    ];
    let run = run_plain(script, 6);
    assert_eq!(run.result, Ok(()), "stderr:\n{}", run.err);
    assert_eq!(run.remaining, 0);
    let notes = lines(&run.err);
    assert_eq!(
        notes.len(),
        1,
        "exactly one note for three time-outs:\n{}",
        run.err
    );
    assert!(
        is_timeout_note(notes[0]),
        "the note says a snapshot timed out: {}",
        notes[0]
    );
    assert!(
        !lines(&run.out).iter().any(|l| is_timeout_note(l)),
        "the note goes to stderr only:\n{}",
        run.out
    );
    assert_eq!(lines(&run.out).len(), 3, "{}", run.out);
}

#[test]
fn nvn6m7_a_watch_without_timeouts_writes_nothing_to_stderr() {
    let script = vec![adc_step(1, adc(5_111_000)), adc_step(2, adc(9_222_000))];
    let run = run_plain(script, 2);
    assert_eq!(run.result, Ok(()));
    assert!(run.err.is_empty(), "{}", run.err);
}

// ---- AC-NVN6M7.3: the same after the CDC watch's snapshot fallback ----

/// Run `watch --source cdc` on `port` for `refreshes` refreshes; returns
/// (result, stdout, stderr).
fn run_cdc(port: &StrayPort, refreshes: u32) -> (Result<(), CliError>, String, String) {
    let (mut s, clock) = stray_port::session(port);
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
fn nvn6m7_the_cdc_fallback_skips_timed_out_snapshots_until_stopped() {
    // The meter echoes every ADCNEW start, so the watch falls back to
    // snapshots; it then leaves snapshot requests 1 and 3 unanswered.
    let port = StrayPort::dropping(false, true, &[1, 3]);
    let (r, out, err) = run_cdc(&port, 6);
    assert_eq!(
        r,
        Ok(()),
        "a missed snapshot ended the fallback:\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(
        port.get_data_requests(),
        6,
        "one snapshot request per refresh, the missed ones included"
    );
    let drawn = lines(&out);
    assert_eq!(
        drawn.len(),
        4,
        "nothing is drawn for the two missed refreshes:\n{out}"
    );
    for (n, l) in drawn.iter().enumerate() {
        let want = StrayPort::vbus_v(u32::try_from(n).expect("small"));
        assert!(
            (value(l, "VBUS") - want).abs() < 0.0005,
            "line {n} shows a fresh reading: {l}"
        );
    }
    let notes = lines(&err);
    assert_eq!(
        notes.len(),
        2,
        "the echo note plus one time-out note:\n{err}"
    );
    assert!(notes[0].to_lowercase().contains("echo"), "{}", notes[0]);
    assert!(is_timeout_note(notes[1]), "{}", notes[1]);
}

// ---- AC-NVN6M7.4: a disconnect or another failure still ends the watch ----

#[test]
fn nvn6m7_a_disconnect_still_ends_the_plain_watch_with_exit_code_5() {
    let script = vec![
        adc_step(1, adc(5_111_000)),
        Step::transact_err(get_data(2, 0x001), TransportError::Disconnected),
        adc_step(3, adc(9_222_000)),
    ];
    let run = run_plain(script, 3);
    let e = run.result.expect_err("a disconnect ends the watch");
    assert!(matches!(e, CliError::Device(_)), "{e:?}");
    assert_eq!(e.exit_code(), 5);
    assert!(e.to_string().contains("disconnected"), "{e}");
    assert_eq!(run.remaining, 1, "nothing is sent after the disconnect");
    assert_eq!(lines(&run.out).len(), 1, "{}", run.out);
}

#[test]
fn nvn6m7_another_transport_failure_still_ends_the_plain_watch() {
    let script = vec![
        adc_step(1, adc(5_111_000)),
        Step::transact_err(
            get_data(2, 0x001),
            TransportError::Io("input/output error".into()),
        ),
        adc_step(3, adc(9_222_000)),
    ];
    let run = run_plain(script, 3);
    let e = run.result.expect_err("an I/O failure ends the watch");
    assert_eq!(e.exit_code(), 5, "{e:?}");
    assert_eq!(run.remaining, 1);
}

#[test]
fn nvn6m7_a_disconnect_after_a_timeout_still_ends_the_plain_watch() {
    let script = vec![
        adc_step(1, adc(5_111_000)),
        timeout_step(2),
        Step::transact_err(get_data(3, 0x001), TransportError::Disconnected),
        adc_step(4, adc(9_222_000)),
    ];
    let run = run_plain(script, 4);
    let e = run.result.expect_err("the disconnect ends the watch");
    assert_eq!(e.exit_code(), 5, "{e:?}");
    assert!(e.to_string().contains("disconnected"), "{e}");
    assert_eq!(run.remaining, 1);
    assert_eq!(lines(&run.err).len(), 1, "the time-out note: {}", run.err);
}

#[test]
fn nvn6m7_a_disconnect_still_ends_the_cdc_fallback_with_exit_code_5() {
    // Falls back to snapshots, misses snapshot 1, then the port goes away
    // at snapshot 3.
    let port = StrayPort::going_away(true, &[1], 3);
    let (r, out, err) = run_cdc(&port, 10);
    let e = r.expect_err("a disconnect ends the fallback watch");
    assert_eq!(e.exit_code(), 5, "{e:?}\nstdout:\n{out}\nstderr:\n{err}");
    assert!(e.to_string().contains("disconnected"), "{e}");
    assert_eq!(lines(&out).len(), 2, "{out}");
    assert_eq!(port.get_data_requests(), 4, "nothing after the disconnect");
}
