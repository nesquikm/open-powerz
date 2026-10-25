//! The pure half of reading a meter's USB identity (FR V9SK56, AC .6):
//! building a [`UsbIdentity`] from descriptor-level facts, and picking the
//! descriptor that belongs to a [`DeviceEntry`] by vendor ID, product ID,
//! bus, address and (when the entry has one) serial. The thin `nusb` reader
//! that produces the descriptors is not tested here. Every value is
//! synthetic.

use powerz_protocol::transport::discovery::{
    DeviceEntry, Model, UsbDescriptor, UsbIdentity, UsbInterface, match_device,
};

const SERIAL_A: &str = "SERIAL-0001";
const SERIAL_B: &str = "SERIAL-0002";

fn iface(number: u8, class: u8, string: Option<&str>) -> UsbInterface {
    UsbInterface {
        number,
        class,
        subclass: 0,
        protocol: 0,
        string: string.map(str::to_owned),
    }
}

fn descriptor(pid: u16, serial: Option<&str>, bus: &str, address: u8) -> UsbDescriptor {
    UsbDescriptor {
        vid: 0x5FC9,
        pid,
        serial: serial.map(str::to_owned),
        product: Some("POWER-Z KM003C".to_owned()),
        bus: bus.to_owned(),
        address,
        interfaces: vec![
            iface(0, 0xFF, Some("APP")),
            iface(1, 0x02, Some("APP")),
            iface(2, 0x0A, None),
            iface(3, 0x03, None),
        ],
    }
}

fn entry(pid: u16, serial: Option<&str>, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::from_pid(pid),
        pid,
        serial: serial.map(str::to_owned),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: bus.to_owned(),
        address,
    }
}

// ---------------------------------------------------------------- mapping

#[test]
fn identity_from_descriptor_keeps_ids_product_and_every_interface_in_order() {
    let d = descriptor(0x0063, Some(SERIAL_A), "1", 7);
    assert_eq!(
        UsbIdentity::from_descriptor(&d),
        UsbIdentity {
            vid: 0x5FC9,
            pid: 0x0063,
            product: Some("POWER-Z KM003C".to_owned()),
            interfaces: vec![
                iface(0, 0xFF, Some("APP")),
                iface(1, 0x02, Some("APP")),
                iface(2, 0x0A, None),
                iface(3, 0x03, None),
            ],
        }
    );
}

#[test]
fn identity_from_a_bootloader_descriptor_has_its_single_boot_interface() {
    let d = UsbDescriptor {
        product: None,
        interfaces: vec![UsbInterface {
            number: 0,
            class: 0xFF,
            subclass: 0x01,
            protocol: 0x02,
            string: Some("BOOT".to_owned()),
        }],
        ..descriptor(0x0063, None, "2", 3)
    };
    let id = UsbIdentity::from_descriptor(&d);
    assert_eq!(id.product, None);
    assert_eq!(id.interfaces.len(), 1);
    assert_eq!(id.interfaces[0].subclass, 0x01);
    assert_eq!(id.interfaces[0].protocol, 0x02);
    assert_eq!(id.interfaces[0].string.as_deref(), Some("BOOT"));
}

// ---------------------------------------------------------------- matching

#[test]
fn match_device_picks_the_descriptor_with_the_same_ids_bus_address_and_serial() {
    let devices = [
        descriptor(0x0063, Some(SERIAL_B), "1", 6),
        descriptor(0x0063, Some(SERIAL_A), "1", 7),
        descriptor(0x0061, Some(SERIAL_A), "2", 7),
    ];
    let found = match_device(&entry(0x0063, Some(SERIAL_A), "1", 7), &devices);
    assert_eq!(found, Some(&devices[1]));
}

#[test]
fn match_device_ignores_serial_when_the_entry_has_none() {
    let devices = [descriptor(0x0063, Some(SERIAL_A), "1", 7)];
    let found = match_device(&entry(0x0063, None, "1", 7), &devices);
    assert_eq!(found, Some(&devices[0]));
}

#[test]
fn match_device_refuses_a_different_product_id_at_the_same_bus_and_address() {
    // The meter was unplugged and something else took its address.
    let devices = [descriptor(0x0061, Some(SERIAL_A), "1", 7)];
    assert_eq!(
        match_device(&entry(0x0063, Some(SERIAL_A), "1", 7), &devices),
        None
    );
}

#[test]
fn match_device_refuses_a_different_serial_at_the_same_bus_and_address() {
    let devices = [descriptor(0x0063, Some(SERIAL_B), "1", 7)];
    assert_eq!(
        match_device(&entry(0x0063, Some(SERIAL_A), "1", 7), &devices),
        None
    );
}

#[test]
fn match_device_refuses_a_device_without_a_serial_when_the_entry_has_one() {
    let devices = [descriptor(0x0063, None, "1", 7)];
    assert_eq!(
        match_device(&entry(0x0063, Some(SERIAL_A), "1", 7), &devices),
        None
    );
}

#[test]
fn match_device_refuses_another_vendor_id() {
    let devices = [UsbDescriptor {
        vid: 0x1234,
        ..descriptor(0x0063, Some(SERIAL_A), "1", 7)
    }];
    assert_eq!(
        match_device(&entry(0x0063, Some(SERIAL_A), "1", 7), &devices),
        None
    );
}

#[test]
fn match_device_with_no_device_at_that_bus_and_address_is_none() {
    let devices = [
        descriptor(0x0063, Some(SERIAL_A), "1", 8),
        descriptor(0x0063, Some(SERIAL_A), "2", 7),
    ];
    assert_eq!(
        match_device(&entry(0x0063, Some(SERIAL_A), "1", 7), &devices),
        None
    );
    assert_eq!(match_device(&entry(0x0063, None, "1", 7), &[]), None);
}
