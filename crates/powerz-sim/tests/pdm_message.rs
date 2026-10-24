//! Custom PD messages and the raw console against the simulated meter
//! (FR DGSQ6T, AC .2, .4): the session-2 240 W e-marked cable answers a
//! SOP' Discover Identity, which decodes into its e-marker details; the
//! console shows reply lines and parsed blocks, skipping binary frames.

use std::time::Duration;

use powerz_protocol::pdm::caps::CapBlock;
use powerz_protocol::pdm::console::{self, Direction};
use powerz_protocol::pdm::message::{self, PdMessage, Preset, SopTarget};
use powerz_protocol::pdm::{Command, HighVoltageConfirmation, PdmSession};
use powerz_sim::{SimCable, SimCharger, SimClock, SimPdm, SimPdmPort};

const T0: u64 = 1_790_000_000_000;
const WAIT: Duration = Duration::from_millis(1000);

fn sim(cable: Option<SimCable>) -> (SimPdm, SimClock) {
    let clock = SimClock::new(T0);
    let mut sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
    if let Some(c) = cable {
        sim = sim.with_cable(c);
    }
    (sim, clock)
}

fn open(sim: &SimPdm, clock: &SimClock) -> PdmSession<SimPdmPort, SimClock> {
    PdmSession::open_with_clock(sim.open_port(), clock.clone()).expect("pdm open")
}

/// Send `msg` with a confirmation for it (whatever its risk class).
fn send(s: &mut PdmSession<SimPdmPort, SimClock>, msg: &PdMessage) -> Command {
    let cmd = msg.to_command().expect("builds");
    let conf = HighVoltageConfirmation::for_command(&cmd);
    s.send_confirmed(&cmd, conf).expect("sent");
    cmd
}

/// Test paths carry `pdm` so `cargo test pdm` selects them.
mod pdm_message {
    use super::*;

    // ---- AC-DGSQ6T.2: the cable answers the preset; the reply decodes ----

    #[test]
    fn the_emarked_cable_answers_discover_identity_with_its_details() {
        let (sim, clock) = sim(Some(SimCable::Epr240W));
        let mut s = open(&sim, &clock);
        let cmd = send(&mut s, &Preset::DiscoverIdentityCable.message());
        let wire = String::from_utf8(cmd.render()).unwrap();
        assert!(sim.writes().contains(&wire), "{:?}", sim.writes());
        let id = message::await_cable_identity(&mut s, WAIT)
            .expect("no session error")
            .expect("the cable answered");
        assert_eq!(id.sop, SopTarget::SopPrime);
        assert_eq!(id.vid, 0x050D);
        assert_eq!(id.pid, Some(0x8254));
        assert!(!id.active, "passive cable");
        assert_eq!(id.max_voltage_v, Some(50));
        assert_eq!(id.max_current_ma, Some(5000));
        assert!(id.epr_capable);
        s.close().expect("close");
    }

    #[test]
    fn without_an_emarked_cable_nothing_answers() {
        let (sim, clock) = sim(None);
        let mut s = open(&sim, &clock);
        send(&mut s, &Preset::DiscoverIdentityCable.message());
        let id = message::await_cable_identity(&mut s, WAIT).expect("no session error");
        assert_eq!(id, None);
        s.close().expect("close");
    }

    #[test]
    fn discover_identity_to_the_charger_is_not_answered_by_the_cable() {
        let (sim, clock) = sim(Some(SimCable::Epr240W));
        let mut s = open(&sim, &clock);
        let mut msg = Preset::DiscoverIdentityCable.message();
        msg.sop = SopTarget::Sop;
        send(&mut s, &msg);
        let id = message::await_cable_identity(&mut s, WAIT).expect("no session error");
        assert_eq!(id, None);
        s.close().expect("close");
    }

    #[test]
    fn the_session_stays_usable_after_pd_cmd_and_pd_data() {
        let (sim, clock) = sim(Some(SimCable::Epr240W));
        let mut s = open(&sim, &clock);
        send(&mut s, &PdMessage::control(SopTarget::SopPrime, 13));
        send(&mut s, &Preset::DiscoverIdentityCable.message());
        message::await_cable_identity(&mut s, WAIT)
            .expect("no session error")
            .expect("the cable answered after a SOP' soft reset");
        let parm = console::send_line(&mut s, "pdm parm", None, WAIT)
            .expect("parm")
            .entries;
        assert!(
            parm.iter().any(|e| e.text == "type=0,em=2,sink=0"),
            "{parm:?}"
        );
        s.close().expect("close");
    }

    // ---- AC-DGSQ6T.4: the console on the simulated meter ----

    #[test]
    fn the_console_shows_lines_and_parsed_blocks_and_skips_binary_frames() {
        let clock = SimClock::new(T0);
        let sim = SimPdm::new(SimCharger::Epr140W, clock.clone()).with_interleaved_binary();
        let mut s = open(&sim, &clock);
        let t = console::send_line(&mut s, "entry pd", None, WAIT)
            .expect("entry pd")
            .entries;
        assert_eq!(t[0].direction, Direction::Tx);
        assert_eq!(t[0].text, "entry pd");
        assert!(t[0].at_ms >= T0, "{t:?}");
        let rx: Vec<_> = t.iter().filter(|e| e.direction == Direction::Rx).collect();
        assert!(rx.iter().any(|e| e.text == "ok"), "{t:?}");
        assert!(
            rx.iter().any(|e| matches!(e.block, Some(CapBlock::Pdo(_)))),
            "{t:?}"
        );
        // Binary frames are skipped, never shown as text.
        assert!(
            rx.iter()
                .all(|e| e.block.is_some() || e.text.chars().all(|c| !c.is_control())),
            "{t:?}"
        );
        // Times never go backwards.
        assert!(t.windows(2).all(|w| w[0].at_ms <= w[1].at_ms), "{t:?}");
        s.close().expect("close");
    }
}
