//! The firmware update check follows AC-V9SK52.3 (FR 4PJRVJ, AC .8): a field
//! of 0 means the firmware is current, and any other value is an unrecognised
//! reply, never "update required". AC-AXHK2M.4: the code that could only ever
//! have said otherwise is gone. Every value is synthetic.

use powerz_protocol::decode::head::HeadReply;

fn head_reply(field: u32) -> Vec<u8> {
    let mut f = vec![0x40, 2, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

#[test]
fn a_non_zero_field_stays_readable() {
    for field in [0u32, 1, 2, 0x100, 0xFFFF_FFFF] {
        let reply = HeadReply::decode(&head_reply(field)).expect("decodes");
        assert_eq!(reply.old_firmware_field(), field);
    }
}

#[test]
fn the_docs_no_longer_promise_update_required() {
    let source = include_str!("../src/decode/head.rs");
    assert!(!source.contains("Any non-zero field means"));
}

#[test]
fn the_head_reply_has_no_update_required_answer() {
    let source = include_str!("../src/decode/head.rs");
    assert!(
        !source.contains("fn update_required"),
        "nothing outside the tests asks it"
    );
}

#[test]
fn no_firmware_warning_says_an_update_is_required() {
    let source = include_str!("../src/firmware/version.rs");
    for dead in ["UpdateRequired", "update_required"] {
        assert!(!source.contains(dead), "{dead} is still in version.rs");
    }
}
