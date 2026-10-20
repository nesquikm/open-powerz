//! `powerz stream`.

use crate::record::sig8;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::session::{Clock, Gap, Session, Streamer};
use powerz_protocol::transport::Transport;

use crate::error::CliError;
use crate::record;

/// Options of `powerz stream`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamOptions {
    /// Queue rate.
    pub rate: Rate,
    /// Stop once a record at or beyond this elapsed device time arrives
    /// (that record is not written). `None` = until `stop`.
    pub duration: Option<Duration>,
    /// Wall-clock time of the first record, ms since the Unix epoch; the
    /// `Unix` column is this plus the record's elapsed device time, in
    /// seconds.
    pub start_unix_ms: u64,
}

/// What a finished stream recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSummary {
    /// Rows written to the CSV.
    pub samples: u64,
    /// Gaps reported by the session.
    pub gaps: Vec<Gap>,
}

/// `powerz stream`: start the queue at `opts.rate`, poll it with
/// [`Streamer`], write CSV rows to `csv` (header [`record::CSV_HEADER`]),
/// stop the queue when the duration is reached or `stop` is set, and print
/// the sample count and any gaps to `err`.
pub fn stream<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    opts: &StreamOptions,
    stop: &AtomicBool,
    csv: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<StreamSummary, CliError> {
    writeln!(csv, "{}", record::CSV_HEADER)?;
    session.start_stream(opts.rate)?;

    let limit_ms = opts.duration.map(|d| d.as_millis());
    let within_limit = |elapsed_ms: u64| limit_ms.is_none_or(|l| u128::from(elapsed_ms) < l);
    let mut summary = StreamSummary {
        samples: 0,
        gaps: Vec::new(),
    };
    let mut first_ms: Option<u32> = None;
    let mut integrator = record::Integrator::new();
    let mut write_err: Option<std::io::Error> = None;
    let polled = Streamer::run(session, opts.rate, |poll| {
        // The origin is the first record's device time; an empty poll (the
        // meter's first reply after 0x0E) must not fix it.
        let Some(first) = poll.records.first() else {
            return !stop.load(Ordering::SeqCst);
        };
        let start = *first_ms.get_or_insert(first.time_ms());
        let elapsed = |t: u32| u64::from(t.wrapping_sub(start));
        summary.gaps.extend(
            poll.gaps
                .iter()
                .filter(|g| within_limit(elapsed(g.to_ms)))
                .copied(),
        );
        for r in &poll.records {
            let el = elapsed(r.time_ms());
            if !within_limit(el) {
                return false;
            }
            let (charge, energy) = integrator.push(r.time_ms(), r.vbus_v(), r.ibus_a());
            let unix_ms = opts.start_unix_ms + el;
            if let Err(e) = write_row(csv, r, el, unix_ms, charge, energy) {
                write_err = Some(e);
                return false;
            }
            summary.samples += 1;
        }
        !stop.load(Ordering::SeqCst)
    });
    // Always try to stop the queue, even after a failed poll or write.
    let stopped = session.stop_stream();
    polled?;
    if let Some(e) = write_err {
        return Err(e.into());
    }
    stopped?;
    csv.flush()?;

    write_summary(err, &summary)?;
    Ok(summary)
}

/// One CSV row in [`record::CSV_HEADER`] column order; `TEMP` stays empty
/// (queue records carry no temperature).
fn write_row(
    csv: &mut dyn Write,
    r: &QueueRecord,
    elapsed_ms: u64,
    unix_ms: u64,
    charge_ah: f64,
    energy_wh: f64,
) -> std::io::Result<()> {
    let unix = unix_ms as f64 / 1000.0;
    writeln!(
        csv,
        "{},{unix:.3},{},{},{},{},{},{},,{},{},{}",
        record::format_elapsed(elapsed_ms),
        sig8(r.vbus_v()),
        sig8(r.ibus_a()),
        sig8(r.dp_v()),
        sig8(r.dm_v()),
        sig8(r.cc1_v()),
        sig8(r.cc2_v()),
        sig8(charge_ah),
        sig8(energy_wh),
        sig8(r.vbus_v() * r.ibus_a()),
    )
}

/// The sample count, then one line per gap.
fn write_summary(err: &mut dyn Write, summary: &StreamSummary) -> Result<(), CliError> {
    writeln!(err, "{} samples recorded", summary.samples)?;
    if !summary.gaps.is_empty() {
        writeln!(err, "{} gaps in the stream:", summary.gaps.len())?;
        for g in &summary.gaps {
            writeln!(
                err,
                "  gap {} ms -> {} ms: expected {} samples, received {}",
                g.from_ms, g.to_ms, g.expected, g.received
            )?;
        }
    }
    Ok(())
}
