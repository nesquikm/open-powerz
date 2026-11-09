//! FR AHTZW4: a request after a failed PD entry went out against old offers.
//!
//! The PDM session records `entry pd` as entered once it is written (the way
//! back to 5 V needs that), and the request path took the same flag to mean
//! "the offers are fresh". PD now counts as ready for a request only once a
//! negotiation brought offers after the last entry, reset, hard reset, open
//! or close; otherwise the request enters PD first. A request is checked
//! against the latest negotiation's offers, never only the caller's.
//!
//! - AC-AHTZW4.1: after an entry that got no offers, the next request enters
//!   PD again before it writes anything.
//! - AC-AHTZW4.2: ready only after offers arrived since the last entry,
//!   reset, hard reset, open or close.
//! - AC-AHTZW4.3: a position the latest negotiation's offers lack is refused
//!   before anything is written.
//! - AC-AHTZW4.4: a written `entry pd` still counts as possibly in PD for the
//!   way back to 5 V.
//!
//! The scripted port answers from the 140 W charger's device-confirmed
//! replies; replies queued for one command are served in order, the last
//! one repeating. The clock is fake, so the entry wait passes instantly.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::legacy::{self, LegacyProtocol};
use powerz_protocol::pdm::pd::{self, Request as PdRequest};
use powerz_protocol::pdm::{Command, HighVoltageConfirmation, PdmSession, Protocol};
use powerz_protocol::session::Clock;

const T0: u64 = 1_790_000_000_000;
const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";
/// `entry pd` taken, but no offers before the wait ends.
const NO_OFFERS: &[u8] = b"ok\n";

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

/// `pd hrst` answered with the SPR offers only, back at 5 V.
fn spr_hard_reset_reply() -> Vec<u8> {
    let mut b = spr_block();
    b.extend(b"ready:5180mV,0mA\n");
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

/// Scripted serial port; clones share state.
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
    /// Bytes the meter sends unasked.
    fn push(&self, bytes: &[u8]) {
        self.state
            .borrow_mut()
            .pending
            .extend(bytes.iter().copied());
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

/// A charger whose `entry pd` answers `entries` in turn (the last one
/// repeating) and `pd hrst` answers `hrst` in turn.
fn charger(entries: &[Vec<u8>], hrst: &[Vec<u8>]) -> FakePort {
    let mut p = FakePort::default();
    for e in entries {
        p = p.on("entry pd", e);
    }
    for h in hrst {
        p = p.on("pd hrst", h);
    }
    p.on("pdm open", OPEN_REPLY)
        .on("pdm close", b"ok\n")
        .on("reset", b"ok\n")
        .on("pdm parm", b"type=0,em=2,sink=0\n")
        .on("pd req=1", b"req:5000mV,3000mA\nready:5180mV,0mA\n")
        .on("pd req=2", b"req:9000mV,3000mA\nready:8554mV,0mA\n")
        .on("pd req=8", b"req:28000mV,5000mA\nready:27580mV,0mA\n")
        .on("entry qc", b"ok\nready\n")
}

fn open(port: &FakePort) -> PdmSession<FakePort, FakeClock> {
    PdmSession::open_with_clock(port.clone(), FakeClock(Rc::new(Cell::new(T0)))).expect("pdm open")
}

fn fixed(position: u8) -> PdRequest {
    PdRequest::Fixed {
        position,
        current_ma: None,
    }
}

fn confirmed(offers: &[pd::Offer], req: &PdRequest) -> Option<HighVoltageConfirmation> {
    let cmd = pd::validate(offers, req).expect("valid against the caller's offers");
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

fn is_entry_timeout(e: &pd::Error) -> bool {
    matches!(
        e,
        pd::Error::Pdm(powerz_protocol::pdm::Error::Timeout { .. })
    )
}

/// `ensure_entered` on `s`: whether it wrote `entry pd` (PD was not ready).
fn enters_pd(port: &FakePort, s: &mut PdmSession<FakePort, FakeClock>) -> bool {
    let before = port.len();
    let _ = pd::ensure_entered(s);
    port.writes_after(before).iter().any(|w| w == "entry pd")
}

// ---- AC-AHTZW4.1 ----

#[test]
fn a_request_after_an_entry_without_offers_enters_pd_again_first() {
    // AC-AHTZW4.1: regression. The first entry answers, the second gets no
    // offers, the request's entry answers again.
    let port = charger(
        &[epr_entry_reply(), NO_OFFERS.to_vec(), epr_entry_reply()],
        &[],
    );
    let mut s = open(&port);
    let offers = pd::enter(&mut s).expect("entry pd").offers;
    let err = pd::enter(&mut s).expect_err("no offers came");
    assert!(is_entry_timeout(&err), "{err:?}");

    let before = port.len();
    let req = fixed(2);
    let out = pd::request(&mut s, &offers, &req, confirmed(&offers, &req))
        .expect("the request goes out after a fresh entry");
    let w = port.writes_after(before);
    assert_eq!(count(&w, "entry pd"), 1, "{w:?}");
    assert!(
        index_of(&w, "entry pd") < index_of(&w, "pd req=2"),
        "entry pd before the request: {w:?}"
    );
    assert_eq!(w.first().map(String::as_str), Some("entry pd"), "{w:?}");
    assert_eq!(out.requested.voltage_mv, 9000);
}

#[test]
fn a_request_after_a_legacy_entry_and_a_failed_pd_entry_enters_pd_again_first() {
    // AC-AHTZW4.1: the field case. PD listed, QC entered, a PD entry timed
    // out; the next request still enters PD before anything else.
    let port = charger(
        &[epr_entry_reply(), NO_OFFERS.to_vec(), epr_entry_reply()],
        &[],
    );
    let mut s = open(&port);
    let offers = pd::enter(&mut s).expect("entry pd").offers;
    legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
    let err = pd::ensure_entered(&mut s).expect_err("no offers came");
    assert!(is_entry_timeout(&err), "{err:?}");

    let before = port.len();
    let req = fixed(2);
    pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).expect("9 V");
    let w = port.writes_after(before);
    assert_eq!(w, ["entry pd", "pd req=2"], "{w:?}");
}

#[test]
fn a_request_after_an_entry_without_offers_writes_nothing_above_5v_when_the_entry_fails_again() {
    // AC-AHTZW4.1: the re-entry gets no offers either: the request fails
    // with no `pd req=` written.
    let port = charger(&[epr_entry_reply(), NO_OFFERS.to_vec()], &[]);
    let mut s = open(&port);
    let offers = pd::enter(&mut s).expect("entry pd").offers;
    pd::enter(&mut s).expect_err("no offers came");

    let before = port.len();
    let req = fixed(2);
    let err =
        pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).expect_err("no offers again");
    assert!(is_entry_timeout(&err), "{err:?}");
    let w = port.writes_after(before);
    assert_eq!(w, ["entry pd"], "{w:?}");
}

// ---- AC-AHTZW4.2 ----

#[test]
fn pd_is_ready_after_an_entry_that_brought_offers() {
    // AC-AHTZW4.2: offers after the last entry: ready, no second entry.
    let port = charger(&[epr_entry_reply()], &[]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    assert!(!enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_open() {
    // AC-AHTZW4.2: nothing negotiated since `pdm open`.
    let port = charger(&[epr_entry_reply()], &[]);
    let mut s = open(&port);
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_an_entry_that_got_no_offers() {
    // AC-AHTZW4.2: the last entry brought no offers.
    let port = charger(&[epr_entry_reply(), NO_OFFERS.to_vec()], &[]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    pd::enter(&mut s).expect_err("no offers came");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_an_entry_into_another_protocol() {
    // AC-AHTZW4.2: an entry of another protocol is an entry too.
    let port = charger(&[epr_entry_reply()], &[]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_reset() {
    // AC-AHTZW4.2
    let port = charger(&[epr_entry_reply()], &[]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    s.send(&Command::Reset).expect("reset");
    s.wait_for(&["ok"], Duration::from_millis(1000))
        .expect("ok");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_ready_after_a_hard_reset_that_brought_offers() {
    // AC-AHTZW4.2: the Hard Reset's negotiation brought offers.
    let port = charger(&[epr_entry_reply()], &[spr_hard_reset_reply()]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    pd::hard_reset(&mut s).expect("pd hrst");
    assert!(!enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_a_hard_reset_that_got_no_offers() {
    // AC-AHTZW4.2: `pd hrst` written, no offers came.
    let port = charger(&[epr_entry_reply()], &[b"".to_vec()]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    let err = pd::hard_reset(&mut s).expect_err("no offers came");
    assert!(is_entry_timeout(&err), "{err:?}");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_a_hard_reset_on_the_safety_path() {
    // AC-AHTZW4.2: a blind `pd hrst` reads no offers at all.
    let port = charger(&[epr_entry_reply()], &[spr_hard_reset_reply()]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    s.send_safety(&Command::PdHardReset).expect("pd hrst");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_pdm_close() {
    // AC-AHTZW4.2
    let port = charger(&[epr_entry_reply()], &[]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    s.close_in_place().expect("pdm close");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn pd_is_not_ready_after_a_reopen() {
    // AC-AHTZW4.2: `pdm open` again on a fresh port.
    let port = charger(&[epr_entry_reply()], &[]);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    s.reopen(port.clone()).expect("reopen");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn offers_that_arrive_unasked_do_not_make_pd_ready() {
    // AC-AHTZW4.2: "anything else leaves it not ready": a `pdo:` block read
    // between commands is no negotiation after an entry.
    let port = charger(&[epr_entry_reply()], &[]);
    let mut s = open(&port);
    legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
    let mut unasked = spr_block();
    unasked.extend(b"ready:5191mV,0mA\n");
    port.push(&unasked);
    while s.next_item(Duration::from_millis(50)).is_ok() {}
    assert!(s.capabilities().pd.is_some(), "the block was read");
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

// ---- AC-AHTZW4.3 ----

#[test]
fn a_request_is_checked_against_the_latest_entry_not_the_callers_offers() {
    // AC-AHTZW4.3: the second entry offers SPR only; the caller still holds
    // the EPR list. Position 8 is refused, nothing written.
    let port = charger(&[epr_entry_reply(), spr_entry_reply()], &[]);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    assert!(old.iter().any(|o| o.position == 8), "{old:?}");
    let latest = pd::enter(&mut s).expect("entry pd again").offers;
    assert!(!latest.iter().any(|o| o.position == 8), "{latest:?}");

    let before = port.len();
    let req = fixed(8);
    let err = pd::request(&mut s, &old, &req, confirmed(&old, &req))
        .expect_err("position 8 is not offered now");
    assert!(
        matches!(
            err,
            pd::Error::EprUnavailable { position: 8 } | pd::Error::NoOffer { position: 8 }
        ),
        "{err:?}"
    );
    assert_eq!(port.writes_after(before), Vec::<String>::new());
}

#[test]
fn a_request_is_checked_against_the_latest_hard_reset() {
    // AC-AHTZW4.3: a Hard Reset's negotiation is the latest one.
    let port = charger(&[epr_entry_reply()], &[spr_hard_reset_reply()]);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    pd::hard_reset(&mut s).expect("pd hrst");

    let before = port.len();
    let req = fixed(8);
    let err = pd::request(&mut s, &old, &req, confirmed(&old, &req))
        .expect_err("position 8 is not offered after the Hard Reset");
    assert!(
        matches!(
            err,
            pd::Error::EprUnavailable { position: 8 } | pd::Error::NoOffer { position: 8 }
        ),
        "{err:?}"
    );
    assert_eq!(port.writes_after(before), Vec::<String>::new());
}

#[test]
fn a_request_after_a_failed_entry_is_checked_against_the_re_entrys_offers() {
    // AC-AHTZW4.3 with .1: EPR listed, an entry timed out, the request's own
    // entry gets SPR only: position 8 is refused after it, never written.
    let port = charger(
        &[epr_entry_reply(), NO_OFFERS.to_vec(), spr_entry_reply()],
        &[],
    );
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    pd::enter(&mut s).expect_err("no offers came");

    let before = port.len();
    let req = fixed(8);
    let err = pd::request(&mut s, &old, &req, confirmed(&old, &req))
        .expect_err("position 8 is not offered now");
    assert!(
        matches!(
            err,
            pd::Error::EprUnavailable { position: 8 } | pd::Error::NoOffer { position: 8 }
        ),
        "{err:?}"
    );
    let w = port.writes_after(before);
    assert_eq!(w, ["entry pd"], "{w:?}");
}

#[test]
fn a_position_still_offered_goes_out_after_the_latest_entry() {
    // AC-AHTZW4.3: the latest offers still hold position 2: sent.
    let port = charger(&[epr_entry_reply(), spr_entry_reply()], &[]);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    pd::enter(&mut s).expect("entry pd again");
    let before = port.len();
    let req = fixed(2);
    pd::request(&mut s, &old, &req, confirmed(&old, &req)).expect("9 V");
    assert_eq!(port.writes_after(before), ["pd req=2"]);
}

// ---- AC-AHTZW4.4 ----

#[test]
fn a_written_entry_without_offers_still_counts_as_entered() {
    // AC-AHTZW4.4: the meter may be in PD although no offers came.
    let port = charger(&[NO_OFFERS.to_vec()], &[]);
    let mut s = open(&port);
    legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
    pd::enter(&mut s).expect_err("no offers came");
    assert_eq!(s.entered(), Some(Protocol::Pd));
}

#[test]
fn the_way_back_to_5v_after_an_entry_without_offers_requests_5v_without_entering() {
    // AC-AHTZW4.4: as before, `pd req=1` goes out as it is.
    let port = charger(&[epr_entry_reply(), NO_OFFERS.to_vec()], &[]);
    let mut s = open(&port);
    let offers = pd::enter(&mut s).expect("entry pd").offers;
    pd::enter(&mut s).expect_err("no offers came");
    let before = port.len();
    let back = pd::return_to_5v(&mut s, &offers).expect("back to 5 V");
    assert!(matches!(back, pd::BackTo5V::Requested), "{back:?}");
    assert_eq!(port.writes_after(before), ["pd req=1"]);
    assert_eq!(s.entered(), Some(Protocol::Pd));
}
