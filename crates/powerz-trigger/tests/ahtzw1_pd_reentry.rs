//! FR AHTZW1: the first PD request after Detect sometimes timed out.
//!
//! Detect (and any legacy listing) leaves another protocol entered last, so a
//! PD request first enters PD again and waits for the charger's offers. The
//! charger is sometimes slower than that wait straight after the automatic
//! list. The engine now sends the entry once more before the request fails;
//! the re-entry only ever asks for 5 V, while the request itself is never sent
//! twice.
//!
//! - AC-AHTZW1.1: the first re-entry gets no offers in time, the second does:
//!   `entry pd` goes out once more and the request goes out after it.
//! - AC-AHTZW1.2: both re-entries get no offers: the timeout error, and
//!   nothing above 5 V was written.
//! - AC-AHTZW1.3: a request whose own reply (`req:`) does not come is not sent
//!   again.
//!
//! The port is scripted per command: the n-th write of a command gets the n-th
//! reply listed for it (the last one repeats). Time is a `SimClock`, so the
//! entry's wait (`pd::ENTRY_TIMEOUT`) passes instantly.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use powerz_sim::SimClock;
use powerz_trigger::{Confirm, Engine, Session, TriggerErrorKind, TriggerProtocol, TriggerRequest};

const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// `entry pd` on a 140 W charger (5 / 9 / 15 / 20 V), SPR list only. [device]
fn offers_reply() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(hex(
        "70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a",
    ));
    b.extend(b"ready:5191mV,0mA\n");
    b
}

/// `entry pd` taken, but no offers before the wait ends.
const NO_OFFERS: &[u8] = b"ok\n";

#[derive(Default)]
struct Script {
    replies: HashMap<String, Vec<Vec<u8>>>,
    count: HashMap<String, usize>,
    writes: Vec<String>,
    pending: VecDeque<u8>,
}

/// A serial port answering each command with its next scripted reply.
#[derive(Clone, Default)]
struct Scripted(Arc<Mutex<Script>>);

impl Scripted {
    fn on(self, cmd: &str, replies: &[&[u8]]) -> Scripted {
        self.0
            .lock()
            .unwrap()
            .replies
            .insert(cmd.to_owned(), replies.iter().map(|r| r.to_vec()).collect());
        self
    }

    fn writes(&self) -> Vec<String> {
        self.0.lock().unwrap().writes.clone()
    }
}

impl Read for Scripted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut s = self.0.lock().unwrap();
        if s.pending.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"));
        }
        let n = buf.len().min(s.pending.len());
        for b in buf.iter_mut().take(n) {
            *b = s.pending.pop_front().unwrap();
        }
        Ok(n)
    }
}

impl Write for Scripted {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut s = self.0.lock().unwrap();
        let cmd = String::from_utf8_lossy(buf).trim().to_owned();
        if cmd.is_empty() {
            return Ok(buf.len());
        }
        s.writes.push(cmd.clone());
        let n = {
            let c = s.count.entry(cmd.clone()).or_default();
            *c += 1;
            *c - 1
        };
        let reply = s
            .replies
            .get(&cmd)
            .and_then(|list| list.get(n).or_else(|| list.last()).cloned());
        if let Some(r) = reply {
            s.pending.extend(r);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The charger behind the meter: `entry pd` answers `entry` in turn (the
/// first one is the PD listing's), `pd req=2` answers `req`.
fn charger(entry: &[&[u8]], req: &[u8]) -> Scripted {
    Scripted::default()
        .on("pdm open", &[OPEN_REPLY])
        .on("pdm close", &[b"ok\n"])
        .on("reset", &[b"ok\n"])
        .on("entry pd", entry)
        .on("entry qc", &[b"ok\nready\n"])
        .on("pdm parm", &[b"type=0,em=2,sink=0\n"])
        .on("pd req=1", &[b"req:5000mV,3000mA\nready:5180mV,0mA\n"])
        .on("pd req=2", &[req])
}

const REQ_9V: &[u8] = b"req:9000mV,3000mA\nready:8554mV,0mA\n";

fn pd(position: u8) -> TriggerRequest {
    TriggerRequest::Pd {
        position,
        voltage_mv: None,
        current_ma: None,
    }
}

/// Open, list PD (the offers the panel shows), then list QC so another
/// protocol was entered last, as after Detect. Returns how many lines were
/// written before the request.
fn after_a_legacy_listing(port: &Scripted) -> (Session<Scripted, SimClock>, usize) {
    let mut s =
        Session::open(port.clone(), SimClock::new(1_760_000_000_000)).expect("the session opens");
    let pd_section = s.list(TriggerProtocol::Pd).expect("list pd");
    assert!(pd_section.supported, "{pd_section:?}");
    s.list(TriggerProtocol::Qc).expect("list qc");
    assert_eq!(port.writes().last().map(String::as_str), Some("entry qc"));
    let before = port.writes().len();
    (s, before)
}

fn count(writes: &[String], cmd: &str) -> usize {
    writes.iter().filter(|w| *w == cmd).count()
}

/// A `pd req=` that asks for more than 5 V (anything but position 1).
fn above_5v(w: &str) -> bool {
    w.starts_with("pd req=") && w != "pd req=1"
}

// ---- AC-AHTZW1.1 ----

#[test]
fn a_re_entry_without_offers_is_sent_once_more_and_the_request_follows() {
    // AC-AHTZW1.1: regression. The listing's entry answers; the request's
    // first re-entry gets no offers in time; the second one does.
    let port = charger(&[&offers_reply(), NO_OFFERS, &offers_reply()], REQ_9V);
    let (mut s, before) = after_a_legacy_listing(&port);

    let reply = s
        .request(pd(2), Confirm::Yes)
        .expect("the request goes out once the offers arrive");
    assert_eq!(reply.requested_mv, Some(9000), "{reply:?}");
    assert_eq!(reply.delivered_mv, Some(8554), "{reply:?}");

    let after: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(
        count(&after, "entry pd"),
        2,
        "the entry is sent once more, no more: {after:?}"
    );
    assert_eq!(count(&after, "pd req=2"), 1, "{after:?}");
    let last_entry = after.iter().rposition(|w| w == "entry pd").unwrap();
    let req = after.iter().position(|w| w == "pd req=2").unwrap();
    assert!(
        req > last_entry,
        "the request goes out after the second entry: {after:?}"
    );
    // Nothing above 5 V before the offers arrived.
    assert!(
        !after[..last_entry].iter().any(|w| above_5v(w)),
        "{after:?}"
    );
}

#[test]
fn a_re_entry_that_answers_at_once_is_not_repeated() {
    // AC-AHTZW1.1: the extra entry is only for a re-entry that timed out.
    let port = charger(&[&offers_reply()], REQ_9V);
    let (mut s, before) = after_a_legacy_listing(&port);
    s.request(pd(2), Confirm::Yes).expect("request");
    let after: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(after, vec!["entry pd".to_owned(), "pd req=2".to_owned()]);
}

// ---- AC-AHTZW1.2 ----

#[test]
fn two_re_entries_without_offers_fail_with_the_timeout_and_nothing_above_5v() {
    // AC-AHTZW1.2: the listing's entry answers, then no entry gets offers.
    let port = charger(&[&offers_reply(), NO_OFFERS], REQ_9V);
    let (mut s, before) = after_a_legacy_listing(&port);

    let err = s
        .request(pd(2), Confirm::Yes)
        .expect_err("no offers came: the request fails");
    assert_eq!(err.kind, TriggerErrorKind::Timeout, "{err:?}");
    assert_eq!(err.message, "The meter did not answer in time.", "{err:?}");

    let after: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(
        count(&after, "entry pd"),
        2,
        "the entry is tried once more before the failure, no more: {after:?}"
    );
    assert!(
        !after.iter().any(|w| above_5v(w)),
        "nothing above 5 V was sent: {after:?}"
    );
}

// ---- AC-AHTZW1.3 ----

#[test]
fn a_request_without_its_own_reply_is_not_sent_again_after_a_re_entry() {
    // AC-AHTZW1.3: the re-entry answers, `pd req=2` gets no `req:`.
    let port = charger(&[&offers_reply()], b"");
    let (mut s, before) = after_a_legacy_listing(&port);

    let err = s
        .request(pd(2), Confirm::Yes)
        .expect_err("the request's reply never came");
    assert_eq!(err.kind, TriggerErrorKind::Timeout, "{err:?}");
    let after: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(count(&after, "pd req=2"), 1, "sent once: {after:?}");
    assert_eq!(count(&after, "entry pd"), 1, "{after:?}");
}

#[test]
fn a_request_without_its_own_reply_is_not_sent_again_after_a_retried_entry() {
    // AC-AHTZW1.3 with AC .1: the entry needed its second try; the request's
    // own missing reply still sends nothing again.
    let port = charger(&[&offers_reply(), NO_OFFERS, &offers_reply()], b"");
    let (mut s, before) = after_a_legacy_listing(&port);

    let err = s
        .request(pd(2), Confirm::Yes)
        .expect_err("the request's reply never came");
    assert_eq!(err.kind, TriggerErrorKind::Timeout, "{err:?}");
    let after: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(count(&after, "entry pd"), 2, "{after:?}");
    assert_eq!(count(&after, "pd req=2"), 1, "sent once: {after:?}");
}

#[test]
fn a_request_in_pd_without_its_own_reply_is_not_sent_again() {
    // AC-AHTZW1.3: PD entered last (no re-entry at all).
    let port = charger(&[&offers_reply()], b"");
    let mut s =
        Session::open(port.clone(), SimClock::new(1_760_000_000_000)).expect("the session opens");
    s.list(TriggerProtocol::Pd).expect("list pd");
    let before = port.writes().len();

    let err = s
        .request(pd(2), Confirm::Yes)
        .expect_err("the request's reply never came");
    assert_eq!(err.kind, TriggerErrorKind::Timeout, "{err:?}");
    let after: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(after, vec!["pd req=2".to_owned()]);
}
