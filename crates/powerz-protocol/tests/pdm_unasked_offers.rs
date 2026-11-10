//! FR 3B70SA: an unasked PD offer list did not refresh the ready offers.
//!
//! The reply reader takes every `pdo:` block into the capabilities, but the
//! offers a request is checked against were set only at the end of a
//! negotiation. A list the charger sends unasked while PD is ready now
//! replaces them; an unasked list with no usable offer leaves PD not ready.
//!
//! - AC-3B70SA.1: regression: an unasked list while PD is ready replaces
//!   the ready offers; a position only the new list lacks is refused before
//!   anything is written.
//! - AC-3B70SA.2: while PD is not ready, an unasked list updates the
//!   capabilities only and PD stays not ready.
//! - AC-3B70SA.3: an unasked list with no usable offer leaves PD not ready,
//!   so the next request enters PD again before it writes anything.
//!
//! The scripted port answers from the 140 W charger's device-confirmed
//! replies; replies queued for one command are served in order, the last
//! one repeating. Bytes pushed with [`FakePort::push`] arrive unasked. The
//! clock is fake, so every wait passes instantly.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::legacy::{self, LegacyProtocol};
use powerz_protocol::pdm::pd::{self, Offer, Request as PdRequest};
use powerz_protocol::pdm::{Command, HighVoltageConfirmation, PdmSession};
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

/// SPR `pdo:` block of the 140 W charger, with its `\n`: positions 1–4. [device]
fn spr_block() -> Vec<u8> {
    hex("70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a")
}

/// EPR `pdo:` block of the 140 W charger, with its `\n`: 1–4, 8, 9. [device]
fn epr_block() -> Vec<u8> {
    hex("70646f3a 39 0b 2c918109 2cd10200 2cb10400 f4410600
         00000000 00000000 00000000 f4c10800 8c9630d2 00000000 00000000 0a")
}

/// A shorter SPR list (5 V and 9 V only), as a charger might send when it
/// lowers its offer. Synthetic, built from the 140 W charger's first two PDOs.
fn short_block() -> Vec<u8> {
    hex("70646f3a 34 0b 2c918109 2cd10200 00000000 00000000
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a")
}

/// A `pdo:` block with every slot unused: no usable offer. Synthetic.
fn empty_block() -> Vec<u8> {
    hex("70646f3a 34 0b 00000000 00000000 00000000 00000000
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a")
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

/// The 9 V request's reply with `block` arriving unasked between `req:` and
/// `ready:`.
fn req2_reply_with(block: &[u8]) -> Vec<u8> {
    let mut b = b"req:9000mV,3000mA\n".to_vec();
    b.extend(block);
    b.extend(b"ready:8554mV,0mA\n");
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

/// The 140 W charger: `entry pd` answers `entries` in turn (the last one
/// repeating), `pd req=2` answers `req2` (or the plain 9 V reply).
fn charger(entries: &[Vec<u8>], req2: Option<Vec<u8>>) -> FakePort {
    let mut p = FakePort::default();
    for e in entries {
        p = p.on("entry pd", e);
    }
    let req2 = req2.unwrap_or_else(|| b"req:9000mV,3000mA\nready:8554mV,0mA\n".to_vec());
    p.on("pdm open", OPEN_REPLY)
        .on("pdm close", b"ok\n")
        .on("reset", b"ok\n")
        .on("pd req=1", b"req:5000mV,3000mA\nready:5180mV,0mA\n")
        .on("pd req=2", &req2)
        .on("pd req=3", b"req:15000mV,3000mA\nready:14890mV,0mA\n")
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

fn confirmed(offers: &[Offer], req: &PdRequest) -> Option<HighVoltageConfirmation> {
    let cmd = pd::validate(offers, req).expect("valid against the caller's offers");
    Some(HighVoltageConfirmation::for_command(&cmd))
}

/// Read everything the port has pending (unasked lines and blocks).
fn drain(s: &mut PdmSession<FakePort, FakeClock>) {
    while s.next_item(Duration::from_millis(50)).is_ok() {}
}

/// `block` arrives unasked and is read between commands.
fn arrives_unasked(port: &FakePort, s: &mut PdmSession<FakePort, FakeClock>, block: &[u8]) {
    port.push(block);
    drain(s);
}

fn positions(offers: &[Offer]) -> Vec<u8> {
    offers.iter().map(|o| o.position).collect()
}

fn is_refused(err: &pd::Error, position: u8) -> bool {
    matches!(
        err,
        pd::Error::EprUnavailable { position: p } | pd::Error::NoOffer { position: p }
            if *p == position
    )
}

/// `ensure_entered` on `s`: whether it wrote `entry pd` (PD was not ready).
fn enters_pd(port: &FakePort, s: &mut PdmSession<FakePort, FakeClock>) -> bool {
    let before = port.len();
    let _ = pd::ensure_entered(s);
    port.writes_after(before).iter().any(|w| w == "entry pd")
}

// ---- AC-3B70SA.1 ----

#[test]
fn an_unasked_list_while_ready_replaces_the_ready_offers() {
    // AC-3B70SA.1: ready on the EPR list, then the SPR list comes unasked:
    // the ready offers are now the SPR list.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    assert_eq!(positions(&old), [1, 2, 3, 4, 8, 9]);

    arrives_unasked(&port, &mut s, &spr_block());

    assert!(s.pd_ready(), "a usable list keeps PD ready");
    let ready = s.pd_offers().expect("ready offers").to_vec();
    assert_eq!(positions(&ready), [1, 2, 3, 4], "{ready:?}");
    assert_eq!(ready, pd::offers(s.capabilities()));
}

#[test]
fn a_position_only_the_unasked_list_lacks_is_refused_before_anything_is_written() {
    // AC-3B70SA.1: regression. The caller still holds the EPR list; the
    // charger dropped EPR unasked. Position 8 is refused, nothing written.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    arrives_unasked(&port, &mut s, &spr_block());

    let before = port.len();
    let req = fixed(8);
    let err = pd::request(&mut s, &old, &req, confirmed(&old, &req))
        .expect_err("position 8 is not offered now");
    assert!(is_refused(&err, 8), "{err:?}");
    assert_eq!(port.writes_after(before), Vec::<String>::new());
}

#[test]
fn a_spr_position_the_shorter_unasked_list_lacks_is_refused_before_anything_is_written() {
    // AC-3B70SA.1: the charger lowers its offer to 5 V / 9 V unasked; the
    // 15 V position the negotiation offered is refused, nothing written.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    assert!(old.iter().any(|o| o.position == 3), "{old:?}");
    arrives_unasked(&port, &mut s, &short_block());

    let before = port.len();
    let req = fixed(3);
    let err = pd::request(&mut s, &old, &req, confirmed(&old, &req))
        .expect_err("position 3 is not offered now");
    assert!(is_refused(&err, 3), "{err:?}");
    assert_eq!(port.writes_after(before), Vec::<String>::new());
    assert_eq!(positions(s.pd_offers().expect("ready offers")), [1, 2]);
}

#[test]
fn an_unasked_list_inside_another_reply_replaces_the_ready_offers() {
    // AC-3B70SA.1: the SPR list comes while the 9 V request's reply is
    // read. The next request for position 8 is refused, nothing written.
    let port = charger(&[epr_entry_reply()], Some(req2_reply_with(&spr_block())));
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    let req = fixed(2);
    pd::request(&mut s, &old, &req, confirmed(&old, &req)).expect("9 V");
    assert_eq!(
        positions(s.pd_offers().expect("ready offers")),
        [1, 2, 3, 4]
    );

    let before = port.len();
    let req = fixed(8);
    let err = pd::request(&mut s, &old, &req, confirmed(&old, &req))
        .expect_err("position 8 is not offered now");
    assert!(is_refused(&err, 8), "{err:?}");
    assert_eq!(port.writes_after(before), Vec::<String>::new());
}

#[test]
fn a_position_the_unasked_list_still_offers_goes_out_without_a_new_entry() {
    // AC-3B70SA.1: the new list still holds position 2: sent as it is.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    arrives_unasked(&port, &mut s, &spr_block());

    let before = port.len();
    let req = fixed(2);
    pd::request(&mut s, &old, &req, confirmed(&old, &req)).expect("9 V");
    assert_eq!(port.writes_after(before), ["pd req=2"]);
}

// ---- AC-3B70SA.2 ----

#[test]
fn an_unasked_list_after_open_updates_the_capabilities_only() {
    // AC-3B70SA.2: nothing negotiated since `pdm open`.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    arrives_unasked(&port, &mut s, &spr_block());

    assert_eq!(positions(&pd::offers(s.capabilities())), [1, 2, 3, 4]);
    assert!(!s.pd_ready());
    assert_eq!(s.pd_offers(), None);
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn an_unasked_list_after_an_entry_without_offers_leaves_pd_not_ready() {
    // AC-3B70SA.2: `entry pd` was written (PD entered) but got no offers;
    // a list arriving afterwards is no negotiation.
    let port = charger(&[NO_OFFERS.to_vec(), epr_entry_reply()], None);
    let mut s = open(&port);
    pd::enter(&mut s).expect_err("no offers came");
    arrives_unasked(&port, &mut s, &spr_block());

    assert_eq!(positions(&pd::offers(s.capabilities())), [1, 2, 3, 4]);
    assert!(!s.pd_ready());
    assert_eq!(s.pd_offers(), None);
    assert!(enters_pd(&port, &mut s), "{:?}", port.writes());
}

#[test]
fn an_unasked_list_after_reset_leaves_pd_not_ready() {
    // AC-3B70SA.2: PD was ready, `reset` cleared it; a list read afterwards
    // updates the capabilities only.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    s.send(&Command::Reset).expect("reset");
    s.wait_for(&["ok"], Duration::from_millis(1000))
        .expect("ok");
    arrives_unasked(&port, &mut s, &spr_block());

    assert_eq!(positions(&pd::offers(s.capabilities())), [1, 2, 3, 4]);
    assert!(!s.pd_ready());
    assert_eq!(s.pd_offers(), None);
}

#[test]
fn an_unasked_list_after_another_protocols_entry_leaves_pd_not_ready() {
    // AC-3B70SA.2: QC entered after PD; the list does not make PD ready.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    legacy::enter(&mut s, LegacyProtocol::Qc).expect("entry qc");
    arrives_unasked(&port, &mut s, &spr_block());

    assert_eq!(positions(&pd::offers(s.capabilities())), [1, 2, 3, 4]);
    assert!(!s.pd_ready());
    assert_eq!(s.pd_offers(), None);
}

// ---- AC-3B70SA.3 ----

#[test]
fn an_unasked_list_with_no_usable_offer_leaves_pd_not_ready() {
    // AC-3B70SA.3: ready on the EPR list, then an empty list comes unasked.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    pd::enter(&mut s).expect("entry pd");
    arrives_unasked(&port, &mut s, &empty_block());

    assert!(pd::offers(s.capabilities()).is_empty());
    assert!(!s.pd_ready());
    assert_eq!(s.pd_offers(), None);
}

#[test]
fn the_request_after_an_unasked_empty_list_enters_pd_again_first() {
    // AC-3B70SA.3: the next request writes `entry pd` before `pd req=`.
    let port = charger(&[epr_entry_reply()], None);
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    arrives_unasked(&port, &mut s, &empty_block());

    let before = port.len();
    let req = fixed(2);
    let out = pd::request(&mut s, &old, &req, confirmed(&old, &req))
        .expect("the request goes out after a fresh entry");
    assert_eq!(port.writes_after(before), ["entry pd", "pd req=2"]);
    assert_eq!(out.requested.voltage_mv, 9000);
}

#[test]
fn the_request_after_an_empty_list_inside_another_reply_enters_pd_again_first() {
    // AC-3B70SA.3: the empty list comes inside the 9 V request's reply; the
    // next request enters PD before it writes anything else.
    let port = charger(&[epr_entry_reply()], Some(req2_reply_with(&empty_block())));
    let mut s = open(&port);
    let old = pd::enter(&mut s).expect("entry pd").offers;
    let req = fixed(2);
    pd::request(&mut s, &old, &req, confirmed(&old, &req)).expect("9 V");
    assert!(!s.pd_ready());

    let before = port.len();
    let req = fixed(1);
    pd::request(&mut s, &old, &req, None).expect("5 V after a fresh entry");
    assert_eq!(port.writes_after(before), ["entry pd", "pd req=1"]);
}
