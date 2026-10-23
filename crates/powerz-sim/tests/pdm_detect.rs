//! Charger protocol detection (FR DGSQ6R) against the simulated meter:
//! the session-2 reply forms (`entry bc` → `ok`, `entry apple` → `fail`,
//! `entry list` / `entry list+` → three lines, then silence until `reset`).
//! The simulated clock makes the 45 s stall instant.

use std::io::{self, Read, Write};
use std::time::Duration;

use powerz_protocol::pdm::PdmSession;
use powerz_protocol::pdm::detect::{self, ListOptions, Support};
use powerz_protocol::pdm::pd;
use powerz_sim::{SimCharger, SimClock, SimPdm, SimPdmPort};

const T0: u64 = 1_790_000_000_000;

fn sim(charger: SimCharger) -> (SimPdm, SimClock) {
    let clock = SimClock::new(T0);
    (SimPdm::new(charger, clock.clone()), clock)
}

fn open(sim: &SimPdm, clock: &SimClock) -> PdmSession<SimPdmPort, SimClock> {
    PdmSession::open_with_clock(sim.open_port(), clock.clone()).expect("pdm open")
}

fn ask(port: &mut SimPdmPort, cmd: &str) -> String {
    port.write_all(cmd.as_bytes()).expect("write");
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
    String::from_utf8_lossy(&out).into_owned()
}

const SESSION2_LIST: &str = "starting\nbc1.2:true 5203mV,0mA\nufcs:null\nvfcp:null\n";

/// Test paths carry `pdm_detect` so `cargo test detect` selects them.
mod pdm_detect {
    use super::*;

    // ---- AC-DGSQ6R.2: the simulator speaks the session-2 list ----

    #[test]
    fn sim_entry_list_answers_the_session_2_lines_then_stalls() {
        let (sim, _clock) = sim(SimCharger::Epr140W);
        let mut port = sim.open_port();
        assert!(ask(&mut port, "pdm open").starts_with("pdm mode entry"));
        assert_eq!(ask(&mut port, "entry list"), SESSION2_LIST);
        // Stalled: nothing more until `reset`.
        assert_eq!(ask(&mut port, "entry apple"), "");
        assert_eq!(ask(&mut port, "reset"), "ok\n");
        // After `reset` the meter answers again.
        assert_eq!(ask(&mut port, "entry apple"), "fail\n");
    }

    #[test]
    fn sim_entry_list_plus_answers_the_same_lines() {
        let (sim, _clock) = sim(SimCharger::Pps65W);
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        assert_eq!(ask(&mut port, "entry list+"), SESSION2_LIST);
        assert_eq!(ask(&mut port, "reset"), "ok\n");
    }

    #[test]
    fn list_against_the_sim_is_incomplete_after_the_default_stall() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        let start = clock.now_ms();
        let mut count = 0;
        let r = detect::list(&mut s, &ListOptions::default(), &mut |_| count += 1).unwrap();
        assert_eq!(count, 3);
        assert!(!r.complete);
        let names: Vec<&str> = r.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["bc1.2", "ufcs", "vfcp"]);
        assert_eq!(r.entries[0].support, Support::Supported);
        assert_eq!(r.entries[0].vbus_mv, Some(5203));
        let elapsed = clock.now_ms() - start;
        assert!(
            (45_000..=47_000).contains(&elapsed),
            "stall took {elapsed} ms"
        );
        s.close().unwrap();
        let w = sim.writes();
        let list_at = w
            .iter()
            .position(|c| c == "entry list")
            .expect("entry list");
        let reset_at = w.iter().position(|c| c == "reset").expect("reset");
        assert!(list_at < reset_at, "{w:?}");
        assert_eq!(w.last().map(String::as_str), Some("pdm close"));
        assert!(!sim.pdm_active());
    }

    #[test]
    fn list_plus_against_the_sim_with_a_short_stall() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        let opts = ListOptions {
            plus: true,
            stall_limit: Duration::from_secs(2),
        };
        let r = detect::list(&mut s, &opts, &mut |_| {}).unwrap();
        assert_eq!(r.entries.len(), 3);
        assert!(!r.complete);
        assert!(sim.writes().contains(&"entry list+".to_string()));
    }

    // ---- AC-DGSQ6R.1: session-2 BC1.2 / Apple forms ----

    #[test]
    fn bc_is_unknown_and_apple_not_supported_on_the_140w_charger() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        assert_eq!(detect::bc12(&mut s).unwrap(), Support::Unknown);
        assert_eq!(detect::apple(&mut s).unwrap(), Support::NotSupported);
        s.close().unwrap();
        let w = sim.writes();
        assert!(w.contains(&"entry bc".to_string()), "{w:?}");
        assert!(w.contains(&"entry apple".to_string()), "{w:?}");
    }

    // ---- AC-DGSQ6R.5: no confirmation, even with the 140 W offers known ----

    #[test]
    fn detection_after_pd_offers_needs_no_confirmation_and_stays_at_5v() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        pd::enter(&mut s).expect("entry pd");
        assert_eq!(detect::apple(&mut s).unwrap(), Support::NotSupported);
        assert_eq!(detect::bc12(&mut s).unwrap(), Support::Unknown);
        let opts = ListOptions {
            plus: false,
            stall_limit: Duration::from_secs(1),
        };
        detect::list(&mut s, &opts, &mut |_| {}).unwrap();
        assert!(sim.vbus_mv() <= 5_300, "VBUS {} mV", sim.vbus_mv());
        s.close().unwrap();
    }
}
