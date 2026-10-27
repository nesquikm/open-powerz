//! `powerz watch --source cdc` (FR 1HHT02, AC .4 and .5): the ADCNEW push
//! stream as the live source, printed in the same line format as the
//! polled (bulk) path, and stopped (`03`) when watch ends. Runs against the
//! simulator's CDC persona on its simulated clock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::Parser;
use powerz_cli::args::WatchSource;
use powerz_cli::commands::watch_cdc;
use powerz_cli::{Cli, Command, WatchOptions, watch};
use powerz_protocol::session::Session;
use powerz_sim::{Persona, Scenario, SimClock, SimDevice};

const T0: u64 = 1_790_000_000_000;

fn text(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).into_owned()
}

/// The line with every number (and the `-` of a missing value) replaced by
/// `#`: what must match between the two paths.
fn shape(line: &str) -> String {
    line.split_whitespace()
        .map(|t| {
            if t == "-" || t.parse::<f64>().is_ok() {
                "#"
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The number after `label` in a watch line.
fn value(line: &str, label: &str) -> f64 {
    let mut words = line.split_whitespace();
    while let Some(w) = words.next() {
        if w == label {
            return words
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("no number after {label} in {line}"));
        }
    }
    panic!("no {label} in {line}");
}

/// Run a watch for three refreshes; returns (result, stdout, session).
fn run(
    persona: Persona,
    cdc: bool,
) -> (
    Result<(), powerz_cli::CliError>,
    String,
    Session<SimDevice, SimClock>,
) {
    let clock = SimClock::new(T0);
    let dev = SimDevice::with_clock(persona, Scenario::Load5v1a, clock.clone());
    let mut s = Session::with_clock(dev, clock.clone());
    if persona == Persona::Bulk {
        // As `powerz` does on bulk: CONNECT first (it also takes the
        // simulator's stale DISCONNECT off the pipe).
        s.connect().expect("CONNECT");
    }
    let stop = AtomicBool::new(false);
    let mut refreshes = 0;
    let mut out = Vec::new();
    let opts = WatchOptions {
        interval: Duration::from_millis(200),
        tty: false,
    };
    let mut sleep = |d: Duration| {
        clock.advance(d);
        refreshes += 1;
        if refreshes == 3 {
            stop.store(true, Ordering::SeqCst);
        }
    };
    let r = if cdc {
        watch_cdc(&mut s, &opts, &stop, &mut sleep, &mut out)
    } else {
        watch(&mut s, &opts, &stop, &mut sleep, &mut out)
    };
    (r, text(&out), s)
}

// ---- AC-1HHT02.4: argument parsing ----

#[test]
fn watch_source_defaults_to_polling_and_takes_cdc() {
    let c = Cli::try_parse_from(["powerz", "watch"]).unwrap();
    assert_eq!(
        c.command,
        Command::Watch {
            interval: 200,
            source: WatchSource::Poll
        }
    );
    let c = Cli::try_parse_from(["powerz", "watch", "--source", "cdc"]).unwrap();
    assert_eq!(
        c.command,
        Command::Watch {
            interval: 200,
            source: WatchSource::Cdc
        }
    );
    let c =
        Cli::try_parse_from(["powerz", "watch", "--source", "poll", "--interval", "50"]).unwrap();
    assert_eq!(
        c.command,
        Command::Watch {
            interval: 50,
            source: WatchSource::Poll
        }
    );
    let e = Cli::try_parse_from(["powerz", "watch", "--source", "hid"]).unwrap_err();
    assert_eq!(e.exit_code(), 2);
}

// ---- AC-1HHT02.4: same output as the polled path ----

#[test]
fn cdc_watch_prints_one_line_per_refresh_in_the_polled_format() {
    let (r, out, _s) = run(Persona::Cdc, true);
    assert_eq!(r, Ok(()));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "one line per refresh:\n{out}");

    let (rb, bulk, _) = run(Persona::Bulk, false);
    assert_eq!(rb, Ok(()));
    let bulk_line = bulk.lines().next().expect("a polled line");
    for l in &lines {
        assert_eq!(shape(l), shape(bulk_line), "\ncdc:  {l}\nbulk: {bulk_line}");
        let v = value(l, "VBUS");
        assert!((4.9..=5.1).contains(&v), "VBUS {v}: {l}");
        let i = value(l, "IBUS");
        assert!((0.95..=1.05).contains(&i), "IBUS {i}: {l}");
        assert!((4.6..=5.4).contains(&value(l, "Power")), "{l}");
    }
}

#[test]
fn cdc_watch_shows_fresh_values_on_each_refresh() {
    // Noisy current: three refreshes 200 ms apart cannot all print the same IBUS.
    let clock = SimClock::new(T0);
    let dev = SimDevice::with_clock(Persona::Cdc, Scenario::NoisyNegative, clock.clone());
    let mut s = Session::with_clock(dev, clock.clone());
    let stop = AtomicBool::new(false);
    let mut n = 0;
    let mut out = Vec::new();
    let opts = WatchOptions {
        interval: Duration::from_millis(200),
        tty: false,
    };
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
    )
    .unwrap();
    let out = text(&out);
    let ibus: Vec<String> = out
        .lines()
        .map(|l| format!("{:.3}", value(l, "IBUS")))
        .collect();
    assert_eq!(ibus.len(), 3);
    assert!(
        ibus[0] != ibus[1] || ibus[1] != ibus[2],
        "the same value three times: {ibus:?}"
    );
}

#[test]
fn cdc_watch_stops_the_stream_when_it_ends() {
    let (r, _out, s) = run(Persona::Cdc, true);
    assert_eq!(r, Ok(()));
    assert!(
        !s.transport().adcnew_active(),
        "the ADCNEW stream still runs after watch ended"
    );
}

#[test]
fn cdc_watch_needs_the_cdc_pipe() {
    // AC-1HHT02.5: never ADCNEW on bulk; the watch fails instead.
    let (r, out, s) = run(Persona::Bulk, true);
    assert!(r.is_err(), "ADCNEW watch ran on the bulk pipe:\n{out}");
    assert!(!s.transport().adcnew_active());
}
