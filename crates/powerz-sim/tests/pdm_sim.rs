//! The simulator's PDM persona (FR PR41FT): raw replies match the
//! device-confirmed session-2 transcript (rewritten here as our own
//! fixtures), and the protocol library's PDM session runs against it.

use std::io::{self, Read, Write};
use std::time::Duration;

use powerz_protocol::pd::SourcePdo;
use powerz_protocol::pdm::session::MIN_COMMAND_GAP;
use powerz_protocol::pdm::{Command, Error, HighVoltageConfirmation, PdmSession, Protocol};
use powerz_sim::{SimCharger, SimClock, SimPdm, SimPdmPort};

const T0: u64 = 1_790_000_000_000;
const T: Duration = Duration::from_secs(2);

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn spr_block() -> Vec<u8> {
    hex("70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a")
}

fn epr_block() -> Vec<u8> {
    hex("70646f3a 39 0b 2c918109 2cd10200 2cb10400 f4410600
         00000000 00000000 00000000 f4c10800 8c9630d2 00000000 00000000 0a")
}

fn sim() -> (SimPdm, SimClock) {
    let clock = SimClock::new(T0);
    (SimPdm::new(SimCharger::Epr140W, clock.clone()), clock)
}

/// Write one command and collect every reply byte until the port idles.
fn ask(port: &mut SimPdmPort, cmd: &str) -> Vec<u8> {
    port.write_all(cmd.as_bytes()).expect("write");
    read_all(port)
}

fn read_all(port: &mut SimPdmPort) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 64];
    loop {
        match port.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => break,
            Err(e) => panic!("read: {e}"),
        }
    }
    out
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn pd_req(position: u8) -> Command {
    Command::PdRequest {
        position,
        voltage_mv: None,
        current_ma: None,
    }
}

/// Test paths carry `pdm` so `cargo test pdm` selects them.
mod pdm_sim {
    use super::*;

    // ---- AC-PR41FT.6: raw replies of the scripted 140 W EPR charger ----

    #[test]
    fn nothing_is_answered_before_pdm_open() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        assert!(ask(&mut port, "pd req=2").is_empty());
        assert!(ask(&mut port, "entry pd").is_empty());
        assert!(!sim.pdm_active());
        assert_eq!(sim.vbus_mv(), 0);
    }

    #[test]
    fn pdm_open_answers_mode_entry_and_presents_rd() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        assert_eq!(
            text(&ask(&mut port, "pdm open")),
            "pdm mode entry\nver1.0\n"
        );
        assert!(sim.pdm_active());
        assert!((5000..=5300).contains(&sim.vbus_mv()), "{}", sim.vbus_mv());
    }

    #[test]
    fn entry_pd_sends_both_pdo_blocks_unprompted() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        let mut want = b"ok\ncc1 attach\n".to_vec();
        want.extend(spr_block());
        want.extend(b"ready:5191mV,0mA\n");
        want.extend(epr_block());
        want.extend(b"ready:5162mV,0mA\n");
        assert_eq!(ask(&mut port, "entry pd"), want);
    }

    #[test]
    fn session_control_replies() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        assert_eq!(text(&ask(&mut port, "pdm parm")), "type=0,em=2,sink=0\n");
        assert_eq!(text(&ask(&mut port, "reset")), "ok\n");
        assert!(sim.pdm_active(), "reset keeps the session");
        assert_eq!(text(&ask(&mut port, "pdm close")), "ok\n");
        assert!(!sim.pdm_active());
        assert!(
            sim.vbus_mv() < 100,
            "Rd removed: VBUS off, got {}",
            sim.vbus_mv()
        );
    }

    #[test]
    fn unknown_text_gets_no_reply_and_matching_is_case_sensitive() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        assert!(ask(&mut port, "bogus").is_empty());
        assert!(ask(&mut port, "PDM PARM").is_empty());
    }

    #[test]
    fn protocols_the_charger_lacks_time_out() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        for p in ["qc", "fcp", "scp", "afc", "sfcp", "tfcp"] {
            assert_eq!(
                text(&ask(&mut port, &format!("entry {p}"))),
                "ok\ntimeout\n",
                "entry {p}"
            );
            assert!((5000..=5300).contains(&sim.vbus_mv()), "stays at 5 V");
        }
        assert_eq!(
            text(&ask(&mut port, "entry ufcs")),
            "ok\ntimeout\ntimeout\ntimeout\n"
        );
    }

    #[test]
    fn pd_requests_answer_req_and_ready_and_move_vbus() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        ask(&mut port, "entry pd");
        let reply = text(&ask(&mut port, "pd req=2"));
        assert!(reply.starts_with("req:9000mV,3000mA\nready:"), "{reply:?}");
        assert!(reply.ends_with("mV,0mA\n"), "{reply:?}");
        assert!((8550..=9450).contains(&sim.vbus_mv()), "{}", sim.vbus_mv());
        let reply = text(&ask(&mut port, "pd req=8"));
        assert!(reply.starts_with("req:28000mV,5000mA\nready:"), "{reply:?}");
        assert!(
            (26600..=29400).contains(&sim.vbus_mv()),
            "{}",
            sim.vbus_mv()
        );
        let reply = text(&ask(&mut port, "pd req=9,volt=15000,cur=1000"));
        assert!(reply.starts_with("req:15000mV,1000mA\nready:"), "{reply:?}");
        assert!(
            (14250..=15750).contains(&sim.vbus_mv()),
            "{}",
            sim.vbus_mv()
        );
    }

    #[test]
    fn hard_reset_returns_to_5v_and_resends_the_offers() {
        let (sim, _) = sim();
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        ask(&mut port, "entry pd");
        ask(&mut port, "pd req=4");
        let reply = ask(&mut port, "pd hrst");
        let pdo_tags = reply.windows(4).filter(|w| w == b"pdo:").count();
        assert_eq!(pdo_tags, 2, "{reply:?}");
        assert!((5000..=5300).contains(&sim.vbus_mv()), "{}", sim.vbus_mv());
    }

    #[test]
    fn reopening_the_tty_drops_the_text_session() {
        let (sim, _) = sim();
        let mut old = sim.open_port();
        ask(&mut old, "pdm open");
        ask(&mut old, "entry pd");
        let mut port = sim.open_port();
        assert!(!sim.pdm_active());
        assert!(
            ask(&mut port, "pd req=2").is_empty(),
            "needs pdm open again"
        );
        let err = old.write(b"pdm parm").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe, "stale handle");
    }

    #[test]
    fn writes_are_recorded_one_per_call_with_sim_time() {
        let (sim, clock) = sim();
        let mut port = sim.open_port();
        port.write_all(b"pdm open").unwrap();
        clock.advance(Duration::from_millis(70));
        port.write_all(b"pdm parm").unwrap();
        assert_eq!(sim.writes(), vec!["pdm open", "pdm parm"]);
        assert_eq!(sim.write_times_ms(), vec![T0, T0 + 70]);
    }

    #[test]
    fn interleaved_binary_frames_come_before_replies() {
        let (sim, _) = sim();
        let sim = sim.with_interleaved_binary();
        let mut port = sim.open_port();
        let reply = ask(&mut port, "pdm open");
        assert!(
            reply
                .first()
                .is_some_and(|b| *b == 0x02 || b & 0x7F == 0x41),
            "{reply:02x?}"
        );
        assert!(text(&reply).contains("pdm mode entry\n"));
    }

    #[test]
    fn dropping_the_handle_closes_the_port() {
        let (sim, _) = sim();
        let port = sim.open_port();
        assert!(sim.port_open());
        drop(port);
        assert!(!sim.port_open());
    }

    // ---- AC-PR41FT.6 with AC .1/.2/.3/.5/.7: the PDM session against the simulator ----

    fn session(sim: &SimPdm, clock: &SimClock) -> PdmSession<SimPdmPort, SimClock> {
        PdmSession::open_with_clock(sim.open_port(), clock.clone()).expect("open")
    }

    #[test]
    fn full_pd_session_against_the_simulator() {
        let (sim, clock) = sim();
        let mut s = session(&sim, &clock);
        s.send(&Command::Entry(Protocol::Pd)).unwrap();
        s.wait_for(&["ready"], T).unwrap();
        s.wait_for(&["ready"], T).unwrap();
        let offers = s.capabilities().pd.as_ref().expect("pdo").offers();
        let positions: Vec<u8> = offers.iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, vec![1, 2, 3, 4, 8, 9]);
        assert!(matches!(offers[5].1, SourcePdo::EprAvs(_)));

        // Above 5 V: refused, nothing written, VBUS unchanged.
        let writes_before = sim.writes();
        let err = s.send(&pd_req(2)).unwrap_err();
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        assert_eq!(sim.writes(), writes_before);
        assert!(sim.vbus_mv() < 5500);

        // Confirmed for that command: sent once, VBUS rises.
        s.send_confirmed(&pd_req(2), HighVoltageConfirmation::for_command(&pd_req(2)))
            .unwrap();
        assert_eq!(s.wait_for(&["req:"], T).unwrap(), "req:9000mV,3000mA");
        s.wait_for(&["ready"], T).unwrap();
        assert!((8550..=9450).contains(&sim.vbus_mv()));

        // Back to 5 V needs no confirmation.
        s.send(&pd_req(1)).unwrap();
        s.wait_for(&["ready"], T).unwrap();
        assert!(sim.vbus_mv() < 5500);

        s.close().unwrap();
        assert!(!sim.pdm_active());
        assert!(!sim.port_open());
        assert_eq!(sim.writes().last().map(String::as_str), Some("pdm close"));
    }

    #[test]
    fn busy_meter_gets_reset_after_pdm_open() {
        let (sim, clock) = sim();
        let sim = sim.with_busy_open();
        let s = session(&sim, &clock);
        assert_eq!(sim.writes(), vec!["pdm open", "reset"]);
        drop(s);
    }

    #[test]
    fn session_writes_are_paced_on_the_sim_clock() {
        let (sim, clock) = sim();
        let mut s = session(&sim, &clock);
        for c in [
            Command::PdmParm,
            Command::Entry(Protocol::Qc),
            Command::Reset,
        ] {
            s.send(&c).unwrap();
        }
        drop(s);
        let times = sim.write_times_ms();
        assert!(times.len() >= 5, "{:?}", sim.writes());
        for pair in times.windows(2) {
            assert!(
                pair[1] - pair[0] >= MIN_COMMAND_GAP.as_millis() as u64,
                "{times:?}"
            );
        }
    }

    #[test]
    fn dropping_the_session_closes_pdm_before_releasing_the_port() {
        let (sim, clock) = sim();
        let mut s = session(&sim, &clock);
        s.send(&Command::Entry(Protocol::Pd)).unwrap();
        s.wait_for(&["ready"], T).unwrap();
        drop(s);
        assert_eq!(sim.writes().last().map(String::as_str), Some("pdm close"));
        assert!(!sim.pdm_active(), "pdm close reached the meter");
        assert!(!sim.port_open());
    }

    #[test]
    fn missing_protocol_reports_the_device_timeout_line() {
        let (sim, clock) = sim();
        let mut s = session(&sim, &clock);
        s.send(&Command::Entry(Protocol::Scp)).unwrap();
        assert_eq!(s.wait_for(&["ok"], T).unwrap(), "ok");
        assert_eq!(s.wait_for(&["ready", "timeout"], T).unwrap(), "timeout");
    }

    #[test]
    fn session_reads_through_interleaved_binary_frames() {
        let (sim, clock) = sim();
        let sim = sim.with_interleaved_binary();
        let mut s = session(&sim, &clock);
        s.send(&Command::PdmParm).unwrap();
        assert_eq!(s.wait_for(&["type="], T).unwrap(), "type=0,em=2,sink=0");
    }

    #[test]
    fn reopen_reopens_pdm_before_any_other_command() {
        let (sim, clock) = sim();
        let mut s = session(&sim, &clock);
        s.send(&Command::Entry(Protocol::Pd)).unwrap();
        s.wait_for(&["ready"], T).unwrap();
        let before = sim.writes().len();
        s.reopen(sim.open_port()).expect("reopen");
        assert!(sim.pdm_active());
        assert_eq!(sim.writes()[before], "pdm open");
        s.send(&Command::PdmParm).unwrap();
        assert_eq!(s.wait_for(&["type="], T).unwrap(), "type=0,em=2,sink=0");
    }
}
