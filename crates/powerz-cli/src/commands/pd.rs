//! `powerz pd`.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use powerz_protocol::decode::pd_packet::{
    CcLine, MonitorType, PdEvent, PdMessage, PdRecordKind, ReceiveErrorCause,
};
use powerz_protocol::pd::{Capture, Decoded};
use powerz_protocol::pdcap;
use powerz_protocol::session::{CapturedRecord, Clock, PdOptions, PdPoll, PdSniffer, Session};
use powerz_protocol::transport::Transport;
use serde_json::json;

use crate::error::CliError;
use crate::output::is_closed_pipe;

/// `powerz pd`: start the PD sniffer, poll it every
/// [`PdSniffer::POLL_PERIOD`] and print one line per record (text, or JSON
/// Lines with `json`) until `stop` is set (Ctrl-C); a closed output pipe
/// ends it with [`CliError::OutputClosed`]. The stop request is sent on
/// Ctrl-C, after a device or output error, and on normal exit.
pub fn pd<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    stop: &AtomicBool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    capture(session, json, stop, out, None)
}

/// `powerz pd --save FILE`: [`pd`], and every status sample and record
/// also written to `file` as a pdcap file (FR TGE83E). The header carries
/// the meter's model and firmware version, never its serial number.
pub fn pd_save<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    stop: &AtomicBool,
    out: &mut dyn Write,
    file: &mut dyn Write,
) -> Result<(), CliError> {
    let info = session.device_info()?;
    let header = pdcap::Header::new(env!("CARGO_PKG_VERSION"), info.model, info.firmware_version);
    let mut writer = pdcap::Writer::new(file, &header)?;
    let captured = capture(session, json, stop, out, Some(&mut writer));
    // The file is finished after a device error too: the polls before it
    // are kept. The first error wins; a failed finish after it is told too.
    let finished = writer.finish();
    match (captured, finished) {
        (Ok(()), Ok(_)) => Ok(()),
        (Ok(()), Err(e)) => Err(e.into()),
        (Err(e), Ok(_)) => Err(e),
        (Err(e), Err(f)) => Err(also_unsaved(e, &f)),
    }
}

/// `e`, with the note that the capture file could not be finished (`f`).
fn also_unsaved(e: CliError, f: &std::io::Error) -> CliError {
    let note = |m: String| format!("{m}; the capture file could not be finished: {f}");
    match e {
        CliError::Usage(m) => CliError::Usage(note(m)),
        CliError::NoDevice(m) => CliError::NoDevice(note(m)),
        CliError::Permission(m) => CliError::Permission(note(m)),
        CliError::Device(m) => CliError::Device(note(m)),
        // Harmless alone, but the file is not: that is the failure to tell.
        CliError::OutputClosed => {
            CliError::Device(format!("the capture file could not be finished: {f}"))
        }
    }
}

/// `powerz pd decode FILE`: print the records of a pdcap file in the
/// live `pd` format. No device is involved. On a damaged line the records
/// before it are printed and the error names the file and the line.
pub fn pd_decode(
    input: impl BufRead,
    name: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut reader = pdcap::Reader::new(input).map_err(|e| damaged(name, &e))?;
    // One decoder context for the whole file, as for a live capture.
    let mut decoder = Capture::new();
    for entry in reader.by_ref() {
        match entry {
            Ok(pdcap::Entry::Record {
                elapsed_ms, record, ..
            }) => write_record(
                out,
                &mut decoder,
                &CapturedRecord { elapsed_ms, record },
                json,
            )
            .map_err(output_error)?,
            Ok(pdcap::Entry::Status { .. }) => {}
            Err(e) => {
                out.flush().map_err(output_error)?;
                return Err(damaged(name, &e));
            }
        }
    }
    out.flush().map_err(output_error)?;
    Ok(())
}

/// A failed write to our output: a closed pipe (`powerz pd decode FILE | head`)
/// just ends the command, as it ends `capture`.
fn output_error(e: std::io::Error) -> CliError {
    if is_closed_pipe(&e) {
        CliError::OutputClosed
    } else {
        e.into()
    }
}

/// A file that does not read as pdcap: refused like any other bad input
/// file, as `firmware info` refuses one, since no meter is involved.
fn damaged(name: &str, e: &pdcap::Error) -> CliError {
    CliError::Usage(format!("{name}: {e}"))
}

/// The live capture loop of [`pd`] and [`pd_save`].
fn capture<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    stop: &AtomicBool,
    out: &mut dyn Write,
    mut save: Option<&mut pdcap::Writer<&mut dyn Write>>,
) -> Result<(), CliError> {
    let mut sniffer = PdSniffer::start(session, PdOptions::default())?;
    let mut write_err: Option<std::io::Error> = None;
    let mut closed = false;
    // Session time base of the records: meter time minus elapsed time,
    // fixed by the first record so session time keeps the meter's spacing.
    let mut base: Option<i64> = None;
    // One decoder context per capture, so a Request decodes against the
    // Source_Capabilities seen before it.
    let mut decoder = Capture::new();
    let polled = sniffer.run(|poll| {
        let shown = poll
            .records
            .iter()
            .try_for_each(|r| write_record(out, &mut decoder, r, json))
            .and_then(|()| out.flush());
        // A closed output pipe only ends the capture: this poll still goes
        // to the file, and a failing file stays an error.
        closed = shown.as_ref().err().is_some_and(is_closed_pipe);
        let written = if closed { Ok(()) } else { shown }.and_then(|()| {
            match save.as_deref_mut() {
                // Flushed once per poll, like stdout, so the file on disk
                // keeps up with the capture.
                Some(w) => save_poll(w, poll, &mut base).and_then(|()| w.flush()),
                None => Ok(()),
            }
        });
        if let Err(e) = written {
            write_err = Some(e);
            return false;
        }
        !closed && !stop.load(Ordering::SeqCst)
    });
    if let Err(e) = polled {
        // Dropping the sniffer sends the stop request.
        drop(sniffer);
        return Err(e.into());
    }
    let stopped = sniffer.stop();
    if let Some(e) = write_err {
        return Err(e.into());
    }
    stopped?;
    if closed {
        return Err(CliError::OutputClosed);
    }
    Ok(())
}

/// Write one poll's status block and records to the pdcap file.
fn save_poll(
    w: &mut pdcap::Writer<&mut dyn Write>,
    poll: &PdPoll,
    base: &mut Option<i64>,
) -> std::io::Result<()> {
    if let Some(status) = poll.status {
        w.write(&pdcap::Entry::Status {
            session_ms: u64::from(status.time_ms),
            status,
        })?;
    }
    for r in &poll.records {
        let elapsed = i64::try_from(r.elapsed_ms).unwrap_or(i64::MAX);
        let b = *base.get_or_insert_with(|| i64::from(r.record.time_ms) - elapsed);
        w.write(&pdcap::Entry::Record {
            session_ms: u64::try_from(b.saturating_add(elapsed)).unwrap_or(0),
            elapsed_ms: r.elapsed_ms,
            record: r.record.clone(),
        })?;
    }
    Ok(())
}

/// Feed one record to the capture's PD decoder and print it as a text line
/// or a JSON object line. Events (attach, detach, resets) are fed too, so
/// the decoder's context follows the capture.
fn write_record(
    out: &mut dyn Write,
    decoder: &mut Capture,
    r: &CapturedRecord,
    json: bool,
) -> std::io::Result<()> {
    let decoded = r.record.pd_input().and_then(|i| decoder.feed(i));
    if json {
        writeln!(out, "{}", record_json(r, decoded.as_ref()))
    } else {
        writeln!(out, "{}", record_text(r, decoded.as_ref()))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cc_number(cc: CcLine) -> u8 {
    match cc {
        CcLine::Cc1 => 1,
        CcLine::Cc2 => 2,
        CcLine::Other(n) => n,
    }
}

/// Event name (snake_case) and, for attach/detach, the CC line.
fn event_name(e: &PdEvent) -> (String, Option<u8>) {
    match e {
        PdEvent::Attach(cc) => ("attach".into(), Some(cc_number(*cc))),
        PdEvent::Detach(cc) => ("detach".into(), Some(cc_number(*cc))),
        PdEvent::BadMessageId => ("bad_message_id".into(), None),
        PdEvent::HardReset => ("hard_reset".into(), None),
        PdEvent::CableReset => ("cable_reset".into(), None),
        PdEvent::ReceiveError(_) => ("receive_error".into(), None),
        PdEvent::MonitorType(_) => ("monitor_type".into(), None),
        PdEvent::BigMessage => ("big_message".into(), None),
        PdEvent::Unknown(b) => (format!("unknown_{b:02x}"), None),
    }
}

/// Detail of events that carry one (receive-error cause, monitor type).
fn event_detail(e: &PdEvent) -> Option<String> {
    match e {
        PdEvent::ReceiveError(c) => Some(match c {
            ReceiveErrorCause::Bmc => "bmc".into(),
            ReceiveErrorCause::PulseNumber => "pulse_number".into(),
            ReceiveErrorCause::Freq => "freq".into(),
            ReceiveErrorCause::Kcode => "kcode".into(),
            ReceiveErrorCause::Sop => "sop".into(),
            ReceiveErrorCause::Eop => "eop".into(),
            ReceiveErrorCause::CrcCheck => "crc_check".into(),
            ReceiveErrorCause::DataCount => "data_count".into(),
            ReceiveErrorCause::Unknown => "unknown".into(),
            ReceiveErrorCause::Retries => "retries".into(),
            ReceiveErrorCause::Other(n) => format!("cause_{n}"),
        }),
        PdEvent::MonitorType(m) => Some(match m {
            MonitorType::Pd => "pd".into(),
            MonitorType::Ufcs => "ufcs".into(),
            MonitorType::Vivo => "vivo".into(),
            MonitorType::Other(n) => format!("type_{n}"),
        }),
        _ => None,
    }
}

fn record_json(r: &CapturedRecord, decoded: Option<&Decoded>) -> serde_json::Value {
    let base = |kind: &str| {
        json!({
            "elapsed_ms": r.elapsed_ms,
            "kind": kind,
            "from_meter": r.record.from_meter,
        })
    };
    let message = |kind: &str, m: &PdMessage| {
        let mut v = base(kind);
        v["sop"] = json!(m.sop);
        v["header"] = json!(m.header);
        v["body"] = json!(hex(&m.body));
        if let Some(d) = decoded {
            v["name"] = json!(d.header.message_type.name());
            v["summary"] = json!(d.summary());
        }
        v
    };
    match &r.record.kind {
        PdRecordKind::Message(m) => message("message", m),
        PdRecordKind::BigMessage(m) => message("big_message", m),
        PdRecordKind::Event(e) => {
            let mut v = base("event");
            let (name, cc) = event_name(e);
            v["event"] = json!(name);
            if let Some(cc) = cc {
                v["cc"] = json!(cc);
            }
            if let Some(d) = event_detail(e) {
                v["detail"] = json!(d);
            }
            v
        }
    }
}

fn sop_name(sop: u8) -> String {
    match sop {
        0 => "SOP".into(),
        1 => "SOP'".into(),
        2 => "SOP''".into(),
        n => format!("SOP{n}"),
    }
}

fn record_text(r: &CapturedRecord, decoded: Option<&Decoded>) -> String {
    // b0 bit 6 marks what the meter sent [device]; anything else came from
    // the line, which in a pass-through capture is either port partner.
    let who = if r.record.from_meter { "meter" } else { "line" };
    let what = match &r.record.kind {
        PdRecordKind::Message(m) | PdRecordKind::BigMessage(m) => {
            // The summary starts with the message name.
            let summary = decoded.map(Decoded::summary).unwrap_or_default();
            format!(
                "{:<5} header 0x{:04x} {:<24} {}",
                sop_name(m.sop),
                m.header,
                hex(&m.body),
                summary
            )
        }
        PdRecordKind::Event(e) => {
            let (name, cc) = event_name(e);
            let mut s = name.replace('_', " ");
            if let Some(cc) = cc {
                s.push_str(&format!(" CC{cc}"));
            }
            if let Some(d) = event_detail(e) {
                s.push_str(&format!(" ({d})"));
            }
            s
        }
    };
    format!("{:>10} ms  {who:<7}  {}", r.elapsed_ms, what.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use powerz_protocol::decode::pd_packet::PdRecord;

    fn attach(from_meter: bool) -> CapturedRecord {
        CapturedRecord {
            elapsed_ms: 40,
            record: PdRecord {
                time_ms: 1,
                from_meter,
                raw: vec![0x05, 1, 0, 0, 0, 0x11],
                kind: PdRecordKind::Event(PdEvent::Attach(CcLine::Cc1)),
            },
        }
    }

    #[test]
    fn records_the_meter_did_not_send_are_not_called_charger() {
        // Pass-through capture: either port partner may have sent it.
        let text = record_text(&attach(false), None);
        assert!(!text.contains("charger"), "{text}");
        assert!(text.contains("line"), "{text}");
        assert!(record_text(&attach(true), None).contains("meter"));
    }

    #[test]
    fn event_details_are_stable_snake_case_names() {
        let detail = |e| event_detail(&e).unwrap();
        assert_eq!(
            detail(PdEvent::ReceiveError(ReceiveErrorCause::PulseNumber)),
            "pulse_number"
        );
        assert_eq!(
            detail(PdEvent::ReceiveError(ReceiveErrorCause::CrcCheck)),
            "crc_check"
        );
        assert_eq!(
            detail(PdEvent::ReceiveError(ReceiveErrorCause::DataCount)),
            "data_count"
        );
        assert_eq!(
            detail(PdEvent::ReceiveError(ReceiveErrorCause::Other(12))),
            "cause_12"
        );
        assert_eq!(detail(PdEvent::MonitorType(MonitorType::Ufcs)), "ufcs");
        assert_eq!(
            detail(PdEvent::MonitorType(MonitorType::Other(5))),
            "type_5"
        );
    }

    // ------------------------------------------- --save durability (FR TGE83E)

    use powerz_protocol::session::Session;
    use powerz_sim::{FaultPlan, PdScenario, Persona, Scenario, SimClock, SimDevice};

    /// A file that keeps what was written only once it is flushed, as a
    /// buffered file on disk does; flush number `fail_at` (1-based) fails.
    #[derive(Default)]
    struct FlushedFile {
        pending: Vec<u8>,
        flushed: Vec<u8>,
        flushes: usize,
        fail_at: Option<usize>,
    }

    impl Write for FlushedFile {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.pending.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            if self.fail_at == Some(self.flushes) {
                return Err(std::io::Error::other("disk full"));
            }
            self.flushed.append(&mut self.pending);
            Ok(())
        }
    }

    /// The demo device playing the PD negotiation on a simulated clock, with
    /// the `POWERZ_SIM_FAULT` plan `faults`.
    fn sim_session(faults: &str) -> Session<SimDevice, SimClock> {
        let clock = SimClock::new(1_700_000_000_000);
        let sim = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
            .with_pd_scenario(PdScenario::Negotiation)
            .with_faults(FaultPlan::parse(faults).expect("fault plan"));
        let mut session = Session::with_clock(sim, clock);
        session.connect().expect("CONNECT");
        session
    }

    /// `pd --save` until the device fails (request 40 times out).
    fn save_until_device_error(file: &mut FlushedFile) -> Result<(), CliError> {
        let mut session = sim_session("timeout@40");
        let stop = AtomicBool::new(false);
        let mut out = Vec::new();
        pd_save(&mut session, true, &stop, &mut out, file)
    }

    #[test]
    fn a_device_error_mid_capture_still_finishes_the_save_file() {
        let mut file = FlushedFile::default();
        let result = save_until_device_error(&mut file);
        let err = result.expect_err("the device error is reported");
        assert!(err.to_string().contains("timed out"), "{err}");
        assert!(file.pending.is_empty(), "every line written is flushed");
        let loaded = pdcap::load(std::io::Cursor::new(&file.flushed));
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        assert!(loaded.header.is_some());
        assert!(!loaded.entries.is_empty(), "the polls before the error");
    }

    #[test]
    fn the_save_file_is_flushed_once_per_poll() {
        let mut file = FlushedFile::default();
        let _ = save_until_device_error(&mut file);
        let polls = pdcap::load(std::io::Cursor::new(&file.flushed))
            .entries
            .iter()
            .filter(|e| matches!(e, pdcap::Entry::Status { .. }))
            .count();
        assert!(polls > 1, "several polls before the error: {polls}");
        assert!(
            file.flushes >= polls,
            "{} flushes for {polls} polls",
            file.flushes
        );
    }

    #[test]
    fn a_failed_finish_after_a_device_error_is_reported_with_it() {
        // How many flushes the run takes; the last one is the finish.
        let mut file = FlushedFile::default();
        let _ = save_until_device_error(&mut file);
        let mut failing = FlushedFile {
            fail_at: Some(file.flushes),
            ..FlushedFile::default()
        };
        let err = save_until_device_error(&mut failing).expect_err("fails");
        let text = err.to_string();
        assert!(
            text.contains("timed out"),
            "the first error is kept: {text}"
        );
        assert!(
            text.contains("disk full"),
            "the finish error is reported: {text}"
        );
    }
}
