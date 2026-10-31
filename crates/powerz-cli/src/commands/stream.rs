//! `powerz stream`.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_protocol::adcnew::Content;
use powerz_protocol::decode::Rate;
use powerz_protocol::session::{
    Clock, Error as SessionError, Gap, Session, SnapshotCadence, Streamer,
};
use powerz_protocol::transport::Transport;

use crate::error::CliError;
use crate::record::CsvRecorder;

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
    /// seconds. `0` means "read the wall clock when the first record
    /// arrives" (what the CLI uses, so setup time does not shift `Unix`).
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

/// What a finished `stream --source cdc` recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdcStreamSummary {
    /// Rows written to the CSV.
    pub records: u64,
    /// Gaps in the records' device time.
    pub gaps: Vec<Gap>,
    /// Good frames the host dropped because they were not read in time.
    pub dropped_frames: u64,
    /// Frames dropped for a bad CRC or length.
    pub crc_errors: u64,
}

/// Echoed starts after which `stream --source cdc` and `watch --source cdc`
/// give up: the meter echoes a start right after a bulk session `[device]`,
/// and the session sends it again after a quiet spell.
pub const MAX_START_ECHOES: u64 = 3;

/// The error once the meter echoed more than [`MAX_START_ECHOES`] starts.
pub(crate) fn too_many_echoes() -> CliError {
    CliError::Device(
        "the meter keeps echoing the ADCNEW start instead of streaming; unplug and replug it"
            .into(),
    )
}

/// How long `stream` keeps polling without receiving a single record.
const NO_DATA: std::time::Duration = std::time::Duration::from_secs(5);

/// `powerz stream`: start the queue at `opts.rate`, poll it with
/// [`Streamer`], write CSV rows to `csv` (header [`CSV_HEADER`](crate::record::CSV_HEADER)),
/// stop the queue when the duration is reached or `stop` is set, and print
/// the sample count and any gaps to `err`.
pub fn stream<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    opts: &StreamOptions,
    stop: &AtomicBool,
    csv: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<StreamSummary, CliError> {
    session.start_stream(opts.rate)?;
    // Only now: a failed start must not leave a header-only file behind.
    let mut recorder = CsvRecorder::new(csv)?;
    // Give up after this many polls in a row without a record.
    let no_data_limit = NO_DATA.as_millis() / Streamer::poll_period(opts.rate).as_millis().max(1);
    let mut empty_polls: u128 = 0;
    let mut device_err: Option<CliError> = None;

    let limit_ms = opts.duration.map(|d| d.as_millis());
    let within_limit = |elapsed_ms: u64| limit_ms.is_none_or(|l| u128::from(elapsed_ms) < l);
    let mut summary = StreamSummary {
        samples: 0,
        gaps: Vec::new(),
    };
    let mut first_ms: Option<u32> = None;
    let mut write_err: Option<std::io::Error> = None;
    // The latest valid snapshot temperature (for the `TEMP` column), read
    // on the same cadence as the desktop app (session clock), with the
    // session-clock time it was read at. Rows more than
    // `SnapshotCadence::MAX_AGE` after it leave `TEMP` empty (AC-CWWKJ6.4).
    let mut temp_c: Option<(f64, u64)> = None;
    // Session-clock time minus device time, fixed by the first record.
    let mut clock_offset: Option<i64> = None;
    let mut snapshots = SnapshotCadence::default();
    let polled = loop {
        let poll = match session.poll_stream() {
            Ok(poll) => poll,
            Err(e) => break Err(e),
        };
        let poll_now = i64::try_from(session.now_ms()).unwrap_or(i64::MAX);
        let mut on_poll = |poll: &powerz_protocol::session::StreamPoll| -> bool {
            // The origin is the first record's device time; an empty poll (the
            // meter's first reply after 0x0E) must not fix it.
            let Some(first) = poll.records.first() else {
                empty_polls += 1;
                if empty_polls > no_data_limit {
                    device_err = Some(CliError::Device(format!(
                        "no data from the meter for {} s",
                        NO_DATA.as_secs()
                    )));
                    return false;
                }
                return !stop.load(Ordering::SeqCst);
            };
            empty_polls = 0;
            let start = *first_ms.get_or_insert(first.time_ms());
            let offset =
                *clock_offset.get_or_insert(poll_now.saturating_sub(i64::from(first.time_ms())));
            if poll.records.iter().any(|r| r.time_ms() < start) {
                device_err = Some(CliError::Device(
                    "the meter's clock went backwards (did it reboot?); stream stopped".into(),
                ));
                return false;
            }
            let elapsed = |t: u32| u64::from(t - start);
            summary.gaps.extend(
                poll.gaps
                    .iter()
                    .filter(|g| within_limit(elapsed(g.to_ms)))
                    .copied(),
            );
            for r in &poll.records {
                if !within_limit(elapsed(r.time_ms())) {
                    return false;
                }
                // Only the first record's wall clock is used (the Unix origin).
                let now_ms = match opts.start_unix_ms {
                    0 if recorder.rows() == 0 => std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64),
                    fixed => fixed,
                };
                let row_clock_ms = offset.saturating_add(i64::from(r.time_ms()));
                let temp = temp_c
                    .filter(|&(_, read_ms)| {
                        row_clock_ms
                            <= SnapshotCadence::expiry_ms(
                                i64::try_from(read_ms).unwrap_or(i64::MAX),
                            )
                    })
                    .map(|(t, _)| t);
                if let Err(e) = recorder.push_with_temp(r, now_ms, temp) {
                    write_err = Some(e);
                    return false;
                }
                summary.samples += 1;
            }
            !stop.load(Ordering::SeqCst)
        };
        if !on_poll(&poll) {
            break Ok(());
        }
        // After the poll, so requests stay one at a time; a failed or
        // invalid snapshot keeps the latest valid temperature.
        if snapshots.take(session.now_ms())
            && let Some(t) = session.read_adc().ok().and_then(|adc| adc.temp_c())
        {
            temp_c = Some((t, session.now_ms()));
        }
        session.sleep(Streamer::poll_period(opts.rate));
    };
    // Always try to stop the queue, even after a failed poll or write.
    let stopped = session.stop_stream();
    polled?;
    if let Some(e) = device_err {
        recorder.flush()?;
        return Err(e);
    }
    if let Some(e) = write_err {
        return Err(e.into());
    }
    // After a queue backlog overflow the meter may reject the stop
    // `[device 2026-10-24]`: the recording is whole, so that is only a note.
    // Gaps are the only sign of an overflow, so the note does not claim one.
    let stop_rejected = match stopped {
        Err(SessionError::Rejected { .. }) if !summary.gaps.is_empty() => true,
        other => {
            other?;
            false
        }
    };
    recorder.finish()?;

    write_summary(err, &summary)?;
    if stop_rejected {
        writeln!(
            err,
            "note: the meter rejected the stop request after gaps in the stream; \
             the recording is complete, but the queue may still be running"
        )?;
    }
    Ok(summary)
}

/// `powerz stream --source cdc`: start the ADCNEW stream (ADC records at
/// `opts.rate`, no sign-in, no queue), write one CSV row per pushed record
/// (`TEMP` empty: records carry none) until the duration is reached or
/// `stop` is set, stop the stream (`03`) and print one summary line to
/// `err` (records, gaps, dropped frames, CRC errors), then one line per
/// gap. Fails at once, writing no CSV, where the pipe has no ADCNEW stream.
pub fn stream_cdc<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    opts: &StreamOptions,
    stop: &AtomicBool,
    csv: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<CdcStreamSummary, CliError> {
    session.start_adcnew(Content::Adc, opts.rate)?;
    let mut summary = CdcStreamSummary {
        records: 0,
        gaps: Vec::new(),
        dropped_frames: 0,
        crc_errors: 0,
    };
    let recorded = CsvRecorder::new(csv)
        .map_err(CliError::from)
        .and_then(|mut recorder| {
            if let Err(e) = record_cdc(session, opts, stop, &mut recorder, &mut summary, err) {
                // Keep the rows received before the failure.
                let _ = recorder.flush();
                return Err(e);
            }
            recorder.finish()?;
            Ok(())
        });
    // Always try to stop the stream, even after a failed poll or write.
    let stopped = session.stop_adcnew();
    recorded?;
    stopped?;
    let counters = session.adcnew_counters();
    summary.dropped_frames = counters.dropped_frames;
    summary.crc_errors = counters.crc_errors;
    writeln!(
        err,
        "{} records, {} gaps, {} dropped frames, {} CRC errors",
        summary.records,
        summary.gaps.len(),
        summary.dropped_frames,
        summary.crc_errors
    )?;
    write_gaps(err, &summary.gaps)?;
    Ok(summary)
}

/// The polling loop of [`stream_cdc`] on a started stream.
fn record_cdc<T: Transport, C: Clock, W: Write>(
    session: &mut Session<T, C>,
    opts: &StreamOptions,
    stop: &AtomicBool,
    recorder: &mut CsvRecorder<W>,
    summary: &mut CdcStreamSummary,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let period = Streamer::poll_period(opts.rate);
    let limit_ms = opts.duration.map(|d| d.as_millis());
    let within_limit = |elapsed_ms: u64| limit_ms.is_none_or(|l| u128::from(elapsed_ms) < l);
    let mut first_ms: Option<u32> = None;
    let mut waited_ms: u64 = 0;
    let mut told_echo = false;
    while !stop.load(Ordering::SeqCst) {
        let poll = session.poll_adcnew()?;
        if session.adcnew_counters().start_echoes > MAX_START_ECHOES {
            return Err(too_many_echoes());
        }
        if session.adcnew_restart_pending() {
            // The meter echoed the start; the session sends it again after
            // a quiet spell, which does not count as missing data.
            if !told_echo {
                writeln!(
                    err,
                    "the meter echoed the stream start (a bulk session ended moments ago); starting again in {} s",
                    powerz_protocol::adcnew::START_ECHO_QUIET.as_secs()
                )?;
                told_echo = true;
            }
            waited_ms = 0;
            session.sleep(period);
            continue;
        }
        let Some(first) = poll.records.first() else {
            waited_ms += period.as_millis() as u64;
            if waited_ms > NO_DATA.as_millis() as u64 {
                return Err(CliError::Device(format!(
                    "no ADCNEW record from the meter for {} s",
                    NO_DATA.as_secs()
                )));
            }
            session.sleep(period);
            continue;
        };
        waited_ms = 0;
        let start = *first_ms.get_or_insert(first.time_ms());
        if poll.records.iter().any(|r| r.time_ms() < start) {
            return Err(CliError::Device(
                "the meter's clock went backwards (did it reboot?); stream stopped".into(),
            ));
        }
        let elapsed = |t: u32| u64::from(t - start);
        summary.gaps.extend(
            poll.gaps
                .iter()
                .filter(|g| within_limit(elapsed(g.to_ms)))
                .copied(),
        );
        for r in &poll.records {
            if !within_limit(elapsed(r.time_ms())) {
                return Ok(());
            }
            // Only the first record's wall clock is used (the Unix origin).
            let now_ms = match opts.start_unix_ms {
                0 if recorder.rows() == 0 => std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64),
                fixed => fixed,
            };
            recorder.push(r, now_ms)?;
            summary.records += 1;
        }
        session.sleep(period);
    }
    Ok(())
}

/// The sample count, then one line per gap.
fn write_summary(err: &mut dyn Write, summary: &StreamSummary) -> Result<(), CliError> {
    writeln!(err, "{} samples recorded", summary.samples)?;
    if !summary.gaps.is_empty() {
        writeln!(err, "{} gaps in the stream:", summary.gaps.len())?;
        write_gaps(err, &summary.gaps)?;
    }
    Ok(())
}

/// One indented line per gap.
fn write_gaps(err: &mut dyn Write, gaps: &[Gap]) -> Result<(), CliError> {
    for g in gaps {
        writeln!(
            err,
            "  gap {} ms -> {} ms: expected {} samples, received {}",
            g.from_ms, g.to_ms, g.expected, g.received
        )?;
    }
    Ok(())
}
