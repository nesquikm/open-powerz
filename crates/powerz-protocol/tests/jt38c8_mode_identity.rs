//! FR JT38C8 (AC .1, .2, .4): the device mode read from a meter's full USB
//! identity, over the four known descriptor shapes. The 2.1 bootloader shape
//! is the one from the field report (bootloader 2.1, app 1.9.5): one
//! vendor-class interface, subclass 0xFF, named after the product, no HID.
//!
//! These tests use only the API that existed before the fix (the mode is
//! compared by its name), so they run unchanged on the code with the bug.
//! Every value is synthetic apart from the descriptor shapes.

use powerz_protocol::transport::discovery::{UsbIdentity, UsbInterface};

fn iface(number: u8, class: u8, subclass: u8, string: Option<&str>) -> UsbInterface {
    UsbInterface {
        number,
        class,
        subclass,
        protocol: 0,
        string: string.map(str::to_owned),
    }
}

fn identity(interfaces: Vec<UsbInterface>) -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces,
    }
}

/// Bootloader 2.1 `[device]` (field report): one interface, class 0xFF,
/// subclass 0xFF, string `POWER-Z KM003C`.
fn bootloader_21() -> UsbIdentity {
    identity(vec![iface(0, 0xFF, 0xFF, Some("POWER-Z KM003C"))])
}

/// Bootloader 2.5 `[device]`: vendor 0xFF subclass 0x00 `BOOT`, CDC 0x02
/// `BOOT`, CDC data 0x0A.
fn bootloader_25() -> UsbIdentity {
    identity(vec![
        iface(0, 0xFF, 0x00, Some("BOOT")),
        iface(1, 0x02, 0x02, Some("BOOT")),
        iface(2, 0x0A, 0x00, None),
    ])
}

/// Application `[device]`: vendor `APP`, CDC `APP`, CDC data, HID.
fn application() -> UsbIdentity {
    identity(vec![
        iface(0, 0xFF, 0x00, Some("APP")),
        iface(1, 0x02, 0x02, Some("APP")),
        iface(2, 0x0A, 0x00, None),
        iface(3, 0x03, 0x00, None),
    ])
}

/// A vendor interface that reports no string.
fn vendor_without_string() -> UsbIdentity {
    identity(vec![iface(0, 0xFF, 0xFF, None)])
}

fn mode_line(id: &UsbIdentity) -> String {
    let text = id.to_string();
    text.lines()
        .find(|l| l.starts_with("Mode:"))
        .unwrap_or_else(|| panic!("no Mode line in:\n{text}"))
        .to_owned()
}

// ---------------------------------------------------------------- AC-JT38C8.1

#[test]
fn jt38c8_the_field_reported_2_1_bootloader_identity_is_the_old_bootloader() {
    assert_eq!(bootloader_21().mode().name(), "legacy_bootloader");
}

#[test]
fn jt38c8_a_product_named_vendor_string_without_hid_is_the_old_bootloader() {
    // Any string that is neither APP nor names boot/dfu counts, not just
    // the one product name seen in the field.
    for s in ["POWER-Z KM002C", "KM003C", "Power-Z"] {
        let id = identity(vec![iface(0, 0xFF, 0xFF, Some(s))]);
        assert_eq!(id.mode().name(), "legacy_bootloader", "{s:?}");
    }
}

#[test]
fn jt38c8_a_product_named_vendor_string_beside_a_hid_interface_is_not_the_old_bootloader() {
    let id = identity(vec![
        iface(0, 0xFF, 0x00, Some("POWER-Z KM003C")),
        iface(1, 0x03, 0x00, None),
    ]);
    assert_ne!(id.mode().name(), "legacy_bootloader");
}

// ---------------------------------------------------------------- AC-JT38C8.2

#[test]
fn jt38c8_the_2_5_bootloader_identity_stays_bootloader() {
    assert_eq!(bootloader_25().mode().name(), "bootloader");
}

#[test]
fn jt38c8_the_application_identity_stays_application() {
    assert_eq!(application().mode().name(), "application");
}

#[test]
fn jt38c8_a_vendor_interface_without_a_string_stays_unknown() {
    assert_eq!(vendor_without_string().mode().name(), "unknown");
}

// ---------------------------------------------------------------- AC-JT38C8.4

#[test]
fn jt38c8_the_2_1_identity_prints_its_mode_as_the_old_bootloader() {
    let line = mode_line(&bootloader_21());
    assert!(
        line.contains("bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)"),
        "{line}"
    );
}

#[test]
fn jt38c8_the_other_shapes_print_their_mode_by_name_as_before() {
    assert!(mode_line(&bootloader_25()).ends_with(" bootloader"));
    assert!(mode_line(&application()).ends_with(" application"));
    assert!(mode_line(&vendor_without_string()).ends_with(" unknown"));
}
