//! USB PD triggering over a scripted PDM port (FR DGSQ6P).
//!
//! The fake port answers exact command bytes with reply bytes rewritten from
//! the device-confirmed session-2 transcript (140 W EPR charger), plus an
//! SPR-only charger for the "cannot enter EPR" case. A fake clock shares the
//! port's time line, so reply timeouts run without sleeping.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::pd::{
    self, Emarker, Error as PdError, OfferKind, PdSettings, PdType, Reading, Request, SinkCaps,
};
use powerz_protocol::pdm::{Error, HighVoltageConfirmation, PdmSession};
use powerz_protocol::session::Clock;

const T0: u64 = 1_790_000_000_000;

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

/// Offers of the 140 W charger after `entry pd` / `pd hrst`. [device]
fn epr_offers_reply(prefix: &[u8]) -> Vec<u8> {
    let mut b = prefix.to_vec();
    b.extend(spr_block());
    b.extend(b"ready:5191mV,0mA\n");
    b.extend(epr_block());
    b.extend(b"ready:5162mV,0mA\n");
    b
}

/// An SPR-only answer: the EPR block never comes. [inferred]
fn spr_only_reply() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(spr_block());
    b.extend(b"ready:5191mV,0mA\n");
    b
}

const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";

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
    fn writes(&self) -> Vec<String> {
        self.state
            .borrow()
            .writes
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect()
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
        let n = buf.len().min(s.pending.len()).min(9);
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

/// The 140 W charger behind the meter, scripted. [device]
fn epr_port() -> FakePort {
    FakePort::default()
        .on("pdm open", OPEN_REPLY)
        .on("entry pd", &epr_offers_reply(b"ok\ncc1 attach\n"))
        .on("pd req=1", b"req:5000mV,3000mA\nready:5180mV,0mA\n")
        .on("pd req=2", b"req:9000mV,3000mA\nready:8554mV,0mA\n")
        .on("pd req=8", b"req:28000mV,5000mA\nready:27580mV,0mA\n")
        .on(
            "pd req=9,volt=15000,cur=1000",
            b"req:15000mV,1000mA\nready:14913mV,0mA\n",
        )
        .on("pd hrst", &epr_offers_reply(b""))
        .on("pdm parm", b"type=0,em=2,sink=0\n")
        .on("pdm close", b"ok\n")
}

fn open(port: &FakePort) -> PdmSession<FakePort, FakeClock> {
    PdmSession::open_with_clock(port.clone(), FakeClock(Rc::new(Cell::new(T0)))).expect("pdm open")
}

fn confirmed(offers: &[pd::Offer], req: &Request) -> Option<HighVoltageConfirmation> {
    let cmd = pd::validate(offers, req).expect("valid request");
    Some(HighVoltageConfirmation::for_command(&cmd))
}

mod pdm_pd {
    use super::*;

    // ---- AC-DGSQ6P.1: a meter that keeps reporting ready: cannot hold entry pd open ----

    #[test]
    fn negotiation_stops_after_a_bounded_number_of_ready_lines() {
        let mut reply = b"ok\ncc1 attach\n".to_vec();
        reply.extend(spr_block());
        for _ in 0..100 {
            reply.extend(b"ready:5191mV,0mA\n");
        }
        let port = FakePort::default()
            .on("pdm open", OPEN_REPLY)
            .on("entry pd", &reply)
            .on("pdm close", b"ok\n");
        let mut s = open(&port);
        pd::enter(&mut s).expect("entry pd returns");
        // Lines past the cap are left unread rather than waited out.
        let next = s.wait_for(&["ready:"], std::time::Duration::from_millis(10));
        assert!(next.is_ok(), "ready: lines left after the cap: {next:?}");
    }

    // ---- AC-DGSQ6P.1: offers after entry pd ----

    #[test]
    fn entry_pd_lists_the_epr_offers_decoded() {
        let port = epr_port();
        let mut s = open(&port);
        let n = pd::enter(&mut s).expect("entry pd");
        assert!(port.writes().contains(&"entry pd".to_string()));
        let rows: Vec<(u8, OfferKind, u32, u32)> = n
            .offers
            .iter()
            .map(|o| (o.position, o.kind, o.min_voltage_mv, o.max_voltage_mv))
            .collect();
        assert_eq!(
            rows,
            [
                (1, OfferKind::Fixed, 5000, 5000),
                (2, OfferKind::Fixed, 9000, 9000),
                (3, OfferKind::Fixed, 15_000, 15_000),
                (4, OfferKind::Fixed, 20_000, 20_000),
                (8, OfferKind::Fixed, 28_000, 28_000),
                (9, OfferKind::EprAvs, 15_000, 28_000),
            ]
        );
        // EPR list: 20 V at 5 A, not the SPR 4.7 A.
        assert_eq!(n.offers[3].max_current_ma, Some(5000));
        assert_eq!(n.offers[5].max_power_mw, Some(140_000));
        assert!(pd::epr_entered(&n.offers));
        assert_eq!(
            n.vbus,
            Reading {
                voltage_mv: 5162,
                current_ma: 0
            }
        );
    }

    // ---- AC-DGSQ6P.2: fixed and AVS requests, req/ready reported ----

    #[test]
    fn confirmed_fixed_request_reports_requested_and_delivered() {
        let port = epr_port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).unwrap().offers;
        let req = Request::Fixed {
            position: 2,
            current_ma: None,
        };
        let out = pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
        assert!(port.writes().contains(&"pd req=2".to_string()));
        assert_eq!(
            out.requested,
            Reading {
                voltage_mv: 9000,
                current_ma: 3000
            }
        );
        assert_eq!(
            out.delivered,
            Reading {
                voltage_mv: 8554,
                current_ma: 0
            }
        );
        assert_eq!(out.command.render(), b"pd req=2");
    }

    #[test]
    fn avs_request_is_sent_in_device_form() {
        let port = epr_port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).unwrap().offers;
        let req = Request::Programmable {
            position: 9,
            voltage_mv: 15_000,
            current_ma: 1000,
        };
        let out = pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
        assert!(
            port.writes()
                .contains(&"pd req=9,volt=15000,cur=1000".to_string())
        );
        assert_eq!(out.requested.voltage_mv, 15_000);
        assert_eq!(out.requested.current_ma, 1000);
        assert_eq!(out.delivered.voltage_mv, 14_913);
    }

    #[test]
    fn unconfirmed_high_voltage_request_writes_nothing() {
        let port = epr_port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).unwrap().offers;
        let before = port.writes().len();
        let r = pd::request(
            &mut s,
            &offers,
            &Request::Fixed {
                position: 2,
                current_ma: None,
            },
            None,
        );
        assert!(
            matches!(r, Err(PdError::Pdm(Error::HighVoltageNotConfirmed { .. }))),
            "{r:?}"
        );
        assert_eq!(port.writes().len(), before);
    }

    #[test]
    fn five_volt_request_needs_no_confirmation() {
        let port = epr_port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).unwrap().offers;
        let out = pd::request(
            &mut s,
            &offers,
            &Request::Fixed {
                position: 1,
                current_ma: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(out.requested.voltage_mv, 5000);
        assert_eq!(out.delivered.voltage_mv, 5180);
    }

    #[test]
    fn out_of_range_request_is_refused_before_writing() {
        let port = epr_port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).unwrap().offers;
        let before = port.writes().len();
        let bad = Request::Programmable {
            position: 9,
            voltage_mv: 30_000,
            current_ma: 1000,
        };
        // Even a confirmation for the rendered text does not get past validation.
        let conf =
            HighVoltageConfirmation::for_command(&powerz_protocol::pdm::Command::PdRequest {
                position: 9,
                voltage_mv: Some(30_000),
                current_ma: Some(1000),
            });
        let r = pd::request(&mut s, &offers, &bad, Some(conf));
        assert!(
            matches!(r, Err(PdError::VoltageOutOfRange { position: 9, .. })),
            "{r:?}"
        );
        assert_eq!(port.writes().len(), before);
    }

    // ---- AC-DGSQ6P.3: EPR positions, and no EPR mode ----

    #[test]
    fn epr_28v_request_by_position() {
        let port = epr_port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).unwrap().offers;
        let req = Request::Fixed {
            position: 8,
            current_ma: None,
        };
        let out = pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
        assert!(port.writes().contains(&"pd req=8".to_string()));
        assert_eq!(out.requested.voltage_mv, 28_000);
        assert_eq!(out.delivered.voltage_mv, 27_580);
    }

    #[test]
    fn charger_without_epr_reports_epr_unavailable() {
        // [inferred] not device-confirmed: a charger or cable that cannot
        // enter EPR sends only the SPR block after `entry pd`.
        let port = FakePort::default()
            .on("pdm open", OPEN_REPLY)
            .on("entry pd", &spr_only_reply())
            .on("pdm close", b"ok\n");
        let mut s = open(&port);
        let n = pd::enter(&mut s).expect("entry pd");
        assert_eq!(n.offers.len(), 4);
        assert!(!pd::epr_entered(&n.offers));
        assert_eq!(n.vbus.voltage_mv, 5191);
        // SPR list: 20 V at 4.7 A.
        assert_eq!(n.offers[3].max_current_ma, Some(4700));
        let before = port.writes().len();
        let conf =
            HighVoltageConfirmation::for_command(&powerz_protocol::pdm::Command::PdRequest {
                position: 8,
                voltage_mv: None,
                current_ma: None,
            });
        let r = pd::request(
            &mut s,
            &n.offers,
            &Request::Fixed {
                position: 8,
                current_ma: None,
            },
            Some(conf),
        );
        assert!(
            matches!(r, Err(PdError::EprUnavailable { position: 8 })),
            "{r:?}"
        );
        assert_eq!(port.writes().len(), before);
        let msg = PdError::EprUnavailable { position: 8 }.to_string();
        assert!(msg.contains("EPR"), "{msg}");
    }

    // ---- AC-DGSQ6P.4: settings ----

    #[test]
    fn settings_are_read_with_pdm_parm() {
        let port = epr_port();
        let mut s = open(&port);
        let got = pd::read_settings(&mut s).unwrap();
        assert!(port.writes().contains(&"pdm parm".to_string()));
        assert_eq!(
            got,
            PdSettings {
                pd_type: PdType::Auto,
                emarker: Emarker::Epr240W,
                sink: SinkCaps::Pps3A,
            }
        );
    }

    #[test]
    fn settings_are_written_with_pdm_set_and_read_back() {
        let port = FakePort::default()
            .on("pdm open", OPEN_REPLY)
            .on("pdm parm", b"type=1,em=0,sink=1\n")
            .on("pdm close", b"ok\n");
        let mut s = open(&port);
        let want = PdSettings {
            pd_type: PdType::Spr,
            emarker: Emarker::Off,
            sink: SinkCaps::Pps5A,
        };
        let got = pd::write_settings(&mut s, &want).unwrap();
        let w = port.writes();
        let set = w
            .iter()
            .position(|c| c == "pdm set type=1,em=0,sink=1")
            .expect("pdm set written");
        let parm = w.iter().rposition(|c| c == "pdm parm").expect("read back");
        assert!(set < parm);
        assert_eq!(got, want);
    }

    // ---- AC-DGSQ6P.5: hard reset ----

    #[test]
    fn hard_reset_returns_to_5v_and_reports_new_offers() {
        let port = epr_port();
        let mut s = open(&port);
        let offers = pd::enter(&mut s).unwrap().offers;
        let req = Request::Fixed {
            position: 8,
            current_ma: None,
        };
        pd::request(&mut s, &offers, &req, confirmed(&offers, &req)).unwrap();
        let n = pd::hard_reset(&mut s).unwrap();
        assert_eq!(port.writes().last().map(String::as_str), Some("pd hrst"));
        assert_eq!(n.offers.len(), 6);
        assert!(pd::epr_entered(&n.offers));
        assert!(n.vbus.voltage_mv <= 5500, "{:?}", n.vbus);
        // The session's capabilities follow the re-offer too.
        assert_eq!(pd::offers(s.capabilities()), n.offers);
    }
}
