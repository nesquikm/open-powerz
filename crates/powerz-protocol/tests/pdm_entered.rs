//! The PDM session tracks which protocol is entered, and a request for
//! another protocol enters its own protocol again first (FR DGSQ6S,
//! AC-DGSQ6S.13).
//!
//! Device fact (2026-10-24, click-through 3): detection enters every
//! protocol in turn (`entry pd`, then `entry qc`, ... `entry ufcs`), which
//! leaves the meter in the last legacy protocol; a `pd req=2` sent after
//! that was answered `req error 252`. The scripted port below answers
//! from the 140 W charger's device-confirmed replies (PD) and the
//! synthetic `ready` replies of FR DGSQ6Q (legacy).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::legacy::{self, LegacyProtocol};
use powerz_protocol::pdm::pd::{self, Request as PdRequest};
use powerz_protocol::pdm::{FixedProtocol, HighVoltageConfirmation, PdmSession, Protocol};
use powerz_protocol::session::Clock;

const T0: u64 = 1_790_000_000_000;
const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// SPR `pdo:` block of the 140 W charger, with its `\n`. [device]
fn spr_block() -> Vec<u8> {
    hex("70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a")
}

/// EPR `pdo:` block of the 140 W charger, with its `\n`. [device]
fn epr_block() -> Vec<u8> {
    hex("70646f3a 39 0b 2c918109 2cd10200 2cb10400 f4410600
         00000000 00000000 00000000 f4c10800 8c9630d2 00000000 00000000 0a")
}

/// `entry pd` on the 140 W charger: SPR then EPR offers. [device]
fn epr_entry_reply() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(spr_block());
    b.extend(b"ready:5191mV,0mA\n");
    b.extend(epr_block());
    b.extend(b"ready:5162mV,0mA\n");
    b
}

/// `entry pd` when the charger stays in SPR: no EPR offers (no 28 V).
fn spr_entry_reply() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(spr_block());
    b.extend(b"ready:5191mV,0mA\n");
    b
}

#[derive(Clone, Debug)]
struct FakeClock(Rc<Cell<u64>>);

impl Clock for FakeClock {
    fn now_ms(&mut self) -> u64 {
        self.0.get()
    }
    fn random_u64(&mut self) -> u64 {
        4
    }
    fn sleep(&mut self, d: Duration) {
        self.0.set(self.0.get() + d.as_millis() as u64);
    }
}

#[derive(Default)]
struct State {
    script: HashMap<Vec<u8>, VecDeque<Vec<u8>>>,
    writes: Vec<Vec<u8>>,
    pending: VecDeque<u8>,
}

/// Scripted serial port; clones share state. Replies queued for one
/// command are served in order, the last one repeating.
#[derive(Clone, Default)]
struct FakePort {
    state: Rc<RefCell<State>>,
}

impl FakePort {
    fn on(self, cmd: &str, reply: &[u8]) -> FakePort {
        self.state
            .borrow_mut()
            .script
            .entry(cmd.as_bytes().to_vec())
            .or_default()
            .push_back(reply.to_vec());
        self
    }
    fn writes(&self) -> Vec<String> {
        self.state
            .borrow()
            .writes
            .iter()
            .map(|b| String::from_utf8_lossy(b).trim().to_owned())
            .collect()
    }
    fn len(&self) -> usize {
        self.state.borrow().writes.len()
    }
    fn writes_after(&self, skip: usize) -> Vec<String> {
        self.writes().into_iter().skip(skip).collect()
    }
}

impl std::fmt::Debug for FakePort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakePort")
    }
}

impl Read for FakePort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        if s.pending.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
        }
        let n = buf.len().min(s.pending.len()).min(7);
        for b in buf.iter_mut().take(n) {
            *b = s.pending.pop_front().unwrap();
        }
        Ok(n)
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        s.writes.push(buf.to_vec());
        let key = String::from_utf8_lossy(buf).trim_end().as_bytes().to_vec();
        let reply = s.script.get_mut(&key).and_then(|q| {
            if q.len() > 1 {
                q.pop_front()
            } else {
                q.front().cloned()
            }
        });
        if let Some(r) = reply {
            s.pending.extend(r);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A charger speaking PD (the 140 W offers) and the fixed legacy
/// protocols QC / FCP.
fn port() -> FakePort {
    port_with_pd_entries(&[epr_entry_reply()])
}

/// As [`port`], `entry pd` answered with `entries` in turn (the last one
/// repeating).
fn port_with_pd_entries(entries: &[Vec<u8>]) -> FakePort {
    let mut p = FakePort::default();
    for e in entries {
        p = p.on("entry pd", e);
    }
    p.on("pdm open", OPEN_REPLY)
        .on("pdm close", b"ok\n")
        .on("reset", b"ok\n")
        .on("pd req=1", b"req:5000mV,3000mA\nready:5180mV,0mA\n")
        .on("pd req=2", b"req:9000mV,3000mA\nready:8554mV,0mA\n")
        .on("pd req=8", b"req:28000mV,5000mA\nready:27580mV,0mA\n")
        .on("entry qc", b"ok\nready\n")
        .on("entry fcp", b"ok\nready\n")
        .on("qc 9V", b"ready\n")
        .on("fcp 9V", b"ready\n")
}

fn open(port: &FakePort) -> PdmSession<FakePort, FakeClock> {
    PdmSession::open_with_clock(port.clone(), FakeClock(Rc::new(Cell::new(T0)))).expect("pdm open")
}

fn pd_fixed(position: u8) -> PdRequest {
    PdRequest::Fixed {
        position,
        current_ma: None,
    }
}

fn pd_confirmed(offers: &[pd::Offer], req: &PdRequest) -> Option<HighVoltageConfirmation> {
    let cmd = pd::validate(offers, req).expect("valid request");
    Some(HighVoltageConfirmation::for_command(&cmd))
}

fn qc(volts: u8) -> legacy::Request {
    legacy::Request::Fixed {
        protocol: FixedProtocol::Qc,
        volts,
    }
}

fn legacy_confirmed(
    s: &PdmSession<FakePort, FakeClock>,
    req: &legacy::Request,
) -> Option<HighVoltageConfirmation> {
    let cmd = legacy::validate(s.capabilities(), req).expect("valid request");
    Some(HighVoltageConfirmation::for_command(&cmd))
}

fn count(writes: &[String], cmd: &str) -> usize {
    writes.iter().filter(|w| *w == cmd).count()
}

fn index_of(writes: &[String], cmd: &str) -> usize {
    writes
        .iter()
        .position(|w| w == cmd)
        .unwrap_or_else(|| panic!("{cmd:?} not written; wrote {writes:?}"))
}

mod pdm_entered {
    use super::*;

    // ---- AC-DGSQ6S.13: the session tracks which protocol is entered ----

    #[test]
    fn the_session_reports_the_protocol_entered_last() {
        let port = port();
        let mut s = open(&port);
        assert_eq!(s.entered(), None, "nothing entered after pdm open");
        pd::enter(&mut s).expect("entry pd");
        assert_eq!(s.entered(), Some(Protocol::Pd));
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        assert_eq!(s.entered(), Some(Protocol::Qc));
        legacy::enter(&mut s, LegacyProtocol::Fcp).expect("entry fcp");
        assert_eq!(s.entered(), Some(Protocol::Fcp));
    }

    #[test]
    fn reset_and_pdm_close_clear_the_entered_protocol() {
        let port = port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        legacy::return_to_5v(&mut s).expect("reset");
        assert_eq!(s.entered(), None, "reset leaves the protocol");
        pd::enter(&mut s).expect("entry pd");
        assert_eq!(s.entered(), Some(Protocol::Pd));
        s.close_in_place().expect("pdm close");
        assert_eq!(s.entered(), None, "pdm close leaves the protocol");
    }

    // ---- AC-DGSQ6S.13: a PD request after another protocol enters PD again ----

    #[test]
    fn a_pd_request_after_a_legacy_entry_enters_pd_first() {
        let port = port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).expect("entry pd").offers;
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        let before = port.len();
        let req = pd_fixed(2);
        let out = pd::request(&mut s, &offers, &req, pd_confirmed(&offers, &req))
            .expect("the PD request succeeds");
        let w = port.writes_after(before);
        let entry = index_of(&w, "entry pd");
        let sent = index_of(&w, "pd req=2");
        assert!(entry < sent, "entry pd before the request: {w:?}");
        assert_eq!(count(&w, "entry pd"), 1, "{w:?}");
        assert_eq!(out.requested.voltage_mv, 9000);
        assert_eq!(s.entered(), Some(Protocol::Pd));
    }

    #[test]
    fn a_pd_request_after_a_legacy_entry_is_checked_against_the_refreshed_offers() {
        // The second `entry pd` answers SPR only: 28 V (position 8) is gone.
        let port = port_with_pd_entries(&[epr_entry_reply(), spr_entry_reply()]);
        let mut s = open(&port);
        let offers = pd::enter(&mut s).expect("entry pd").offers;
        assert!(offers.iter().any(|o| o.position == 8), "{offers:?}");
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        let before = port.len();
        let req = pd_fixed(8);
        let err = pd::request(&mut s, &offers, &req, pd_confirmed(&offers, &req));
        let w = port.writes_after(before);
        assert!(w.iter().any(|c| c == "entry pd"), "{w:?}");
        assert!(
            err.is_err(),
            "position 8 is no longer offered after the re-entry: {err:?}"
        );
        assert!(
            !w.iter().any(|c| c.starts_with("pd req=8")),
            "a request the refreshed offers lack writes nothing: {w:?}"
        );
    }

    #[test]
    fn a_pd_request_with_pd_entered_does_not_enter_again() {
        let port = port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).expect("entry pd").offers;
        let before = port.len();
        let req = pd_fixed(2);
        pd::request(&mut s, &offers, &req, pd_confirmed(&offers, &req)).expect("9 V");
        let w = port.writes_after(before);
        assert_eq!(w, ["pd req=2"], "no extra entry pd");
    }

    // ---- AC-DGSQ6S.13: a legacy request after another protocol enters its own ----

    #[test]
    fn a_qc_request_after_pd_was_entered_enters_qc_first() {
        let port = port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        pd::enter(&mut s).expect("entry pd");
        let before = port.len();
        let req = qc(9);
        let out = {
            let conf = legacy_confirmed(&s, &req);
            legacy::request(&mut s, &req, conf)
        }
        .expect("the QC request succeeds");
        let w = port.writes_after(before);
        let entry = index_of(&w, "entry qc");
        let sent = index_of(&w, "qc 9V");
        assert!(entry < sent, "entry qc before the request: {w:?}");
        assert_eq!(out.reply.as_deref().map(str::trim), Some("ready"));
        assert_eq!(s.entered(), Some(Protocol::Qc));
    }

    #[test]
    fn a_qc_request_after_another_legacy_protocol_enters_qc_first() {
        let port = port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        legacy::enter(&mut s, LegacyProtocol::Fcp).expect("entry fcp");
        let before = port.len();
        let req = qc(9);
        {
            let conf = legacy_confirmed(&s, &req);
            legacy::request(&mut s, &req, conf)
        }
        .expect("qc 9V");
        let w = port.writes_after(before);
        let entry = index_of(&w, "entry qc");
        let sent = index_of(&w, "qc 9V");
        assert!(entry < sent, "{w:?}");
    }

    #[test]
    fn a_qc_request_after_reset_enters_qc_again() {
        let port = port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        legacy::return_to_5v(&mut s).expect("reset");
        let before = port.len();
        let req = qc(9);
        {
            let conf = legacy_confirmed(&s, &req);
            legacy::request(&mut s, &req, conf)
        }
        .expect("qc 9V");
        let w = port.writes_after(before);
        assert!(index_of(&w, "entry qc") < index_of(&w, "qc 9V"), "{w:?}");
    }

    #[test]
    fn a_qc_request_with_qc_entered_does_not_enter_again() {
        let port = port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
        let before = port.len();
        let req = qc(9);
        {
            let conf = legacy_confirmed(&s, &req);
            legacy::request(&mut s, &req, conf)
        }
        .expect("qc 9V");
        assert_eq!(port.writes_after(before), ["qc 9V"], "no extra entry qc");
    }
}
