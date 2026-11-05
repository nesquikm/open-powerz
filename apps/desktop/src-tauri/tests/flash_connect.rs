//! AC-4PJRVM.4: a meter can be connected while another one flashes firmware;
//! the flashing meter stays locked. AC-AXHK2M.3: attach and `devices_changed`
//! keep working for the other meters during an update. Simulators only;
//! synthetic serials.

#![allow(dead_code)]

mod common;

use common::*;

include!("common/two_meters.inc.rs");

#[test]
fn another_meter_connects_while_one_flashes_and_the_flashing_one_stays_locked() {
    let mut rig = Rig::new();
    let (rx_a, _) = stream(&mut rig, METER_A);
    let holds = rig.m.flash_holds(METER_A);
    let lease = rig.m.begin_flash(METER_A, &holds).expect("flash meter A");
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
    let holds = rig.m.flash_holds(METER_A);
    let lease = rig.m.begin_flash(METER_A, &holds).expect("flash meter A");
    let (_rx_b, sink) = channel_sink();
    rig.m
        .connect(METER_B, 50, sink)
        .unwrap_or_else(|e| panic!("meter B refused: {}", e.message));
    assert!(
        rig.m.begin_flash(METER_B, &[]).is_err(),
        "one update at a time"
    );
    drop(lease);
    rig.m.disconnect_for(METER_B).unwrap();
}

// ---- AC-AXHK2M.3: an update holds its target, not the other meters ----

/// Meter A streams and goes away, so the app waits for it; meter B is free.
fn waiting_for_a(rig: &mut Rig) -> mpsc::Receiver<BackendEvent> {
    let (rx_a, _) = stream(rig, METER_A);
    rig.unplug_a();
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    assert!(rig.m.waiting_for().is_some(), "nothing waits for meter A");
    rx_a
}

#[test]
fn an_awaited_meter_reconnects_while_another_one_flashes() {
    let mut rig = Rig::new();
    let rx_a = waiting_for_a(&mut rig);
    let lease = rig.m.begin_flash(METER_B, &[]).expect("flash meter B");

    assert!(
        rig.replug_a(),
        "the awaited meter was not reconnected during the update of another"
    );
    recv_until(&rx_a, is_samples);
    assert!(rig.m.flashing(), "the update still runs");
    drop(lease);
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn devices_changed_never_opens_the_update_target() {
    let mut rig = Rig::new();
    let _rx_a = waiting_for_a(&mut rig);
    let opens = rig.opens_of(SERIAL_B);
    let lease = rig.m.begin_flash(METER_B, &[]).expect("flash meter B");

    let list = rig.m.list_devices();
    assert!(
        !rig.m.devices_changed(&list),
        "meter A is still gone, so nothing starts"
    );
    assert_eq!(rig.opens_of(SERIAL_B), opens, "the target was opened");
    assert!(rig.replug_a(), "meter A came back");
    assert_eq!(rig.opens_of(SERIAL_B), opens, "the target was opened");
    drop(lease);
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn a_meter_that_cannot_be_told_from_the_target_stays_waiting() {
    let mut rig = Rig::new();
    let _rx_a = waiting_for_a(&mut rig);
    // The target reports no serial: meter A may be that meter at a new address.
    rig.attached.lock().unwrap().iter_mut().for_each(|e| {
        if e.serial.as_deref() == Some(SERIAL_B) {
            e.serial = None;
        }
    });
    let lease = rig.m.begin_flash(METER_B, &[]).expect("flash meter B");

    assert!(
        !rig.replug_a(),
        "a meter that may be the target was connected"
    );
    assert!(rig.m.connected_ids().is_empty(), "nothing may run");
    drop(lease);
}

#[test]
fn a_page_attaches_to_another_meter_during_an_update() {
    let mut rig = Rig::new();
    let (rx_a, _) = stream(&mut rig, METER_A);
    let (_rx_b, _) = stream(&mut rig, METER_B);
    let holds = rig.m.flash_holds(METER_A);
    let lease = rig.m.begin_flash(METER_A, &holds).expect("flash meter A");
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });

    assert_eq!(rig.m.connected_ids(), vec![METER_B.to_owned()]);
    assert!(
        rig.m.attacher_for(METER_A).is_none(),
        "the update's target has no session to attach to"
    );
    let (rx, sink) = channel_sink();
    let attached = rig
        .m
        .attacher_for(METER_B)
        .expect("meter B still runs")
        .attach(sink)
        .expect("meter B's session is reported");
    assert_eq!(attached.device_id, METER_B);
    recv_until(&rx, is_samples);
    drop(lease);
    rig.m.disconnect_for(METER_B).unwrap();
}
