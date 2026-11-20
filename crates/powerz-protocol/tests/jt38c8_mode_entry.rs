//! FR JT38C8 (AC .1, .2): the device mode of a device-list entry, which is
//! built from the OS device list without opening anything. The entry now
//! carries the vendor interface's subclass and whether the device has a HID
//! interface, next to the vendor-interface string, so the 2.1 bootloader
//! (one vendor interface named after the product, no HID) is told apart
//! from the application and the 2.5 bootloader.
//!
//! Every serial, bus and address is synthetic.

use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::transport::discovery::{DeviceEntry, UsbDeviceInfo, filter_devices};

/// One OS-list record of a KM003C with this vendor interface.
fn listed(vendor_string: Option<&str>, vendor_subclass: Option<u8>, has_hid: bool) -> DeviceEntry {
    let info = UsbDeviceInfo {
        vid: 0x5FC9,
        pid: 0x0063,
        serial: Some("SERIAL-JT38-0001".to_owned()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: vendor_string.map(str::to_owned),
        vendor_subclass,
        has_hid,
        bus: "1".to_owned(),
        address: 9,
    };
    let mut entries = filter_devices(&[info]);
    assert_eq!(entries.len(), 1, "the POWER-Z device is kept");
    entries.remove(0)
}

// ---------------------------------------------------------------- AC-JT38C8.1

#[test]
fn jt38c8_the_2_1_bootloader_entry_is_the_old_bootloader() {
    let e = listed(Some("POWER-Z KM003C"), Some(0xFF), false);
    assert_eq!(e.mode(), DeviceMode::LegacyBootloader);
}

#[test]
fn jt38c8_the_entry_keeps_the_vendor_subclass_and_the_hid_flag_from_the_os_list() {
    let e = listed(Some("POWER-Z KM003C"), Some(0xFF), false);
    assert_eq!(e.vendor_subclass, Some(0xFF));
    assert!(!e.has_hid);
    let app = listed(Some("APP"), Some(0x00), true);
    assert!(app.has_hid);
}

#[test]
fn jt38c8_the_old_bootloader_is_named_legacy_bootloader() {
    assert_eq!(DeviceMode::LegacyBootloader.name(), "legacy_bootloader");
}

#[test]
fn jt38c8_a_product_named_entry_with_hid_is_not_the_old_bootloader() {
    let e = listed(Some("POWER-Z KM003C"), Some(0x00), true);
    assert_ne!(e.mode(), DeviceMode::LegacyBootloader);
}

// ---------------------------------------------------------------- AC-JT38C8.2

#[test]
fn jt38c8_the_2_5_bootloader_entry_stays_bootloader() {
    let e = listed(Some("BOOT"), Some(0x00), false);
    assert_eq!(e.mode(), DeviceMode::Bootloader);
}

#[test]
fn jt38c8_the_application_entry_stays_application() {
    let e = listed(Some("APP"), Some(0x00), true);
    assert_eq!(e.mode(), DeviceMode::Application);
}

#[test]
fn jt38c8_an_entry_whose_vendor_interface_has_no_string_stays_unknown() {
    let e = listed(None, Some(0xFF), false);
    assert_eq!(e.mode(), DeviceMode::Unknown);
}
