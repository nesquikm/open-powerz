//! Legacy and vendor protocol triggering (FR DGSQ6Q) against the simulated
//! meter: the session-2 140 W charger (PD only: every legacy entry times
//! out, VFCP answers `ready` with no block) and the synthetic
//! multi-protocol charger (QC / FCP / AFC / SFCP / SCP / VFCP / TFCP / UFCS
//! with synthetic capability blocks).

use std::io::{self, Read, Write};

use powerz_protocol::pdm::legacy::{self, Error as LegacyError, LegacyProtocol, Request};
use powerz_protocol::pdm::{FixedProtocol, HighVoltageConfirmation, PdmSession};
use powerz_sim::{SimCharger, SimClock, SimPdm, SimPdmPort};

const T0: u64 = 1_790_000_000_000;

fn sim(charger: SimCharger) -> (SimPdm, SimClock) {
    let clock = SimClock::new(T0);
    (SimPdm::new(charger, clock.clone()), clock)
}

fn open(sim: &SimPdm, clock: &SimClock) -> PdmSession<SimPdmPort, SimClock> {
    PdmSession::open_with_clock(sim.open_port(), clock.clone()).expect("pdm open")
}

fn ask(port: &mut SimPdmPort, cmd: &str) -> Vec<u8> {
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
    out
}

fn ask_text(port: &mut SimPdmPort, cmd: &str) -> String {
    String::from_utf8_lossy(&ask(port, cmd)).into_owned()
}

fn within_5_percent(got_mv: u32, want_mv: u32) -> bool {
    got_mv.abs_diff(want_mv) * 20 <= want_mv
}

/// Send `req` with a confirmation made for its rendered command.
fn confirmed(
    s: &mut PdmSession<SimPdmPort, SimClock>,
    req: Request,
) -> Result<legacy::Outcome, LegacyError> {
    let cmd = legacy::validate(s.capabilities(), &req)?;
    legacy::request(s, &req, Some(HighVoltageConfirmation::for_command(&cmd)))
}

/// Writes after `pdm open`.
fn sent(sim: &SimPdm) -> Vec<String> {
    sim.writes().into_iter().skip(1).collect()
}

/// Test paths carry `pdm` so `cargo test pdm` selects them.
mod pdm_legacy {
    use super::*;

    // ---- the simulated chargers themselves ----

    #[test]
    fn multi_protocol_entries_answer_ready_with_their_blocks() {
        let (sim, _clock) = sim(SimCharger::MultiProtocol);
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        for p in ["qc", "fcp", "afc", "sfcp"] {
            let r = ask_text(&mut port, &format!("entry {p}"));
            assert!(r.starts_with("ok\nready\n"), "{p}: {r:?}");
            ask(&mut port, "reset");
        }
        for (p, tag) in [
            ("scp", &b"scp:"[..]),
            ("vfcp", b"vfcp:"),
            ("tfcp", b"tfcp:"),
            ("ufcs", b"ufcs:"),
        ] {
            let r = ask(&mut port, &format!("entry {p}"));
            assert!(r.starts_with(b"ok\nready\n"), "{p}: {r:?}");
            assert!(
                r.windows(tag.len()).any(|w| w == tag),
                "{p}: no block in {r:?}"
            );
            ask(&mut port, "reset");
        }
    }

    #[test]
    fn multi_protocol_requests_move_vbus_and_reset_returns_to_5v() {
        let (sim, _clock) = sim(SimCharger::MultiProtocol);
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        ask(&mut port, "entry qc");
        let r = ask_text(&mut port, "qc 9V");
        assert!(r.starts_with("ready"), "{r:?}");
        assert!(within_5_percent(sim.vbus_mv(), 9000), "{}", sim.vbus_mv());
        ask(&mut port, "qc3 volt=6000");
        assert!(within_5_percent(sim.vbus_mv(), 6000), "{}", sim.vbus_mv());
        ask(&mut port, "qc3 inc=2");
        assert!(within_5_percent(sim.vbus_mv(), 6400), "{}", sim.vbus_mv());
        ask(&mut port, "qc3 dec=1");
        assert!(within_5_percent(sim.vbus_mv(), 6200), "{}", sim.vbus_mv());
        assert_eq!(ask_text(&mut port, "reset"), "ok\n");
        assert!(within_5_percent(sim.vbus_mv(), 5000), "{}", sim.vbus_mv());
    }

    #[test]
    fn epr_charger_still_times_out_every_legacy_entry() {
        let (sim, _clock) = sim(SimCharger::Epr140W);
        let mut port = sim.open_port();
        ask(&mut port, "pdm open");
        for p in ["qc", "fcp", "scp", "afc", "sfcp", "tfcp"] {
            assert_eq!(ask_text(&mut port, &format!("entry {p}")), "ok\ntimeout\n");
        }
        assert_eq!(
            ask_text(&mut port, "entry ufcs"),
            "ok\ntimeout\ntimeout\ntimeout\n"
        );
        assert_eq!(ask_text(&mut port, "entry vfcp"), "ok\nready\n");
        // Requests after a timed-out entry get no reply; VBUS stays at 5 V. [device]
        assert_eq!(ask_text(&mut port, "qc 9V"), "");
        assert!(within_5_percent(sim.vbus_mv(), 5180), "{}", sim.vbus_mv());
    }

    // ---- AC-DGSQ6Q.1: QC 2.0 and QC 3.0 ----

    #[test]
    fn qc2_fixed_voltages_reach_vbus() {
        for v in [5u8, 9, 12, 20] {
            let (sim, clock) = sim(SimCharger::MultiProtocol);
            let mut s = open(&sim, &clock);
            legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
            let out = confirmed(
                &mut s,
                Request::Fixed {
                    protocol: FixedProtocol::Qc,
                    volts: v,
                },
            )
            .unwrap();
            assert_eq!(out.command.render(), format!("qc {v}V").as_bytes());
            assert!(
                within_5_percent(sim.vbus_mv(), u32::from(v) * 1000),
                "{v} V: {}",
                sim.vbus_mv()
            );
        }
    }

    #[test]
    fn qc3_voltage_then_steps() {
        let (sim, clock) = sim(SimCharger::MultiProtocol);
        let mut s = open(&sim, &clock);
        legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
        confirmed(&mut s, Request::Qc3Volt { voltage_mv: 7200 }).unwrap();
        assert!(within_5_percent(sim.vbus_mv(), 7200), "{}", sim.vbus_mv());
        confirmed(&mut s, Request::Qc3Inc(3)).unwrap();
        assert!(within_5_percent(sim.vbus_mv(), 7800), "{}", sim.vbus_mv());
        legacy::request(&mut s, &Request::Qc3Dec(2), None).unwrap();
        assert!(within_5_percent(sim.vbus_mv(), 7400), "{}", sim.vbus_mv());
        assert_eq!(
            sent(&sim),
            ["entry qc", "qc3 volt=7200", "qc3 inc=3", "qc3 dec=2"]
        );
    }

    // ---- AC-DGSQ6Q.2: FCP, AFC, SFCP ----

    #[test]
    fn fcp_afc_sfcp_fixed_voltages_reach_vbus() {
        let cases = [
            (LegacyProtocol::Fcp, FixedProtocol::Fcp),
            (LegacyProtocol::Afc, FixedProtocol::Afc),
            (LegacyProtocol::Sfcp, FixedProtocol::Sfcp),
        ];
        for (p, fp) in cases {
            for v in [5u8, 9, 12] {
                let (sim, clock) = sim(SimCharger::MultiProtocol);
                let mut s = open(&sim, &clock);
                legacy::enter(&mut s, p).unwrap();
                confirmed(
                    &mut s,
                    Request::Fixed {
                        protocol: fp,
                        volts: v,
                    },
                )
                .unwrap();
                assert!(
                    within_5_percent(sim.vbus_mv(), u32::from(v) * 1000),
                    "{p:?} {v} V: {}",
                    sim.vbus_mv()
                );
            }
        }
    }

    // ---- AC-DGSQ6Q.3: SCP / VFCP / TFCP ----

    #[test]
    fn scp_vfcp_tfcp_show_limits_and_request_inside_them() {
        let cases = [
            (
                LegacyProtocol::Scp,
                20_000,
                4000,
                Request::Scp {
                    voltage_mv: 10_000,
                    current_ma: 2000,
                },
                10_000,
            ),
            (
                LegacyProtocol::Vfcp,
                12_000,
                2000,
                Request::Vfcp {
                    voltage_mv: 9000,
                    current_ma: 1500,
                },
                9000,
            ),
            (
                LegacyProtocol::Tfcp,
                20_000,
                3000,
                Request::Tfcp {
                    voltage_mv: 15_000,
                    current_ma: 2000,
                },
                15_000,
            ),
        ];
        for (p, max_mv, max_ma, req, want_mv) in cases {
            let (sim, clock) = sim(SimCharger::MultiProtocol);
            let mut s = open(&sim, &clock);
            let e = legacy::enter(&mut s, p).unwrap();
            assert_eq!(e.limits.len(), 1, "{p:?}: {e:?}");
            assert_eq!(e.limits[0].max_voltage_mv, max_mv, "{p:?}");
            assert_eq!(e.limits[0].max_current_ma, max_ma, "{p:?}");
            let out = confirmed(&mut s, req).unwrap();
            assert!(
                out.reply.as_deref().is_some_and(|r| r.starts_with("ready")),
                "{p:?}: {out:?}"
            );
            assert!(
                within_5_percent(sim.vbus_mv(), want_mv),
                "{p:?}: {}",
                sim.vbus_mv()
            );
        }
    }

    #[test]
    fn scp_request_outside_the_limits_never_reaches_the_meter() {
        let (sim, clock) = sim(SimCharger::MultiProtocol);
        let mut s = open(&sim, &clock);
        legacy::enter(&mut s, LegacyProtocol::Scp).unwrap();
        let err = confirmed(
            &mut s,
            Request::Scp {
                voltage_mv: 9000,
                current_ma: 4500,
            },
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                LegacyError::CurrentOutOfRange {
                    current_ma: 4500,
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(sent(&sim), ["entry scp"]);
        assert!(sim.vbus_mv() < 5500, "{}", sim.vbus_mv());
    }

    // ---- AC-DGSQ6Q.4: UFCS ----

    #[test]
    fn ufcs_capabilities_request_by_number_and_control_command() {
        let (sim, clock) = sim(SimCharger::MultiProtocol);
        let mut s = open(&sim, &clock);
        let e = legacy::enter(&mut s, LegacyProtocol::Ufcs).unwrap();
        let idx: Vec<Option<u8>> = e.limits.iter().map(|l| l.index).collect();
        assert_eq!(idx, [Some(1), Some(2)]);
        assert_eq!(e.limits[0].max_voltage_mv, 11_000);
        assert_eq!(e.limits[1].max_voltage_mv, 21_000);
        confirmed(
            &mut s,
            Request::Ufcs {
                index: 2,
                voltage_mv: 18_000,
                current_ma: 1500,
            },
        )
        .unwrap();
        assert!(within_5_percent(sim.vbus_mv(), 18_000), "{}", sim.vbus_mv());
        // UFCS control numbers need a confirmation (AC-PR41FT.4).
        confirmed(&mut s, Request::UfcsCmd(6)).unwrap();
        assert_eq!(
            sent(&sim),
            ["entry ufcs", "ufcs req=2,volt=18000,cur=1500", "ufcs cmd=6"]
        );
    }

    // ---- AC-DGSQ6Q.5: unsupported protocols leave the meter at 5 V ----

    #[test]
    fn unsupported_protocols_are_reported_and_vbus_stays_at_5v() {
        for p in LegacyProtocol::ALL {
            let (sim, clock) = sim(SimCharger::Epr140W);
            let mut s = open(&sim, &clock);
            let err = legacy::enter(&mut s, p).unwrap_err();
            assert!(
                matches!(err, LegacyError::NotSupported { protocol } if protocol == p),
                "{p:?}: {err:?}"
            );
            // Nothing but the entry reached the meter.
            assert_eq!(sent(&sim), [format!("entry {}", p.name())], "{p:?}");
            assert!(
                within_5_percent(sim.vbus_mv(), 5180),
                "{p:?}: {}",
                sim.vbus_mv()
            );
            legacy::return_to_5v(&mut s).unwrap();
            s.close().unwrap();
        }
    }

    // ---- AC-DGSQ6Q.6: the way back to 5 V ----

    #[test]
    fn return_to_5v_after_a_high_request() {
        let (sim, clock) = sim(SimCharger::MultiProtocol);
        let mut s = open(&sim, &clock);
        legacy::enter(&mut s, LegacyProtocol::Afc).unwrap();
        confirmed(
            &mut s,
            Request::Fixed {
                protocol: FixedProtocol::Afc,
                volts: 12,
            },
        )
        .unwrap();
        assert!(within_5_percent(sim.vbus_mv(), 12_000));
        legacy::return_to_5v(&mut s).unwrap();
        assert!(within_5_percent(sim.vbus_mv(), 5000), "{}", sim.vbus_mv());
        s.close().unwrap();
        assert_eq!(sim.writes().last().map(String::as_str), Some("pdm close"));
    }
}
