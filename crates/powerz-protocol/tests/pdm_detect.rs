//! Charger protocol detection over a scripted PDM port (FR DGSQ6R).
//!
//! The fake port answers exact command bytes with timed reply chunks: each
//! chunk becomes readable a given number of ms (on the shared fake clock)
//! after the command was written, so stalls and "as it arrives" are tested
//! without sleeping. Reply forms are rewritten from the device-confirmed
//! session-2 transcript (`entry bc` → `ok`, `entry apple` → `fail`,
//! `entry list` → three lines then silence until `reset`), plus the doc
//! forms `true` / `false`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::detect::{self, ListEntry, ListOptions, Support};
use powerz_protocol::pdm::{Command, PdmSession, Protocol, Risk};
use powerz_protocol::session::Clock;

const T0: u64 = 1_790_000_000_000;
const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";
/// `entry list` on the 140 W charger, session 2 [device].
const SESSION2_LIST: &[u8] = b"starting\nbc1.2:true 5203mV,0mA\nufcs:null\nvfcp:null\n";

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
    /// command → chunks (delay after the write in ms, bytes)
    script: HashMap<Vec<u8>, Vec<(u64, Vec<u8>)>>,
    writes: Vec<(u64, String)>,
    /// (due time, bytes) not yet readable or not yet read
    scheduled: Vec<(u64, Vec<u8>)>,
}

/// Scripted serial port on a shared fake clock; clones share state.
#[derive(Clone)]
struct FakePort {
    state: Rc<RefCell<State>>,
    now: Rc<Cell<u64>>,
}

impl FakePort {
    fn new(now: &Rc<Cell<u64>>) -> FakePort {
        FakePort {
            state: Rc::default(),
            now: Rc::clone(now),
        }
    }
    fn on(self, cmd: &str, reply: &[u8]) -> FakePort {
        self.on_timed(cmd, &[(0, reply)])
    }
    fn on_timed(self, cmd: &str, chunks: &[(u64, &[u8])]) -> FakePort {
        self.state.borrow_mut().script.insert(
            cmd.as_bytes().to_vec(),
            chunks.iter().map(|(d, b)| (*d, b.to_vec())).collect(),
        );
        self
    }
    fn writes(&self) -> Vec<String> {
        self.state
            .borrow()
            .writes
            .iter()
            .map(|(_, w)| w.clone())
            .collect()
    }
    fn write_time(&self, cmd: &str) -> Option<u64> {
        self.state
            .borrow()
            .writes
            .iter()
            .find(|(_, w)| w == cmd)
            .map(|(t, _)| *t)
    }
}

impl std::fmt::Debug for FakePort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakePort")
    }
}

impl Read for FakePort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let now = self.now.get();
        let mut s = self.state.borrow_mut();
        let Some(i) = s.scheduled.iter().position(|(due, _)| *due <= now) else {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
        };
        let chunk = &mut s.scheduled[i].1;
        let n = buf.len().min(chunk.len()).min(7);
        buf[..n].copy_from_slice(&chunk[..n]);
        chunk.drain(..n);
        if chunk.is_empty() {
            s.scheduled.remove(i);
        }
        Ok(n)
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let now = self.now.get();
        let mut s = self.state.borrow_mut();
        s.writes
            .push((now, String::from_utf8_lossy(buf).into_owned()));
        if let Some(chunks) = s.script.get(buf).cloned() {
            for (delay, bytes) in chunks {
                s.scheduled.push((now + delay, bytes));
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn clock() -> Rc<Cell<u64>> {
    Rc::new(Cell::new(T0))
}

fn base(now: &Rc<Cell<u64>>) -> FakePort {
    FakePort::new(now)
        .on("pdm open", OPEN_REPLY)
        .on("reset", b"ok\n")
        .on("pdm close", b"ok\n")
}

fn open(port: &FakePort, now: &Rc<Cell<u64>>) -> PdmSession<FakePort, FakeClock> {
    PdmSession::open_with_clock(port.clone(), FakeClock(Rc::clone(now))).expect("pdm open")
}

fn names(entries: &[ListEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

mod pdm_detect {
    use super::*;

    // ---- AC-DGSQ6R.1: BC1.2 / Apple reply forms ----

    #[test]
    fn bc_ok_only_is_unknown() {
        // Session 2: `entry bc` → `ok`, nothing else within 8 s. [device]
        let now = clock();
        let port = base(&now).on("entry bc", b"ok\n");
        let mut s = open(&port, &now);
        let start = now.get();
        assert_eq!(detect::bc12(&mut s).unwrap(), Support::Unknown);
        assert!(port.writes().contains(&"entry bc".to_string()));
        // Gives up in bounded time, on the clock.
        assert!(
            now.get() - start <= 10_000,
            "waited {} ms",
            now.get() - start
        );
    }

    #[test]
    fn bc_doc_forms() {
        for (reply, want) in [
            (&b"ok\ntrue\n"[..], Support::Supported),
            (b"ok\nfalse\n", Support::NotSupported),
            (b"true\n", Support::Supported),
            (b"false\n", Support::NotSupported),
        ] {
            let now = clock();
            let port = base(&now).on("entry bc", reply);
            let mut s = open(&port, &now);
            assert_eq!(
                detect::bc12(&mut s).unwrap(),
                want,
                "{}",
                String::from_utf8_lossy(reply)
            );
        }
    }

    #[test]
    fn bc_true_after_a_delay_is_still_seen() {
        let now = clock();
        let port = base(&now).on_timed("entry bc", &[(0, b"ok\n"), (1500, b"true\n")]);
        let mut s = open(&port, &now);
        assert_eq!(detect::bc12(&mut s).unwrap(), Support::Supported);
    }

    #[test]
    fn bc_with_no_reply_at_all_is_unknown() {
        let now = clock();
        let port = base(&now);
        let mut s = open(&port, &now);
        assert_eq!(detect::bc12(&mut s).unwrap(), Support::Unknown);
    }

    #[test]
    fn apple_fail_is_not_supported() {
        // Session 2, non-Apple charger. [device]
        let now = clock();
        let port = base(&now).on("entry apple", b"fail\n");
        let mut s = open(&port, &now);
        assert_eq!(detect::apple(&mut s).unwrap(), Support::NotSupported);
        assert!(port.writes().contains(&"entry apple".to_string()));
    }

    #[test]
    fn apple_doc_forms() {
        for (reply, want) in [
            (&b"true\n"[..], Support::Supported),
            (b"false\n", Support::NotSupported),
            (b"ok\ntrue\n", Support::Supported),
            (b"ok\n", Support::Unknown),
        ] {
            let now = clock();
            let port = base(&now).on("entry apple", reply);
            let mut s = open(&port, &now);
            assert_eq!(
                detect::apple(&mut s).unwrap(),
                want,
                "{}",
                String::from_utf8_lossy(reply)
            );
        }
    }

    // ---- AC-DGSQ6R.2: the automatic list ----

    #[test]
    fn session_2_list_stalls_resets_and_is_incomplete() {
        let now = clock();
        let port = base(&now).on("entry list", SESSION2_LIST);
        let mut s = open(&port, &now);
        let mut seen = Vec::new();
        let r = detect::list(&mut s, &ListOptions::default(), &mut |e| {
            seen.push(e.clone())
        })
        .unwrap();
        assert!(!r.complete);
        assert_eq!(names(&r.entries), ["bc1.2", "ufcs", "vfcp"]);
        assert_eq!(r.entries, seen);
        assert_eq!(r.entries[0].support, Support::Supported);
        assert_eq!(r.entries[0].vbus_mv, Some(5203));
        assert_eq!(r.entries[0].ibus_ma, Some(0));
        assert_eq!(r.entries[1].support, Support::NotSupported);
        assert_eq!(r.entries[2].support, Support::NotSupported);
        // `reset` ends the stall, 45 s (default) after the last line.
        let w = port.writes();
        let list_at = w
            .iter()
            .position(|c| c == "entry list")
            .expect("entry list sent");
        let reset_at = w.iter().rposition(|c| c == "reset").expect("reset sent");
        assert!(reset_at > list_at, "{w:?}");
        let sent = port.write_time("entry list").unwrap();
        let reset = port.state.borrow().writes[reset_at].0;
        let waited = reset - sent;
        assert!(
            (45_000..=46_000).contains(&waited),
            "reset {waited} ms after entry list"
        );
        // Not `entry list+` unless asked.
        assert!(!w.contains(&"entry list+".to_string()), "{w:?}");
    }

    #[test]
    fn a_list_that_never_stops_is_cut_off() {
        // A meter that keeps printing entries must not hold the list open.
        let now = clock();
        let mut reply = b"starting\n".to_vec();
        for i in 0..200 {
            reply.extend(format!("p{i}:null\n").as_bytes());
        }
        let port = base(&now).on("entry list", &reply);
        let mut s = open(&port, &now);
        let r = detect::list(&mut s, &ListOptions::default(), &mut |_| {}).unwrap();
        assert!(!r.complete);
        assert_eq!(r.entries.len(), detect::MAX_LIST_ENTRIES);
        assert!(port.write_time("reset").is_some(), "reset ends the list");
    }

    #[test]
    fn entries_survive_a_reset_that_gets_no_ok() {
        let now = clock();
        let port = FakePort::new(&now)
            .on("pdm open", OPEN_REPLY)
            .on("pdm close", b"ok\n")
            .on("entry list", SESSION2_LIST);
        let mut s = open(&port, &now);
        let r = detect::list(&mut s, &ListOptions::default(), &mut |_| {}).expect("partial list");
        assert!(!r.complete);
        assert_eq!(r.entries.len(), 3);
        assert!(port.write_time("reset").is_some());
    }

    #[test]
    fn a_stop_request_ends_the_list_soon_with_reset() {
        // Ctrl-C in the CLI: the list must not wait out the stall limit.
        let now = clock();
        let port = base(&now).on("entry list", SESSION2_LIST);
        let mut s = open(&port, &now);
        let stop = std::sync::atomic::AtomicBool::new(true);
        let start = now.get();
        let r = detect::list_until(&mut s, &ListOptions::default(), &mut |_| {}, &stop).unwrap();
        assert!(!r.complete);
        let reset = port.write_time("reset").expect("reset sent");
        assert!(reset - start < 2_000, "reset after {} ms", reset - start);
    }

    #[test]
    fn stall_limit_is_configurable() {
        let now = clock();
        let port = base(&now).on("entry list", SESSION2_LIST);
        let mut s = open(&port, &now);
        let opts = ListOptions {
            plus: false,
            stall_limit: Duration::from_secs(5),
        };
        let r = detect::list(&mut s, &opts, &mut |_| {}).unwrap();
        assert!(!r.complete);
        assert_eq!(r.entries.len(), 3);
        let sent = port.write_time("entry list").unwrap();
        let reset = port.write_time("reset").expect("reset sent");
        assert!(
            (5_000..=6_000).contains(&(reset - sent)),
            "reset {} ms after entry list",
            reset - sent
        );
    }

    #[test]
    fn list_plus_only_on_request() {
        let now = clock();
        let port = base(&now).on("entry list+", SESSION2_LIST);
        let mut s = open(&port, &now);
        let opts = ListOptions {
            plus: true,
            stall_limit: Duration::from_secs(5),
        };
        let r = detect::list(&mut s, &opts, &mut |_| {}).unwrap();
        assert_eq!(names(&r.entries), ["bc1.2", "ufcs", "vfcp"]);
        let w = port.writes();
        assert!(w.contains(&"entry list+".to_string()), "{w:?}");
        assert!(!w.contains(&"entry list".to_string()), "{w:?}");
    }

    #[test]
    fn each_line_is_reported_as_it_arrives_and_resets_the_stall_timer() {
        // Lines 30 s apart: each under the 45 s limit, together far over it.
        let now = clock();
        let port = base(&now).on_timed(
            "entry list",
            &[
                (0, b"starting\n"),
                (30_000, b"bc1.2:true 5203mV,0mA\n"),
                (60_000, b"ufcs:null\n"),
                (90_000, b"qc:true 5190mV,0mA\n"),
            ],
        );
        let mut s = open(&port, &now);
        let sent_at = Rc::new(Cell::new(0u64));
        let mut seen_at: Vec<(String, u64)> = Vec::new();
        let clock_view = Rc::clone(&now);
        let r = detect::list(&mut s, &ListOptions::default(), &mut |e| {
            seen_at.push((e.name.clone(), clock_view.get()))
        })
        .unwrap();
        sent_at.set(port.write_time("entry list").unwrap());
        assert_eq!(names(&r.entries), ["bc1.2", "ufcs", "qc"]);
        assert!(!r.complete);
        // Reported when it arrived, not at the end.
        for ((name, at), due) in seen_at.iter().zip([30_000u64, 60_000, 90_000]) {
            let rel = at - sent_at.get();
            assert!(
                (due..due + 1_000).contains(&rel),
                "{name} reported {rel} ms after entry list, arrived at {due}"
            );
        }
        // The stall counts from the last line: reset 45 s after `qc`.
        let reset = port.write_time("reset").expect("reset sent");
        let rel = reset - sent_at.get();
        assert!((135_000..=136_000).contains(&rel), "reset at {rel} ms");
    }

    #[test]
    fn list_survives_unknown_names_and_noise() {
        let now = clock();
        let port = base(&now).on(
            "entry list",
            b"starting\nvooc:true 5100mV,0mA\nwhatever\nmtkpe:null\n",
        );
        let mut s = open(&port, &now);
        let opts = ListOptions {
            plus: false,
            stall_limit: Duration::from_secs(2),
        };
        let r = detect::list(&mut s, &opts, &mut |_| {}).unwrap();
        assert_eq!(names(&r.entries), ["vooc", "mtkpe"]);
    }

    #[test]
    fn session_is_usable_after_a_stall() {
        let now = clock();
        let port = base(&now)
            .on("entry list", SESSION2_LIST)
            .on("entry apple", b"fail\n");
        let mut s = open(&port, &now);
        let opts = ListOptions {
            plus: false,
            stall_limit: Duration::from_secs(3),
        };
        detect::list(&mut s, &opts, &mut |_| {}).unwrap();
        // `reset` → `ok` consumed by list: the next reply is apple's own.
        assert_eq!(detect::apple(&mut s).unwrap(), Support::NotSupported);
        s.close().unwrap();
        assert_eq!(port.writes().last().map(String::as_str), Some("pdm close"));
    }

    // ---- AC-DGSQ6R.5: detection never needs a high-voltage confirmation ----

    #[test]
    fn detection_commands_are_at_most_5v() {
        let now = clock();
        let port = base(&now);
        let s = open(&port, &now);
        for p in [
            Protocol::Bc,
            Protocol::Apple,
            Protocol::List,
            Protocol::ListPlus,
        ] {
            let r = s.risk(&Command::Entry(p));
            assert!(r <= Risk::AtMost5V, "{p:?}: {r:?}");
        }
    }

    #[test]
    fn detection_runs_without_any_confirmation() {
        let now = clock();
        let port = base(&now)
            .on("entry bc", b"ok\n")
            .on("entry apple", b"fail\n")
            .on("entry list", SESSION2_LIST);
        let mut s = open(&port, &now);
        // None of these APIs takes a confirmation; all must succeed.
        detect::bc12(&mut s).expect("bc without confirmation");
        detect::apple(&mut s).expect("apple without confirmation");
        let opts = ListOptions {
            plus: false,
            stall_limit: Duration::from_secs(1),
        };
        detect::list(&mut s, &opts, &mut |_| {}).expect("list without confirmation");
        let w = port.writes();
        for cmd in ["entry bc", "entry apple", "entry list", "reset"] {
            assert!(w.contains(&cmd.to_string()), "{cmd} not sent: {w:?}");
        }
    }
}
