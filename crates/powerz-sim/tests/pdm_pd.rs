//! The PD trigger layer (FR DGSQ6P) against the simulated meter: the
//! session-2 140 W EPR charger, an SPR-only 65 W PPS charger (cannot enter
//! EPR), `pdm set` changing what `pdm parm` reports, and hard reset.

use std::io::{self, Read, Write};

use powerz_protocol::pdm::pd::{
    self, Emarker, Error as PdError, OfferKind, PdSettings, PdType, Request, SinkCaps,
};
use powerz_protocol::pdm::{HighVoltageConfirmation, PdmSession};
use powerz_sim::{SimCharger, SimClock, SimPdm, SimPdmPort};

const T0: u64 = 1_790_000_000_000;

fn sim(charger: SimCharger) -> (SimPdm, SimClock) {
    let clock = SimClock::new(T0);
    (SimPdm::new(charger, clock.clone()), clock)
}

fn open(sim: &SimPdm, clock: &SimClock) -> PdmSession<SimPdmPort, SimClock> {
    PdmSession::open_with_clock(sim.open_port(), clock.clone()).expect("pdm open")
}

fn confirmed(offers: &[pd::Offer], req: &Request) -> Option<HighVoltageConfirmation> {
    let cmd = pd::validate(offers, req).expect("valid request");
    Some(HighVoltageConfirmation::for_command(&cmd))
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

fn within_5_percent(got_mv: u32, want_mv: u32) -> bool {
    got_mv.abs_diff(want_mv) * 20 <= want_mv
}

/// Test paths carry `pdm` so `cargo test pdm` selects them.
mod pdm_pd {
    use super::*;

    // ---- AC-DGSQ6P.3: no extra wait once the EPR offers are in ----

    #[test]
    fn entry_pd_returns_as_soon_as_the_epr_offers_arrived() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        let start = clock.now_ms();
        let n = pd::enter(&mut s).expect("entry pd");
        assert!(pd::epr_entered(&n.offers));
        let waited = clock.now_ms() - start;
        assert!(
            waited < u64::try_from(pd::EPR_SETTLE.as_millis()).unwrap(),
            "waited {waited} ms after the EPR list"
        );
    }

    // ---- AC-DGSQ6P.1: offers after entry pd ----

    #[test]
    fn epr_charger_offers_are_listed_after_entry_pd() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        let n = pd::enter(&mut s).unwrap();
        let positions: Vec<u8> = n.offers.iter().map(|o| o.position).collect();
        assert_eq!(positions, [1, 2, 3, 4, 8, 9]);
        assert_eq!(n.offers[5].kind, OfferKind::EprAvs);
        assert!(pd::epr_entered(&n.offers));
    }

    #[test]
    fn pps_charger_offers_include_the_pps_range() {
        let (sim, clock) = sim(SimCharger::Pps65W);
        let mut s = open(&sim, &clock);
        let n = pd::enter(&mut s).unwrap();
        let pps = n
            .offers
            .iter()
            .find(|o| o.kind == OfferKind::Pps)
            .expect("a PPS offer");
        assert_eq!(pps.position, 5);
        assert_eq!((pps.min_voltage_mv, pps.max_voltage_mv), (3300, 21_000));
        assert_eq!(pps.max_current_ma, Some(3250));
    }

    // ---- AC-DGSQ6P.2: fixed, PPS and AVS requests ----

    #[test]
    fn fixed_and_avs_requests_deliver_the_voltage() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        let offers = pd::enter(&mut s).unwrap().offers;
        for (req, want_mv) in [
            (
                Request::Fixed {
                    position: 2,
                    current_ma: None,
                },
                9000,
            ),
            (
                Request::Fixed {
                    position: 4,
                    current_ma: Some(3000),
                },
                20_000,
            ),
            (
                Request::Programmable {
                    position: 9,
                    voltage_mv: 21_000,
                    current_ma: 1000,
                },
                21_000,
            ),
        ] {
            let out = pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
            assert_eq!(out.requested.voltage_mv, want_mv, "{req:?}");
            assert!(
                within_5_percent(out.delivered.voltage_mv, want_mv),
                "{req:?}: {:?}",
                out.delivered
            );
            assert!(within_5_percent(sim.vbus_mv(), want_mv));
        }
        assert!(
            sim.writes()
                .contains(&"pd req=9,volt=21000,cur=1000".to_string())
        );
        assert!(sim.writes().contains(&"pd req=4,cur=3000".to_string()));
    }

    #[test]
    fn pps_request_with_voltage_and_current() {
        let (sim, clock) = sim(SimCharger::Pps65W);
        let mut s = open(&sim, &clock);
        let offers = pd::enter(&mut s).unwrap().offers;
        let req = Request::Programmable {
            position: 5,
            voltage_mv: 9000,
            current_ma: 2000,
        };
        let out = pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
        assert!(
            sim.writes()
                .contains(&"pd req=5,volt=9000,cur=2000".to_string())
        );
        assert_eq!(out.requested.voltage_mv, 9000);
        assert_eq!(out.requested.current_ma, 2000);
        assert!(within_5_percent(out.delivered.voltage_mv, 9000));
    }

    #[test]
    fn out_of_range_pps_request_writes_nothing() {
        let (sim, clock) = sim(SimCharger::Pps65W);
        let mut s = open(&sim, &clock);
        let offers = pd::enter(&mut s).unwrap().offers;
        let before = sim.writes().len();
        let r = pd::request(
            &mut s,
            &offers,
            &Request::Programmable {
                position: 5,
                voltage_mv: 22_000,
                current_ma: 1000,
            },
            None,
        );
        assert!(
            matches!(r, Err(PdError::VoltageOutOfRange { position: 5, .. })),
            "{r:?}"
        );
        assert_eq!(sim.writes().len(), before);
    }

    // ---- AC-DGSQ6P.3: EPR positions; no EPR mode ----

    #[test]
    fn epr_28v_request_delivers_28v() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        let offers = pd::enter(&mut s).unwrap().offers;
        let req = Request::Fixed {
            position: 8,
            current_ma: None,
        };
        let out = pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
        assert_eq!(out.requested.voltage_mv, 28_000);
        assert_eq!(out.requested.current_ma, 5000);
        assert!(within_5_percent(sim.vbus_mv(), 28_000));
    }

    #[test]
    fn spr_only_charger_cannot_enter_epr() {
        // [inferred]: no EPR `pdo:` block arrives, so position 8 is refused.
        let (sim, clock) = sim(SimCharger::Pps65W);
        let mut s = open(&sim, &clock);
        let n = pd::enter(&mut s).unwrap();
        assert!(!pd::epr_entered(&n.offers));
        assert!(n.offers.iter().all(|o| o.position < 8));
        let before = sim.writes().len();
        let r = pd::request(
            &mut s,
            &n.offers,
            &Request::Fixed {
                position: 8,
                current_ma: None,
            },
            None,
        );
        assert!(
            matches!(r, Err(PdError::EprUnavailable { position: 8 })),
            "{r:?}"
        );
        assert_eq!(sim.writes().len(), before);
    }

    // ---- AC-DGSQ6P.4: pdm set changes what pdm parm reports ----

    #[test]
    fn simulated_pdm_set_is_reported_by_pdm_parm() {
        let (sim, _) = sim(SimCharger::Epr140W);
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        assert_eq!(ask(&mut port, "pdm parm"), "type=0,em=2,sink=0\n");
        ask(&mut port, "pdm set type=2,em=3,sink=1");
        assert_eq!(ask(&mut port, "pdm parm"), "type=2,em=3,sink=1\n");
    }

    #[test]
    fn settings_round_trip_through_the_session() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        assert_eq!(
            pd::read_settings(&mut s).unwrap(),
            PdSettings {
                pd_type: PdType::Auto,
                emarker: Emarker::Epr240W,
                sink: SinkCaps::Pps3A,
            }
        );
        let want = PdSettings {
            pd_type: PdType::Epr,
            emarker: Emarker::V20A5,
            sink: SinkCaps::Pps5A,
        };
        assert_eq!(pd::write_settings(&mut s, &want).unwrap(), want);
        assert!(
            sim.writes()
                .contains(&"pdm set type=2,em=1,sink=1".to_string())
        );
        assert_eq!(pd::read_settings(&mut s).unwrap(), want);
    }

    // ---- AC-DGSQ6P.5: hard reset ----

    #[test]
    fn hard_reset_returns_the_charger_to_5v_with_new_offers() {
        let (sim, clock) = sim(SimCharger::Epr140W);
        let mut s = open(&sim, &clock);
        let offers = pd::enter(&mut s).unwrap().offers;
        let req = Request::Fixed {
            position: 8,
            current_ma: None,
        };
        pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
        assert!(sim.vbus_mv() > 20_000);
        let n = pd::hard_reset(&mut s).unwrap();
        assert!(sim.writes().contains(&"pd hrst".to_string()));
        assert!(within_5_percent(sim.vbus_mv(), 5000), "{}", sim.vbus_mv());
        assert!(within_5_percent(n.vbus.voltage_mv, 5000), "{:?}", n.vbus);
        assert_eq!(n.offers.len(), 6);
    }

    #[test]
    fn hard_reset_on_spr_charger_reports_spr_offers() {
        let (sim, clock) = sim(SimCharger::Pps65W);
        let mut s = open(&sim, &clock);
        pd::enter(&mut s).unwrap();
        let n = pd::hard_reset(&mut s).unwrap();
        assert_eq!(n.offers.len(), 5);
        assert!(within_5_percent(n.vbus.voltage_mv, 5000), "{:?}", n.vbus);
    }
}
