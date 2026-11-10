//! FR AHTZW4: a request after a failed PD entry went out against old offers.
//!
//! The engine path the desktop and the browser share. After a request whose
//! PD entries got no offers (FR AHTZW1 gives up after the second), the next
//! press must enter PD again and be checked against what the charger offers
//! then, never sent straight against the offers from before.
//!
//! - AC-AHTZW4.1: the next press enters PD again before it writes anything.
//! - AC-AHTZW4.3: a position the new offers lack is refused, nothing written.
//! - AC-AHTZW4.4: a written `entry pd` still counts as possibly in PD.
//!
//! The port is scripted per command: the n-th write of a command gets the n-th
//! reply listed for it (the last one repeats). Time is a `SimClock`.

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
fn four_offers() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(hex(
        "70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a",
    ));
    b.extend(b"ready:5191mV,0mA\n");
    b
}

/// `entry pd` when the charger now offers only 5 V and 9 V.
fn two_offers() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(hex(
        "70646f3a 32 0b 2c918109 2cd10200 00000000 00000000
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
/// first one is the PD listing's).
fn charger(entry: &[&[u8]]) -> Scripted {
    Scripted::default()
        .on("pdm open", &[OPEN_REPLY])
        .on("pdm close", &[b"ok\n"])
        .on("reset", &[b"ok\n"])
        .on("entry pd", entry)
        .on("entry qc", &[b"ok\nready\n"])
        .on("pdm parm", &[b"type=0,em=2,sink=0\n"])
        .on("pd req=1", &[b"req:5000mV,3000mA\nready:5180mV,0mA\n"])
        .on("pd req=2", &[b"req:9000mV,3000mA\nready:8554mV,0mA\n"])
        .on("pd req=3", &[b"req:15000mV,3000mA\nready:14900mV,0mA\n"])
}

fn pd(position: u8) -> TriggerRequest {
    TriggerRequest::Pd {
        position,
        voltage_mv: None,
        current_ma: None,
    }
}

fn count(writes: &[String], cmd: &str) -> usize {
    writes.iter().filter(|w| *w == cmd).count()
}

/// Open, list PD, list QC, then press PD 9 V while neither re-entry gets
/// offers (FR AHTZW1: two tries, then the timeout). Returns how many lines
/// were written before the next press.
fn after_a_request_without_offers(port: &Scripted) -> (Session<Scripted, SimClock>, usize) {
    let mut s =
        Session::open(port.clone(), SimClock::new(1_760_000_000_000)).expect("the session opens");
    let section = s.list(TriggerProtocol::Pd).expect("list pd");
    assert!(section.supported, "{section:?}");
    s.list(TriggerProtocol::Qc).expect("list qc");
    let before = port.writes().len();
    let err = s
        .request(pd(2), Confirm::Yes)
        .expect_err("no offers came: the request fails");
    assert_eq!(err.kind, TriggerErrorKind::Timeout, "{err:?}");
    let w: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(w, ["entry pd", "entry pd"], "{w:?}");
    let before = port.writes().len();
    (s, before)
}

// ---- AC-AHTZW4.1 ----

#[test]
fn the_press_after_a_request_without_offers_enters_pd_again_first() {
    // AC-AHTZW4.1: regression. Listing answers, both entries of the first
    // press get nothing, the second press's entry answers.
    let port = charger(&[&four_offers(), NO_OFFERS, NO_OFFERS, &four_offers()]);
    let (mut s, before) = after_a_request_without_offers(&port);

    let reply = s
        .request(pd(2), Confirm::Yes)
        .expect("the request goes out after a fresh entry");
    assert_eq!(reply.requested_mv, Some(9000), "{reply:?}");
    let w: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(w, ["entry pd", "pd req=2"], "{w:?}");
}

#[test]
fn the_press_after_a_request_without_offers_writes_no_request_while_offers_stay_away() {
    // AC-AHTZW4.1: the charger still sends no offers: the press fails, and
    // no `pd req=` was written.
    let port = charger(&[&four_offers(), NO_OFFERS]);
    let (mut s, before) = after_a_request_without_offers(&port);

    let err = s.request(pd(2), Confirm::Yes).expect_err("still no offers");
    assert_eq!(err.kind, TriggerErrorKind::Timeout, "{err:?}");
    let w: Vec<String> = port.writes()[before..].to_vec();
    assert!(
        !w.iter().any(|c| c.starts_with("pd req=")),
        "no request without fresh offers: {w:?}"
    );
    assert!(w.iter().any(|c| c == "entry pd"), "{w:?}");
}

// ---- AC-AHTZW4.3 ----

#[test]
fn a_position_the_new_offers_lack_is_refused_with_nothing_requested() {
    // AC-AHTZW4.3: the panel shows 15 V (position 3); after the failed
    // entries the charger offers only 5 V and 9 V. The press re-enters and
    // refuses 15 V; no `pd req=3` is written.
    let port = charger(&[&four_offers(), NO_OFFERS, NO_OFFERS, &two_offers()]);
    let (mut s, before) = after_a_request_without_offers(&port);

    s.request(pd(3), Confirm::Yes)
        .expect_err("15 V is not offered now");
    let w: Vec<String> = port.writes()[before..].to_vec();
    assert_eq!(count(&w, "entry pd"), 1, "{w:?}");
    assert!(
        !w.iter().any(|c| c.starts_with("pd req=")),
        "nothing requested: {w:?}"
    );
}

// ---- AC-AHTZW4.4 ----

#[test]
fn a_written_entry_without_offers_still_counts_as_pd_entered() {
    // AC-AHTZW4.4: the meter may be in PD although no offers came.
    let port = charger(&[&four_offers(), NO_OFFERS]);
    let (mut s, _) = after_a_request_without_offers(&port);
    assert!(s.pd_entered());
}
