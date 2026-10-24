//! PDM session over a scripted fake port (FR PR41FT).
//!
//! The fake port answers exact command bytes with scripted reply bytes taken
//! from the device-confirmed session-2 replies (rewritten here as our own
//! fixtures). A fake clock shares one time line with the port so pacing and
//! timeouts are checked without sleeping.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::session::{IDLE_POLL, MIN_COMMAND_GAP};
use powerz_protocol::pdm::{
    BlockTag, CapBlock, Command, Error, HighVoltageConfirmation, PdmSession, Protocol, ReplyItem,
    Risk,
};
use powerz_protocol::session::Clock;

const T0: u64 = 1_790_000_000_000;

// ---------------------------------------------------------------- fixtures

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// SPR `pdo:` block of the 140 W charger, with its `\n`.
fn spr_block() -> Vec<u8> {
    hex("70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a")
}

/// EPR `pdo:` block of the 140 W charger, with its `\n`.
fn epr_block() -> Vec<u8> {
    hex("70646f3a 39 0b 2c918109 2cd10200 2cb10400 f4410600
         00000000 00000000 00000000 f4c10800 8c9630d2 00000000 00000000 0a")
}

fn entry_pd_reply() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(spr_block());
    b.extend(b"ready:5191mV,0mA\n");
    b.extend(epr_block());
    b.extend(b"ready:5162mV,0mA\n");
    b
}

const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";
const BUSY_REPLY: &[u8] = b"pdm mode entry\nbusy\n";

// ---------------------------------------------------------------- fakes

/// Clock sharing the port's time line; `sleep` advances it.
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
    /// Exact command bytes → successive replies (the last one repeats).
    script: HashMap<Vec<u8>, VecDeque<Vec<u8>>>,
    /// (time ms, bytes) per `write` call.
    writes: Vec<(u64, Vec<u8>)>,
    pending: VecDeque<u8>,
    fail_writes: bool,
    released: bool,
    /// Once `pending` is empty, every read returns these bytes again and
    /// 1 ms passes: a device that never stops talking.
    noise: Option<Vec<u8>>,
}

/// Scripted serial port. Clones share state; the session owns one clone.
#[derive(Clone)]
struct FakePort {
    state: Rc<RefCell<State>>,
    now: Rc<Cell<u64>>,
    owner: bool,
}

impl FakePort {
    fn new(now: &Rc<Cell<u64>>) -> FakePort {
        FakePort {
            state: Rc::default(),
            now: now.clone(),
            owner: false,
        }
    }
    /// The handle given to the session: its drop marks the port released.
    fn handle(&self) -> FakePort {
        FakePort {
            state: self.state.clone(),
            now: self.now.clone(),
            owner: true,
        }
    }
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
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .collect()
    }
    fn write_times(&self) -> Vec<u64> {
        self.state.borrow().writes.iter().map(|(t, _)| *t).collect()
    }
    fn released(&self) -> bool {
        self.state.borrow().released
    }
    fn push_unsolicited(&self, bytes: &[u8]) {
        self.state.borrow_mut().pending.extend(bytes);
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
            let Some(noise) = s.noise.clone() else {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
            };
            self.now.set(self.now.get() + 1);
            s.pending.extend(noise);
        }
        let n = buf.len().min(s.pending.len()).min(7); // small USB-like chunks
        for b in buf.iter_mut().take(n) {
            *b = s.pending.pop_front().unwrap();
        }
        Ok(n)
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        if s.fail_writes {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "unplugged"));
        }
        let now = self.now.get();
        s.writes.push((now, buf.to_vec()));
        let reply = s.script.get_mut(buf).and_then(|q| {
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

impl Drop for FakePort {
    fn drop(&mut self) {
        if self.owner {
            self.state.borrow_mut().released = true;
        }
    }
}

fn time() -> Rc<Cell<u64>> {
    Rc::new(Cell::new(T0))
}

/// A port scripted like the 140 W EPR charger behind the meter.
fn charger_port(now: &Rc<Cell<u64>>) -> FakePort {
    FakePort::new(now)
        .on("pdm open", OPEN_REPLY)
        .on("pdm close", b"ok\n")
        .on("reset", b"ok\n")
        .on("pdm parm", b"type=0,em=2,sink=0\n")
        .on("entry pd", &entry_pd_reply())
        .on("pd req=1", b"req:5000mV,3000mA\nready:5180mV,0mA\n")
        .on("pd req=2", b"req:9000mV,3000mA\nready:8554mV,0mA\n")
        .on("pd req=4", b"req:20000mV,5000mA\nready:19870mV,0mA\n")
}

type Session = PdmSession<FakePort, FakeClock>;

fn open(port: &FakePort, now: &Rc<Cell<u64>>) -> Session {
    PdmSession::open_with_clock(port.handle(), FakeClock(now.clone())).expect("open")
}

/// Open and run `entry pd` until both ready lines arrived.
fn open_in_pd(port: &FakePort, now: &Rc<Cell<u64>>) -> Session {
    let mut s = open(port, now);
    s.send(&Command::Entry(Protocol::Pd)).expect("entry pd");
    s.wait_for(&["ready"], Duration::from_secs(1))
        .expect("first ready");
    s.wait_for(&["ready"], Duration::from_secs(1))
        .expect("second ready");
    s
}

fn pd_req(position: u8) -> Command {
    Command::PdRequest {
        position,
        voltage_mv: None,
        current_ma: None,
    }
}

/// Test paths carry `pdm` so `cargo test pdm` selects them.
mod pdm_session {
    use super::*;

    // ---- AC-PR41FT.1: open, busy/reset, close on exit / error / drop ----

    #[test]
    fn open_sends_pdm_open_and_needs_mode_entry() {
        let now = time();
        let port = charger_port(&now);
        let _s = open(&port, &now);
        assert_eq!(port.writes(), vec!["pdm open"]);
    }

    #[test]
    fn open_on_a_busy_meter_sends_reset_first() {
        let now = time();
        let port = charger_port(&now);
        // The first `pdm open` finds the meter busy.
        port.state
            .borrow_mut()
            .script
            .insert(b"pdm open".to_vec(), VecDeque::from([BUSY_REPLY.to_vec()]));
        let mut s = open(&port, &now);
        assert_eq!(port.writes(), vec!["pdm open", "reset"]);
        s.send(&Command::PdmParm).unwrap();
        assert_eq!(
            s.wait_for(&["type="], Duration::from_secs(1)).unwrap(),
            "type=0,em=2,sink=0"
        );
    }

    #[test]
    fn busy_before_mode_entry_still_sends_reset() {
        // The vendor app checks one reply chunk for both strings, so their
        // order is not fixed. [static]
        let now = time();
        let port = charger_port(&now);
        port.state.borrow_mut().script.insert(
            b"pdm open".to_vec(),
            VecDeque::from([b"busy\npdm mode entry\n".to_vec()]),
        );
        let _s = open(&port, &now);
        assert_eq!(port.writes(), vec!["pdm open", "reset"]);
    }

    #[test]
    fn endless_unrelated_lines_still_time_out() {
        let now = time();
        let port = FakePort::new(&now).on("pdm close", b"ok\n");
        port.state.borrow_mut().noise = Some(b"chatter\n".to_vec());
        let err = PdmSession::open_with_clock(port.handle(), FakeClock(now.clone())).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
        assert!(now.get() - T0 < 3_000, "bounded: {} ms", now.get() - T0);
    }

    #[test]
    fn endless_busy_lines_still_time_out() {
        let now = time();
        let port = FakePort::new(&now).on("pdm close", b"ok\n");
        port.state.borrow_mut().noise = Some(b"busy\n".to_vec());
        let err = PdmSession::open_with_clock(port.handle(), FakeClock(now.clone())).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
        assert!(now.get() - T0 < 3_000, "bounded: {} ms", now.get() - T0);
    }

    #[test]
    fn drop_on_a_silent_meter_waits_only_briefly() {
        // A destructor must not stall a UI thread for a full reply timeout.
        let now = time();
        let port = FakePort::new(&now).on("pdm open", OPEN_REPLY);
        let s = open(&port, &now);
        let before = now.get();
        drop(s);
        assert_eq!(port.writes().last().map(String::as_str), Some("pdm close"));
        assert!(
            now.get() - before <= 200,
            "drop took {} ms",
            now.get() - before
        );
    }

    #[test]
    fn open_without_mode_entry_times_out_and_closes() {
        let now = time();
        let port = FakePort::new(&now).on("pdm close", b"ok\n");
        let err = PdmSession::open_with_clock(port.handle(), FakeClock(now.clone())).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
        assert_eq!(port.writes(), vec!["pdm open", "pdm close"]);
        assert!(port.released());
    }

    #[test]
    fn busy_open_whose_reset_fails_closes_before_release() {
        let now = time();
        let port = FakePort::new(&now)
            .on("pdm open", BUSY_REPLY)
            .on("pdm close", b"ok\n");
        let err = PdmSession::open_with_clock(port.handle(), FakeClock(now.clone())).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
        assert_eq!(port.writes(), vec!["pdm open", "reset", "pdm close"]);
        assert!(port.released());
    }

    #[test]
    fn close_sends_pdm_close_once_then_releases_the_port() {
        let now = time();
        let port = charger_port(&now);
        let s = open(&port, &now);
        s.close().expect("close");
        assert_eq!(port.writes(), vec!["pdm open", "pdm close"]);
        assert!(port.released());
    }

    #[test]
    fn drop_sends_pdm_close() {
        let now = time();
        let port = charger_port(&now);
        let s = open(&port, &now);
        drop(s);
        assert_eq!(port.writes(), vec!["pdm open", "pdm close"]);
        assert!(port.released());
    }

    #[test]
    fn drop_after_an_error_still_sends_pdm_close() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        s.send(&Command::Entry(Protocol::Qc)).unwrap(); // not scripted: no reply
        let err = s.wait_for(&["ok"], Duration::from_millis(200)).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
        drop(s);
        assert_eq!(port.writes().last().map(String::as_str), Some("pdm close"));
        assert!(port.released());
    }

    #[test]
    fn drop_on_an_unplugged_port_does_not_panic() {
        let now = time();
        let port = charger_port(&now);
        let s = open(&port, &now);
        port.state.borrow_mut().fail_writes = true;
        drop(s);
        assert!(port.released());
    }

    // ---- AC-PR41FT.2: one write per command, ≥ 60 ms apart ----

    #[test]
    fn commands_are_single_writes_at_least_60_ms_apart() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        s.send(&Command::PdmParm).unwrap();
        s.send(&Command::PdPdo).unwrap();
        s.send(&Command::Entry(Protocol::Pd)).unwrap();
        assert_eq!(
            port.writes(),
            vec!["pdm open", "pdm parm", "pd pdo", "entry pd"]
        );
        let times = port.write_times();
        for pair in times.windows(2) {
            assert!(
                pair[1] - pair[0] >= MIN_COMMAND_GAP.as_millis() as u64,
                "gap {} ms in {times:?}",
                pair[1] - pair[0]
            );
        }
    }

    #[test]
    fn no_extra_wait_when_the_gap_already_passed() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        now.set(now.get() + 500);
        let before = now.get();
        s.send(&Command::PdmParm).unwrap();
        assert_eq!(port.write_times().last(), Some(&before));
    }

    // ---- AC-PR41FT.2: lines with prefix matching, typed timeout ----

    #[test]
    fn wait_for_matches_by_prefix_and_skips_other_lines() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        s.send(&Command::Entry(Protocol::Pd)).unwrap();
        let ready = s
            .wait_for(&["ready", "error"], Duration::from_secs(1))
            .unwrap();
        assert_eq!(ready, "ready:5191mV,0mA");
    }

    #[test]
    fn reply_timeout_is_a_typed_error_on_the_clock() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        s.send(&Command::Raw("bogus".into()))
            .expect_err("raw text needs a confirmation");
        let conf = HighVoltageConfirmation::for_command(&Command::Raw("bogus".into()));
        s.send_confirmed(&Command::Raw("bogus".into()), conf)
            .unwrap();
        let start = now.get();
        let err = s.wait_for(&["ok"], Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
        let waited = now.get() - start;
        assert!(
            (300..300 + 10 * IDLE_POLL.as_millis() as u64).contains(&waited),
            "waited {waited} ms on the clock"
        );
    }

    #[test]
    fn next_item_returns_lines_and_blocks_in_order() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        s.send(&Command::Entry(Protocol::Pd)).unwrap();
        let t = Duration::from_secs(1);
        // `pdm open`'s ver line may still be queued: skip to `ok`.
        s.wait_for(&["ok"], t).unwrap();
        assert_eq!(
            s.next_item(t).unwrap(),
            ReplyItem::Line("cc1 attach".into())
        );
        match s.next_item(t).unwrap() {
            ReplyItem::Block(b) => assert_eq!(b.tag(), BlockTag::Pdo),
            other => panic!("expected the SPR block, got {other:?}"),
        }
        assert_eq!(
            s.next_item(t).unwrap(),
            ReplyItem::Line("ready:5191mV,0mA".into())
        );
    }

    #[test]
    fn interleaved_binary_frames_are_skipped() {
        let now = time();
        let mut noisy = vec![0x02, 0x12, 0x05, 0x00];
        noisy.extend([0x70u8; 20]); // ADCNEW body that looks like "ppp…"
        let mut reply = noisy.clone();
        reply.extend(b"type=0,em=2,sink=0\n");
        let port = charger_port(&now).on("pdm parm", &reply);
        port.state
            .borrow_mut()
            .script
            .insert(b"pdm parm".to_vec(), VecDeque::from([reply]));
        let mut s = open(&port, &now);
        s.send(&Command::PdmParm).unwrap();
        assert_eq!(
            s.wait_for(&["type="], Duration::from_secs(1)).unwrap(),
            "type=0,em=2,sink=0"
        );
    }

    // ---- AC-PR41FT.3: unprompted blocks update the capabilities ----

    #[test]
    fn entry_pd_blocks_update_capabilities_without_being_asked() {
        let now = time();
        let port = charger_port(&now);
        let s = open_in_pd(&port, &now);
        let pd = s.capabilities().pd.as_ref().expect("pdo seen");
        assert_eq!(pd.marker, b'9', "the EPR list replaced the SPR one");
        let positions: Vec<u8> = pd.offers().iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, vec![1, 2, 3, 4, 8, 9]);
    }

    #[test]
    fn a_block_pushed_while_idle_is_picked_up_by_the_next_read() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        let mut pushed = spr_block();
        pushed.extend(b"ready:5191mV,0mA\n");
        port.push_unsolicited(&pushed);
        s.wait_for(&["ready"], Duration::from_secs(1)).unwrap();
        assert_eq!(s.capabilities().pd.as_ref().map(|p| p.marker), Some(b'4'));
    }

    #[test]
    fn malformed_block_is_reported_as_a_typed_error() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        let mut bad = b"pdo:4".to_vec();
        bad.push(0x30);
        bad.extend(b"\n");
        port.push_unsolicited(&bad);
        // A leftover `ver1.0` line from `pdm open` may come first.
        let mut seen = Vec::new();
        for _ in 0..3 {
            match s.next_item(Duration::from_secs(1)) {
                Ok(item) => seen.push(item),
                Err(err) => {
                    assert!(
                        matches!(
                            err,
                            Error::Malformed {
                                tag: BlockTag::Pdo,
                                ..
                            }
                        ),
                        "{err:?}"
                    );
                    return;
                }
            }
        }
        panic!("no Malformed error, got {seen:?}");
    }

    // ---- AC-PR41FT.5: above 5 V only with a single-command confirmation ----

    #[test]
    fn send_refuses_above_5v_without_writing() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open_in_pd(&port, &now);
        let before = port.writes();
        assert_eq!(s.risk(&pd_req(2)), Risk::Above5V);
        let err = s.send(&pd_req(2)).unwrap_err();
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        assert_eq!(port.writes(), before, "nothing written");
    }

    #[test]
    fn send_allows_at_most_5v_without_confirmation() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open_in_pd(&port, &now);
        s.send(&pd_req(1)).unwrap();
        assert_eq!(port.writes().last().map(String::as_str), Some("pd req=1"));
    }

    #[test]
    fn send_confirmed_sends_the_confirmed_command() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open_in_pd(&port, &now);
        let conf = HighVoltageConfirmation::for_command(&pd_req(2));
        s.send_confirmed(&pd_req(2), conf).unwrap();
        assert_eq!(port.writes().last().map(String::as_str), Some("pd req=2"));
        assert_eq!(
            s.wait_for(&["ready"], Duration::from_secs(1)).unwrap(),
            "ready:8554mV,0mA"
        );
    }

    #[test]
    fn a_confirmation_covers_only_its_own_command() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open_in_pd(&port, &now);
        let before = port.writes();
        let conf = HighVoltageConfirmation::for_command(&pd_req(2));
        let err = s.send_confirmed(&pd_req(4), conf).unwrap_err();
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        assert_eq!(port.writes(), before);
    }

    #[test]
    fn unknown_text_needs_a_confirmation() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        let err = s.send(&Command::parse("pd req=abc")).unwrap_err();
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        assert_eq!(port.writes(), vec!["pdm open"]);
    }

    #[test]
    fn guard_refuses_text_whose_first_byte_is_a_forbidden_frame_type() {
        let now = time();
        let port = charger_port(&now);
        let mut s = open(&port, &now);
        let cmd = Command::Raw("Hello".into()); // 'H' = 0x48, flash write
        let conf = HighVoltageConfirmation::for_command(&cmd);
        assert_eq!(
            s.send_confirmed(&cmd, conf),
            Err(Error::Forbidden { ty: 0x48 })
        );
        let cmd = Command::Raw("\u{2}x".into()); // 0x02 starts ADCNEW on CDC
        let conf = HighVoltageConfirmation::for_command(&cmd);
        assert_eq!(
            s.send_confirmed(&cmd, conf),
            Err(Error::UnsupportedOnCdc { ty: 0x02 })
        );
        assert_eq!(port.writes(), vec!["pdm open"]);
    }

    // ---- AC-PR41FT.7: reopening the port re-opens the PDM session first ----

    #[test]
    fn reopen_sends_pdm_open_before_any_other_command() {
        let now = time();
        let first = charger_port(&now);
        let mut s = open_in_pd(&first, &now);
        let second = charger_port(&now);
        s.reopen(second.handle()).expect("reopen");
        assert_eq!(second.writes(), vec!["pdm open"]);
        s.send(&Command::PdmParm).unwrap();
        assert_eq!(second.writes(), vec!["pdm open", "pdm parm"]);
        assert!(first.released(), "the old port is released");
    }

    #[test]
    fn reopen_forgets_the_old_chargers_capabilities() {
        // Another charger may sit on the meter after a reconnect: its offers
        // are unknown until new blocks arrive, so risk must not use the old ones.
        let now = time();
        let first = charger_port(&now);
        let mut s = open_in_pd(&first, &now);
        assert!(s.capabilities().pd.is_some());
        let second = charger_port(&now);
        s.reopen(second.handle()).expect("reopen");
        assert!(s.capabilities().pd.is_none());
        let req1 = Command::PdRequest {
            position: 1,
            voltage_mv: None,
            current_ma: None,
        };
        assert_eq!(s.risk(&req1), Risk::Above5V);
    }

    #[test]
    fn reopen_without_mode_entry_is_a_timeout() {
        let now = time();
        let first = charger_port(&now);
        let mut s = open(&first, &now);
        let dead = FakePort::new(&now);
        let err = s.reopen(dead.handle()).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
        assert_eq!(dead.writes().first().map(String::as_str), Some("pdm open"));
    }

    #[test]
    fn capability_block_enum_is_reachable() {
        // Keeps the CapBlock re-export exercised by the integration surface.
        let now = time();
        let port = charger_port(&now);
        let s = open_in_pd(&port, &now);
        let pd = s.capabilities().pd.clone().unwrap();
        assert_eq!(CapBlock::Pdo(pd).tag(), BlockTag::Pdo);
    }
}
