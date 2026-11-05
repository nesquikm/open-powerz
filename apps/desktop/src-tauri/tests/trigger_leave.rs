//! AC-AXHK2P.15: leaving the Trigger tab sends one way back to 5 V, then
//! `pdm close`. The tab's leave is two backend calls, as the Tauri commands
//! run them: `trigger_back_to_5v { leaving: true }` (`leave_view`) and then
//! `trigger_close` (`TriggerWorker::close`). The test counts the lines the
//! simulated meter's serial port was written.

mod common;

use common::*;
use open_powerz_desktop_lib::trigger::{TriggerProtocol, TriggerRequest, TriggerWorker};
use powerz_sim::{SimCharger, SimPdm};

const BACK: &str = "pd req=1";
const CLOSE: &str = "pdm close";

fn pd(position: u8) -> TriggerRequest {
    TriggerRequest::Pd {
        position,
        voltage_mv: None,
        current_ma: None,
    }
}

/// The lines written so far, trimmed.
fn lines(pdm: &SimPdm) -> Vec<String> {
    pdm.writes()
        .iter()
        .flat_map(|w| w.lines().map(|l| l.trim().to_owned()).collect::<Vec<_>>())
        .filter(|l| !l.is_empty())
        .collect()
}

/// A trigger session with PD entered and `position` requested.
fn at(position: u8, confirmed: bool) -> (SimPdm, TriggerWorker) {
    let clock = SimClock::new(T0);
    let pdm = SimPdm::new(SimCharger::Epr140W, clock.clone());
    let worker = TriggerWorker::spawn(pdm.open_port(), clock).expect("trigger session");
    let c = worker.control();
    c.list(TriggerProtocol::Pd).expect("list pd");
    c.request(pd(position), confirmed).expect("request");
    (pdm, worker)
}

/// What the tab's leave writes: the lines after `before`.
fn leave(pdm: &SimPdm, worker: TriggerWorker) -> Vec<String> {
    let before = lines(pdm).len();
    worker.control().leave_view().expect("back to 5 V");
    worker.close().expect("close");
    lines(pdm)[before..].to_vec()
}

fn assert_one_way_back_then_close(written: &[String]) {
    let backs = written.iter().filter(|l| l.as_str() == BACK).count();
    assert_eq!(backs, 1, "one way back to 5 V: {written:#?}");
    let closes = written.iter().filter(|l| l.as_str() == CLOSE).count();
    assert_eq!(closes, 1, "one pdm close: {written:#?}");
    let back = written.iter().position(|l| l == BACK).unwrap();
    let close = written.iter().position(|l| l == CLOSE).unwrap();
    assert!(back < close, "the way back comes first: {written:#?}");
    assert_eq!(
        written.last().map(String::as_str),
        Some(CLOSE),
        "{written:#?}"
    );
}

#[test]
fn leaving_at_9v_sends_one_way_back_then_pdm_close() {
    let (pdm, worker) = at(2, true);
    assert!(pdm.vbus_mv() > 8000, "the charger is at 9 V");
    let written = leave(&pdm, worker);
    assert_one_way_back_then_close(&written);
    assert!(pdm.vbus_mv() < 6000, "back at 5 V: {} mV", pdm.vbus_mv());
}

#[test]
fn leaving_at_5v_sends_one_way_back_then_pdm_close() {
    let (pdm, worker) = at(1, false);
    let written = leave(&pdm, worker);
    assert_one_way_back_then_close(&written);
}

#[test]
fn leaving_after_back_to_5v_still_sends_one_way_back_then_pdm_close() {
    let (pdm, worker) = at(2, true);
    worker.control().back_to_5v().expect("Back to 5 V");
    let written = leave(&pdm, worker);
    assert_one_way_back_then_close(&written);
}

/// A close with no leave before it (a disconnect, a closing app) still
/// goes back to 5 V first: one way back, then `pdm close`.
#[test]
fn a_close_on_its_own_still_goes_back_to_5v_once() {
    let (pdm, worker) = at(2, true);
    let before = lines(&pdm).len();
    worker.close().expect("close");
    let written = lines(&pdm)[before..].to_vec();
    assert_one_way_back_then_close(&written);
}
