//! FR JQ971R, AC .1: opening a trigger session stops a running PD capture,
//! and the capture-stopped event says so. Wire shape pinned here (what the UI
//! reads, as JSON):
//!
//! - `{ "type": "pd_state", "data": { "capturing": false, "reason": "trigger" } }`
//!   when the trigger install stopped the capture;
//! - `{ "type": "pd_state", "data": { "capturing": false } }` (no `reason`
//!   key) for a stop the user asked for, a disconnect or an unplug.
//!
//! Simulators only; every serial here is synthetic.

#![allow(dead_code)]

mod common;

use common::*;
use open_powerz_desktop_lib::trigger::TriggerWorker;
use powerz_sim::{SimCharger, SimPdm};
use serde_json::Value;

include!("common/two_meters.inc.rs");

fn json(e: &BackendEvent) -> Value {
    serde_json::to_value(e).expect("event serializes")
}

fn is_pd_stopped(e: &BackendEvent) -> bool {
    let v = json(e);
    v["type"] == "pd_state" && v["data"]["capturing"] == false
}

fn is_pd_started(e: &BackendEvent) -> bool {
    let v = json(e);
    v["type"] == "pd_state" && v["data"]["capturing"] == true
}

/// The `data` object of the first capture-stopped event in `events`.
fn stopped_data(events: &[BackendEvent]) -> Value {
    events
        .iter()
        .find(|e| is_pd_stopped(e))
        .map(|e| json(e)["data"].clone())
        .expect("a capture-stopped event")
}

/// Meter A streaming with its PD capture running.
fn capturing_a() -> (Rig, mpsc::Receiver<BackendEvent>) {
    let mut rig = Rig::new();
    let (rx_a, _) = stream(&mut rig, METER_A);
    rig.m
        .control_for(METER_A)
        .expect("meter A's control")
        .start_pd()
        .expect("PD capture on meter A");
    recv_until(&rx_a, is_pd_started);
    (rig, rx_a)
}

fn open_trigger(rig: &mut Rig) -> SimPdm {
    let pdm = SimPdm::new(SimCharger::Epr140W, SimClock::new(T0));
    let w = TriggerWorker::spawn(pdm.open_port(), SimClock::new(T0)).expect("trigger opens");
    rig.m
        .install_trigger_for(METER_A, w)
        .expect("trigger on meter A");
    pdm
}

#[test]
fn a_trigger_session_stopping_the_capture_reports_the_reason_trigger() {
    let (mut rig, rx_a) = capturing_a();
    let _pdm = open_trigger(&mut rig);
    let got = recv_until(&rx_a, is_pd_stopped);
    let data = stopped_data(&got);
    assert_eq!(
        data["reason"], "trigger",
        "the stop by the trigger session carries no reason: {data}"
    );
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn a_trigger_session_without_a_running_capture_sends_no_stop() {
    let mut rig = Rig::new();
    let (rx_a, _) = stream(&mut rig, METER_A);
    let _pdm = open_trigger(&mut rig);
    recv_until(&rx_a, is_samples);
    assert!(
        !drain(&rx_a).iter().any(is_pd_stopped),
        "a capture that never ran was reported stopped"
    );
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn a_stop_the_user_asked_for_carries_no_reason() {
    let (mut rig, rx_a) = capturing_a();
    rig.m
        .control_for(METER_A)
        .expect("meter A's control")
        .stop_pd()
        .expect("the capture stops");
    let got = recv_until(&rx_a, is_pd_stopped);
    let data = stopped_data(&got);
    assert!(
        data.get("reason").is_none(),
        "a user stop carries a reason: {data}"
    );
    // A later trigger session with nothing to stop adds no reasoned stop.
    let _pdm = open_trigger(&mut rig);
    recv_until(&rx_a, is_samples);
    assert!(
        !drain(&rx_a).iter().any(is_pd_stopped),
        "the trigger reported stopping a capture that was not running"
    );
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn a_disconnect_while_capturing_carries_no_reason() {
    let (mut rig, rx_a) = capturing_a();
    rig.m.disconnect_for(METER_A).unwrap();
    let got = recv_until(&rx_a, is_pd_stopped);
    let data = stopped_data(&got);
    assert!(
        data.get("reason").is_none(),
        "a disconnect stop carries a reason: {data}"
    );
}

#[test]
fn an_unplug_while_capturing_carries_no_reason() {
    let (mut rig, rx_a) = capturing_a();
    rig.unplug_a();
    let got = recv_until(&rx_a, is_pd_stopped);
    let data = stopped_data(&got);
    assert!(
        data.get("reason").is_none(),
        "an unplug stop carries a reason: {data}"
    );
}

#[test]
fn a_start_after_the_trigger_stop_carries_no_reason() {
    let (mut rig, rx_a) = capturing_a();
    let _pdm = open_trigger(&mut rig);
    recv_until(&rx_a, is_pd_stopped);
    if let Some(t) = rig.m.take_trigger_for(METER_A) {
        let _ = t.close();
    }
    let _ = rig
        .m
        .control_for(METER_A)
        .expect("meter A's control")
        .start_pd();
    let got = recv_until(&rx_a, is_pd_started);
    let started = got
        .iter()
        .rev()
        .find(|e| is_pd_started(e))
        .map(|e| json(e)["data"].clone())
        .expect("a capture-started event");
    assert!(
        started.get("reason").is_none(),
        "a start carries a reason: {started}"
    );
    rig.m.disconnect_for(METER_A).unwrap();
}
