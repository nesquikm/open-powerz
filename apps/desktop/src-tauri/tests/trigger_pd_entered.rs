//! AC-AXHK2P.13, the backend side: the panel learns from the backend whether
//! PD is entered in this trigger session (`TriggerControl::pd_entered`, the
//! `trigger_pd_entered` command), since Read cable is off until it is. The
//! answer is the session's own state: a PD listing or a PD request enters
//! PD, any other protocol's listing leaves it. Asking writes nothing.

mod common;

use common::*;
use open_powerz_desktop_lib::trigger::{TriggerProtocol, TriggerRequest, TriggerWorker};
use powerz_sim::{SimCharger, SimPdm};

fn session(charger: SimCharger) -> (SimPdm, TriggerWorker) {
    let clock = SimClock::new(T0);
    let pdm = SimPdm::new(charger, clock.clone());
    let worker = TriggerWorker::spawn(pdm.open_port(), clock).expect("trigger session");
    (pdm, worker)
}

const PD_5V: TriggerRequest = TriggerRequest::Pd {
    position: 1,
    voltage_mv: None,
    current_ma: None,
};

#[test]
fn a_fresh_session_has_not_entered_pd() {
    let (_pdm, worker) = session(SimCharger::Epr140W);
    assert!(!worker.control().pd_entered().expect("answers"));
    worker.close().expect("close");
}

#[test]
fn a_pd_listing_enters_pd() {
    let (_pdm, worker) = session(SimCharger::Epr140W);
    let c = worker.control();
    c.list(TriggerProtocol::Pd).expect("list pd");
    assert!(c.pd_entered().expect("answers"));
    worker.close().expect("close");
}

#[test]
fn another_protocol_listed_after_pd_leaves_it() {
    let (_pdm, worker) = session(SimCharger::MultiProtocol);
    let c = worker.control();
    c.list(TriggerProtocol::Pd).expect("list pd");
    c.list(TriggerProtocol::Qc).expect("list qc");
    assert!(
        !c.pd_entered().expect("answers"),
        "QC was entered last, so the cable read would be refused"
    );
    worker.close().expect("close");
}

#[test]
fn a_pd_request_enters_pd_again() {
    let (_pdm, worker) = session(SimCharger::MultiProtocol);
    let c = worker.control();
    c.list(TriggerProtocol::Pd).expect("list pd");
    c.list(TriggerProtocol::Qc).expect("list qc");
    c.request(PD_5V, false).expect("5 V");
    assert!(c.pd_entered().expect("answers"));
    worker.close().expect("close");
}

#[test]
fn the_answer_agrees_with_the_cable_read() {
    let (_pdm, worker) = session(SimCharger::MultiProtocol);
    let c = worker.control();
    c.list(TriggerProtocol::Pd).expect("list pd");
    c.list(TriggerProtocol::Qc).expect("list qc");
    assert!(!c.pd_entered().expect("answers"));
    assert!(c.cable_read().is_err(), "refused with PD not entered");
    c.request(PD_5V, false).expect("5 V");
    assert!(c.pd_entered().expect("answers"));
    assert!(c.cable_read().is_ok(), "the read goes out with PD entered");
    worker.close().expect("close");
}

#[test]
fn asking_writes_nothing() {
    let (pdm, worker) = session(SimCharger::Epr140W);
    let c = worker.control();
    c.list(TriggerProtocol::Pd).expect("list pd");
    let before = pdm.writes().len();
    for _ in 0..3 {
        c.pd_entered().expect("answers");
    }
    assert_eq!(pdm.writes().len(), before, "{:#?}", pdm.writes());
    worker.close().expect("close");
}
