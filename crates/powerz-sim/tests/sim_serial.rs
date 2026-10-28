//! The simulator's serial override (FR 1HHT03, AC .6): a second simulated
//! meter reports a serial of its own, so two can run side by side. Every
//! serial here is synthetic.

mod common;

use common::*;
use powerz_protocol::session::Session;

/// A made-up serial for the second simulated meter.
const OTHER_SERIAL: &str = "SIM-B-0002";

#[test]
fn a_simulator_with_a_serial_override_reports_that_serial() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_serial(OTHER_SERIAL);
    let mut s = session(dev, &clock);
    s.connect().expect("CONNECT");
    let info = s.device_info().expect("device info");
    assert_eq!(info.serial, OTHER_SERIAL);
}

#[test]
fn without_an_override_the_simulator_keeps_its_serial() {
    let (mut s, _clock) = connected(Scenario::Idle);
    assert_eq!(s.device_info().unwrap().serial, powerz_sim::SIM_SERIAL);
}

#[test]
fn two_simulators_run_side_by_side_with_their_own_serials_and_data() {
    let clock = clock();
    let a = device(Persona::Bulk, Scenario::Load5v1a, &clock);
    let b = device(Persona::Bulk, Scenario::Pd9v2a, &clock).with_serial(OTHER_SERIAL);
    let mut sa: Session<_, _> = session(a, &clock);
    let mut sb: Session<_, _> = session(b, &clock);
    sa.connect().unwrap();
    sb.connect().unwrap();

    assert_eq!(sa.device_info().unwrap().serial, powerz_sim::SIM_SERIAL);
    assert_eq!(sb.device_info().unwrap().serial, OTHER_SERIAL);
    let va = sa.read_adc().unwrap().vbus_v();
    let vb = sb.read_adc().unwrap().vbus_v();
    assert!((va - 5.0).abs() < 0.1, "A reads {va}");
    assert!((vb - 9.0).abs() < 0.1, "B reads {vb}");
}
