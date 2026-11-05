//! AC-AXHK2J.4: whether the meter a pipe was opened on is still listed. With
//! a known serial, the product ID, bus and address alone do not decide:
//! another meter of the same model can land on the freed address.

use powerz_protocol::transport::discovery::{DeviceEntry, Model, still_listed};

fn entry(pid: u16, serial: Option<&str>, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid,
        serial: serial.map(str::to_owned),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: bus.to_owned(),
        address,
    }
}

const PID: u16 = 0x0063;

#[test]
fn the_same_meter_at_the_same_place_is_listed() {
    let pipe = entry(PID, Some("A"), "1", 7);
    assert!(still_listed(&pipe, &[entry(PID, Some("A"), "1", 7)]));
}

#[test]
fn another_meter_on_the_same_bus_address_and_pid_is_not_this_one() {
    let pipe = entry(PID, Some("A"), "1", 7);
    assert!(!still_listed(&pipe, &[entry(PID, Some("B"), "1", 7)]));
}

#[test]
fn a_device_without_a_serial_there_is_not_this_one_when_its_serial_is_known() {
    let pipe = entry(PID, Some("A"), "1", 7);
    assert!(!still_listed(&pipe, &[entry(PID, None, "1", 7)]));
}

#[test]
fn the_same_meter_at_a_new_address_left_this_pipe_behind() {
    let pipe = entry(PID, Some("A"), "1", 7);
    assert!(!still_listed(&pipe, &[entry(PID, Some("A"), "1", 9)]));
}

#[test]
fn without_a_known_serial_the_place_and_pid_still_decide() {
    let pipe = entry(PID, None, "1", 7);
    assert!(still_listed(&pipe, &[entry(PID, Some("A"), "1", 7)]));
    assert!(!still_listed(&pipe, &[entry(PID, None, "1", 8)]));
    assert!(!still_listed(&pipe, &[entry(PID, None, "2", 7)]));
    assert!(!still_listed(&pipe, &[entry(0x0064, None, "1", 7)]));
}

#[test]
fn an_empty_list_lists_nothing() {
    assert!(!still_listed(&entry(PID, Some("A"), "1", 7), &[]));
}
