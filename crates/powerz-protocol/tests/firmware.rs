//! Device mode, HEAD old-firmware check and firmware version warnings
//! (FR V9SK52, AC .1, .3, .4). Pure functions over strings and byte
//! vectors; no device and no simulator. Every byte vector is synthetic.

use std::time::Duration;

use powerz_protocol::decode::DecodeError;
use powerz_protocol::decode::head::{self, HeadReply};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::version::{
    self, FirmwareWarning, KNOWN_LATEST, MIN_SUPPORTED, Version,
};
use powerz_protocol::transport::Transport;
use powerz_protocol::transport::discovery::{DeviceEntry, Model, UsbDeviceInfo, filter_devices};
use powerz_protocol::transport::guard::{self, FORBIDDEN_TYPES, GuardedTransport};
use powerz_protocol::transport::mock::{MockTransport, Step};

// ---------------------------------------------------------------- AC-V9SK52.1

#[test]
fn interface_string_app_is_application_mode() {
    assert_eq!(
        DeviceMode::from_interface_string(Some("APP")),
        DeviceMode::Application
    );
    // Surrounding whitespace from the OS string descriptor is ignored.
    assert_eq!(
        DeviceMode::from_interface_string(Some(" APP ")),
        DeviceMode::Application
    );
}

#[test]
fn interface_string_naming_boot_or_dfu_is_bootloader_mode() {
    for s in [
        "BOOT",
        "Bootloader",
        "KM003C BOOT",
        "DFU",
        "dfu mode",
        "km003c-boot",
    ] {
        assert_eq!(
            DeviceMode::from_interface_string(Some(s)),
            DeviceMode::Bootloader,
            "{s:?}"
        );
    }
}

#[test]
fn missing_empty_or_unknown_interface_string_is_unknown_mode() {
    assert_eq!(DeviceMode::from_interface_string(None), DeviceMode::Unknown);
    for s in [
        "",
        "   ",
        "POWER-Z",
        "KM003C",
        "POWER-Z KM003C",
        "APPLICATION?",
        "AP",
    ] {
        assert_eq!(
            DeviceMode::from_interface_string(Some(s)),
            DeviceMode::Unknown,
            "{s:?}"
        );
    }
}

#[test]
fn device_mode_names_are_lowercase_words() {
    assert_eq!(DeviceMode::Application.name(), "application");
    assert_eq!(DeviceMode::Bootloader.name(), "bootloader");
    assert_eq!(DeviceMode::Unknown.name(), "unknown");
}

/// The iProduct string of a KM003C in application mode [device].
const PRODUCT: &str = "POWER-Z KM003C";

fn usb(pid: u16, interface: Option<&str>, address: u8) -> UsbDeviceInfo {
    UsbDeviceInfo {
        vid: 0x5FC9,
        pid,
        serial: Some(format!("SERIAL-{address:04}")),
        product: Some(PRODUCT.to_owned()),
        interface_string: interface.map(str::to_owned),
        bus: "1".to_owned(),
        address,
    }
}

#[test]
fn discovery_carries_the_interface_string_and_reports_the_mode() {
    let found = filter_devices(&[
        usb(0x0063, Some("APP"), 1),
        usb(0x0063, Some("BOOT"), 2),
        usb(0x0061, None, 3),
        usb(0x0063, Some("something else"), 4),
    ]);
    assert_eq!(found.len(), 4);
    assert_eq!(found[0].interface_string.as_deref(), Some("APP"));
    assert_eq!(found[0].product.as_deref(), Some(PRODUCT));
    assert_eq!(found[0].mode(), DeviceMode::Application);
    assert_eq!(found[1].interface_string.as_deref(), Some("BOOT"));
    assert_eq!(found[1].mode(), DeviceMode::Bootloader);
    assert_eq!(found[2].interface_string, None);
    assert_eq!(found[2].mode(), DeviceMode::Unknown);
    assert_eq!(found[3].mode(), DeviceMode::Unknown);
}

#[test]
fn device_entry_mode_follows_its_interface_string_not_the_product() {
    let entry = |product: Option<&str>, interface: Option<&str>| DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: None,
        product: product.map(str::to_owned),
        interface_string: interface.map(str::to_owned),
        bus: "2".to_owned(),
        address: 9,
    };
    assert_eq!(
        entry(Some(PRODUCT), Some("DFU")).mode(),
        DeviceMode::Bootloader
    );
    // The real meter's product string is never `APP`; the interface string
    // alone decides the mode.
    assert_eq!(
        entry(Some(PRODUCT), Some("APP")).mode(),
        DeviceMode::Application
    );
    assert_eq!(entry(Some("APP"), None).mode(), DeviceMode::Unknown);
    assert_eq!(
        entry(Some("BOOT"), Some("APP")).mode(),
        DeviceMode::Application
    );
}

// ---------------------------------------------------------------- AC-V9SK52.3

/// Synthetic HEAD reply: `40 id 00 01`, the old-firmware u32 at offset 4,
/// zero padding to 20 bytes.
fn head_reply(id: u8, field: u32) -> Vec<u8> {
    let mut f = vec![0x40, id, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

#[test]
fn head_request_is_header_and_sixteen_zero_bytes() {
    let r = head::request(0x2A);
    assert_eq!(r.len(), 20);
    assert_eq!(&r[..4], &[0x40, 0x2A, 0x00, 0x01]);
    assert!(r[4..].iter().all(|&b| b == 0), "{r:02x?}");
}

#[test]
fn head_reply_field_zero_is_normal_firmware() {
    let reply = HeadReply::decode(&head_reply(2, 0)).unwrap();
    assert_eq!(reply.old_firmware_field(), 0);
    assert!(!reply.update_required());
}

#[test]
fn head_reply_non_zero_field_means_update_required() {
    for field in [1u32, 2, 0x100, 0xFFFF_FFFF] {
        let reply = HeadReply::decode(&head_reply(2, field)).unwrap();
        assert_eq!(reply.old_firmware_field(), field);
        assert!(reply.update_required(), "field {field:#x}");
    }
}

#[test]
fn head_reply_field_is_little_endian_at_offset_4() {
    let mut f = head_reply(5, 0);
    f[4..8].copy_from_slice(&[0x78, 0x56, 0x34, 0x12]);
    assert_eq!(
        HeadReply::decode(&f).unwrap().old_firmware_field(),
        0x1234_5678
    );
}

#[test]
fn head_reply_with_extend_bit_is_still_head() {
    let mut f = head_reply(3, 7);
    f[0] = 0xC0;
    assert!(HeadReply::decode(&f).unwrap().update_required());
}

#[test]
fn head_reply_of_exactly_eight_bytes_is_enough() {
    let f = head_reply(3, 1);
    assert!(HeadReply::decode(&f[..8]).unwrap().update_required());
}

#[test]
fn short_head_reply_is_a_length_error_never_a_panic() {
    let f = head_reply(3, 1);
    for len in 0..8 {
        assert!(
            matches!(
                HeadReply::decode(&f[..len]),
                Err(DecodeError::Length { got, .. }) if got == len
            ),
            "len {len} must be a length error"
        );
    }
}

#[test]
fn reply_of_another_type_is_an_unexpected_type_error() {
    // REJECT, ACCEPT and a PUT_DATA are not HEAD replies.
    for first in [0x06u8, 0x05, 0x41, 0x44] {
        let mut f = head_reply(3, 0);
        f[0] = first;
        assert!(
            matches!(
                HeadReply::decode(&f),
                Err(DecodeError::UnexpectedType { got, .. }) if got == first & 0x7F
            ),
            "type {first:#04x} must be refused"
        );
    }
}

#[test]
fn guard_allows_head() {
    assert!(!FORBIDDEN_TYPES.contains(&0x40), "HEAD must stay allowed");
    assert_eq!(guard::check(&head::request(1)), Ok(()));
    let mut t = GuardedTransport::new(MockTransport::new(vec![Step::transact(
        head::request(1).to_vec(),
        head_reply(1, 0),
    )]));
    let reply = t
        .transact(&head::request(1), Duration::from_millis(100))
        .unwrap();
    assert_eq!(reply, head_reply(1, 0));
}

// ---------------------------------------------------------------- AC-V9SK52.4

#[test]
fn version_constants_are_the_floor_and_the_newest_known() {
    assert_eq!(MIN_SUPPORTED, Version::new(1, 7, 0));
    assert_eq!(KNOWN_LATEST, Version::new(2, 0, 7));
}

#[test]
fn version_parses_dotted_strings() {
    assert_eq!(Version::parse("2.0.6"), Some(Version::new(2, 0, 6)));
    assert_eq!(Version::parse("1.7.0"), Some(Version::new(1, 7, 0)));
    // The bootloader reports two components.
    assert_eq!(Version::parse("2.5"), Some(Version::new(2, 5, 0)));
    assert_eq!(Version::parse("3"), Some(Version::new(3, 0, 0)));
    assert_eq!(Version::parse(" 2.0.7 "), Some(Version::new(2, 0, 7)));
}

#[test]
fn unparseable_versions_are_none_never_a_panic() {
    for s in [
        "",
        " ",
        "abc",
        "2..1",
        ".1.2",
        "1.2.",
        "1.2.3.4",
        "1.-2.0",
        "99999999999.0.0",
        "\u{FFFD}\u{FFFD}",
        "2.0.6beta",
    ] {
        assert_eq!(Version::parse(s), None, "{s:?}");
    }
}

#[test]
fn versions_compare_numerically_not_as_text() {
    let v = |s| Version::parse(s).unwrap();
    assert!(v("1.10.0") > v("1.9.0"));
    assert!(v("2.0.6") < v("2.0.7"));
    assert!(v("1.6.9") < MIN_SUPPORTED);
    assert!(v("1.7") == MIN_SUPPORTED);
    assert!(v("10.0.0") > KNOWN_LATEST);
}

#[test]
fn firmware_below_1_7_0_is_too_old() {
    assert_eq!(
        version::warnings("1.6.9", false),
        vec![FirmwareWarning::TooOld]
    );
    assert_eq!(
        version::warnings("1.0", false),
        vec![FirmwareWarning::TooOld]
    );
}

#[test]
fn firmware_below_the_newest_known_is_outdated() {
    for v in ["1.7.0", "2.0.0", "2.0.6"] {
        assert_eq!(
            version::warnings(v, false),
            vec![FirmwareWarning::Outdated],
            "{v}"
        );
    }
}

#[test]
fn newest_or_newer_firmware_has_no_warning() {
    for v in ["2.0.7", "2.1", "3.0.0"] {
        assert_eq!(version::warnings(v, false), Vec::new(), "{v}");
    }
}

#[test]
fn head_update_required_adds_its_own_warning() {
    assert_eq!(
        version::warnings("2.0.7", true),
        vec![FirmwareWarning::UpdateRequired]
    );
    let w = version::warnings("1.2.0", true);
    assert!(w.contains(&FirmwareWarning::TooOld), "{w:?}");
    assert!(w.contains(&FirmwareWarning::UpdateRequired), "{w:?}");
}

#[test]
fn unparseable_version_gives_no_version_warning_and_no_panic() {
    assert_eq!(version::warnings("", false), Vec::new());
    assert_eq!(version::warnings("garbage", false), Vec::new());
    assert_eq!(
        version::warnings("garbage", true),
        vec![FirmwareWarning::UpdateRequired]
    );
}

#[test]
fn warnings_have_stable_codes_and_readable_text() {
    assert_eq!(FirmwareWarning::TooOld.code(), "too_old");
    assert_eq!(FirmwareWarning::Outdated.code(), "outdated");
    assert_eq!(FirmwareWarning::UpdateRequired.code(), "update_required");
    assert!(FirmwareWarning::TooOld.to_string().contains("1.7.0"));
    assert!(FirmwareWarning::Outdated.to_string().contains("2.0.7"));
    assert!(
        FirmwareWarning::UpdateRequired
            .to_string()
            .to_lowercase()
            .contains("update required")
    );
}
