//! AC-4PJRVM.4: a meter can be connected while another one flashes firmware;
//! the flashing meter stays locked. Simulators only; synthetic serials.

#![allow(dead_code)]

mod common;

use common::*;

include!("common/two_meters.inc.rs");

#[test]
fn another_meter_connects_while_one_flashes_and_the_flashing_one_stays_locked() {
    let mut rig = Rig::new();
    let (rx_a, _) = stream(&mut rig, METER_A);
    let lease = rig.m.begin_flash(METER_A).expect("flash meter A");
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });

    let (rx_b, sink) = channel_sink();
    rig.m
        .connect(METER_B, 50, sink)
        .unwrap_or_else(|e| panic!("meter B refused while meter A flashes: {}", e.message));
    recv_until(&rx_b, is_samples);
    assert_eq!(rig.m.connected_ids(), vec![METER_B.to_owned()]);

    let (_rx, sink) = channel_sink();
    let refused = rig
        .m
        .connect(METER_A, 50, sink)
        .expect_err("the flashing meter stays locked");
    assert!(
        refused.message.to_lowercase().contains("firmware"),
        "{}",
        refused.message
    );
    drop(lease);
    rig.m.disconnect_for(METER_B).unwrap();
}

#[test]
fn a_second_update_stays_refused_while_one_runs() {
    let mut rig = Rig::new();
    let (_rx_a, _) = stream(&mut rig, METER_A);
    let lease = rig.m.begin_flash(METER_A).expect("flash meter A");
    let (_rx_b, sink) = channel_sink();
    rig.m
        .connect(METER_B, 50, sink)
        .unwrap_or_else(|e| panic!("meter B refused: {}", e.message));
    assert!(rig.m.begin_flash(METER_B).is_err(), "one update at a time");
    drop(lease);
    rig.m.disconnect_for(METER_B).unwrap();
}
