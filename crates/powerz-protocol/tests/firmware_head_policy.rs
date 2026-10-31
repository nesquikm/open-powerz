//! `HeadReply::update_required()` follows AC-V9SK52.3 (FR 4PJRVJ, AC .8): a
//! field of 0 means the firmware is current, and any other value is an
//! unrecognised reply, never "update required". Every value is synthetic.

use powerz_protocol::decode::head::HeadReply;

fn head_reply(field: u32) -> Vec<u8> {
    let mut f = vec![0x40, 2, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

#[test]
fn a_zero_field_says_no_update_is_required() {
    let reply = HeadReply::decode(&head_reply(0)).expect("decodes");
    assert_eq!(reply.update_required(), Some(false));
}

#[test]
fn a_non_zero_field_is_unknown_and_never_update_required() {
    for field in [1u32, 2, 0x100, 0xFFFF_FFFF] {
        let reply = HeadReply::decode(&head_reply(field)).expect("decodes");
        assert_eq!(reply.update_required(), None, "field {field:#x}");
        assert_eq!(
            reply.old_firmware_field(),
            field,
            "the field stays readable"
        );
    }
}

#[test]
fn the_docs_no_longer_promise_update_required() {
    let source = include_str!("../src/decode/head.rs");
    assert!(!source.contains("Any non-zero field means"));
}
