//! `powerz pd`.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use powerz_protocol::decode::pd_packet::{
    CcLine, MonitorType, PdEvent, PdMessage, PdRecordKind, ReceiveErrorCause,
};
use powerz_protocol::session::{CapturedRecord, Clock, PdOptions, PdSniffer, Session};
use powerz_protocol::transport::Transport;
use serde_json::json;

use crate::error::CliError;

/// `powerz pd`: start the PD sniffer, poll it every
/// [`PdSniffer::POLL_PERIOD`] and print one line per record (text, or JSON
/// Lines with `json`) until `stop` is set (Ctrl-C). The stop request is
/// sent on Ctrl-C, after a device or output error, and on normal exit.
pub fn pd<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    stop: &AtomicBool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut sniffer = PdSniffer::start(session, PdOptions::default())?;
    let mut write_err: Option<std::io::Error> = None;
    let polled = sniffer.run(|poll| {
        let written = poll
            .records
            .iter()
            .try_for_each(|r| write_record(out, r, json))
            .and_then(|()| out.flush());
        if let Err(e) = written {
            write_err = Some(e);
            return false;
        }
        !stop.load(Ordering::SeqCst)
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
    stopped.map_err(CliError::from)
}

/// Print one record as a text line or a JSON object line.
fn write_record(out: &mut dyn Write, r: &CapturedRecord, json: bool) -> std::io::Result<()> {
    if json {
        writeln!(out, "{}", record_json(r))
    } else {
        writeln!(out, "{}", record_text(r))
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

fn record_json(r: &CapturedRecord) -> serde_json::Value {
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

fn record_text(r: &CapturedRecord) -> String {
    // b0 bit 6 marks what the meter sent [device]; anything else came from
    // the line, which in a pass-through capture is either port partner.
    let who = if r.record.from_meter { "meter" } else { "line" };
    let what = match &r.record.kind {
        PdRecordKind::Message(m) | PdRecordKind::BigMessage(m) => format!(
            "{:<5} header 0x{:04x} {}",
            sop_name(m.sop),
            m.header,
            hex(&m.body)
        ),
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
        let text = record_text(&attach(false));
        assert!(!text.contains("charger"), "{text}");
        assert!(text.contains("line"), "{text}");
        assert!(record_text(&attach(true)).contains("meter"));
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
}
