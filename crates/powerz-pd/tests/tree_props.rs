//! AC-TGE83B.7: every decoded message has a one-line summary and a field
//! tree whose fields carry a name, bit range, raw value and meaning.
//! Reserved or invalid values are flagged, and no input makes the decoder
//! panic (property test).

mod common;

use common::*;
use powerz_pd::{Capture, Decoded, Field, Flag, Input};
use proptest::collection::vec;
use proptest::prelude::*;

/// A spread of valid messages of every family.
fn samples() -> Vec<(u8, u16, Vec<u8>)> {
    let mut out = vec![
        (SOP, control(0x01), vec![]),
        (SOP, control(0x0D), vec![]),
        (
            SOP,
            data_from_source(4, 1),
            objects(&[0x0981_912C, 0x0002_D12C, 0xC9A4_213C, 0xD230_968C]),
        ),
        (SOP, data_from_sink(1, 2), objects(&[0x1344_B12C])),
        (
            SOP,
            data_from_sink(2, 4),
            objects(&[0x3101_912C, 0xC1A4_213C]),
        ),
        (SOP, data_from_source(1, 3), objects(&[0x5000_0000])),
        (SOP, data_from_source(1, 5), objects(&[0x01F4_0600])),
        (SOP, data_from_source(1, 6), objects(&[0x0200_0000])),
        (SOP, data_from_sink(1, 7), objects(&[0x5553_0000])),
        (SOP, data_from_source(1, 8), objects(&[0x2000_0000])),
        (
            SOP,
            data_from_sink(2, 9),
            objects(&[0x9344_B014, 0xD230_968C]),
        ),
        (SOP, data_from_sink(1, 10), objects(&[0x0100_0000])),
        (SOP, data_from_source(1, 11), objects(&[0x8064_6464])),
        (SOP, data_from_source(1, 12), objects(&[0x3211_0000])),
        (
            SOP_PRIME,
            0x518F,
            objects(&[
                0xFF00_A841,
                0x1860_050D,
                0x0000_404D,
                0x8254_0000,
                0x000A_2640,
            ]),
        ),
        (
            SOP,
            data_from_source(3, 15),
            objects(&[0x05AC_1234, 0xDEAD_BEEF, 1]),
        ),
    ];
    for (ty, data) in [
        (0x02u16, vec![45u8, 2, 0, 2, 0, 0, 0]),
        (0x03, vec![1]),
        (
            0x05,
            vec![0x34, 0x12, 0x78, 0x56, 0xF4, 0x01, 0xE0, 0x01, 0],
        ),
        (0x07, b"\x34\x12\x78\x56ACME".to_vec()),
        (0x0C, vec![0xC2, 0x01, 0x28, 0]),
        (0x10, vec![3, 0]),
    ] {
        let (h, b) = extended_single(ty, &data, true);
        out.push((SOP, h, b));
    }
    out
}

fn check_tree(fields: &[Field], ctx: &str) {
    assert!(!fields.is_empty(), "{ctx}: empty field tree");
    for f in all_fields(fields) {
        assert!(!f.name.is_empty(), "{ctx}: unnamed field {f:?}");
        assert!(
            !f.meaning.is_empty(),
            "{ctx}: field {} has no meaning",
            f.name
        );
        assert!(
            f.bits.hi >= f.bits.lo,
            "{ctx}: field {} bit range {:?}",
            f.name,
            f.bits
        );
    }
}

#[test]
fn every_message_has_a_one_line_summary_and_a_full_tree() {
    for (sop, h, body) in samples() {
        let d = decode_one(sop, h, &body);
        let ctx = format!(
            "{} (sop {sop}, header {h:#06x})",
            d.header.message_type.name()
        );
        let s = d.summary();
        assert!(!s.is_empty(), "{ctx}: empty summary");
        assert!(
            !s.contains('\n') && !s.contains('\r'),
            "{ctx}: multi-line summary {s:?}"
        );
        assert!(
            s.starts_with(d.header.message_type.name()),
            "{ctx}: summary {s:?}"
        );
        let fields = d.fields();
        assert_eq!(fields[0].name, "Header", "{ctx}: the header comes first");
        check_tree(&fields, &ctx);
        // Leaf raw values fit their bit range.
        for f in all_fields(&fields)
            .into_iter()
            .filter(|f| f.children.is_empty())
        {
            let width = u32::from(f.bits.hi - f.bits.lo) + 1;
            if width < 64 {
                assert!(
                    f.raw < (1u64 << width),
                    "{ctx}: {} raw {:#x} wider than {width} bits",
                    f.name,
                    f.raw
                );
            }
        }
    }
}

#[test]
fn truncated_data_object_is_flagged_not_panicked() {
    // Header says 2 objects, only 5 bytes follow.
    let d = decode_one(SOP, data_from_source(2, 1), &[0x2C, 0x91, 0x81, 0x09, 0x2C]);
    assert!(!d.issues.is_empty(), "a short body is reported");
    let fields = d.fields();
    check_tree(&fields, "truncated");
    assert!(
        all_fields(&fields)
            .iter()
            .any(|f| f.flag == Some(Flag::Invalid))
            || !d.issues.is_empty()
    );
    assert!(!d.summary().contains('\n'));
}

fn touch(d: &Decoded) -> usize {
    let s = d.summary();
    let fields = d.fields();
    s.len() + all_fields(&fields).len() + d.issues.len()
}

fn input() -> impl Strategy<Value = (u8, u16, Vec<u8>, u8)> {
    (
        prop_oneof![0u8..5, any::<u8>()],
        any::<u16>(),
        vec(any::<u8>(), 0..300),
        0u8..8,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn random_messages_never_panic((sop, header, body, _) in input()) {
        let mut cap = Capture::new();
        if let Some(d) = cap.feed(Input::Message { sop, header, body: &body }) {
            touch(&d);
            prop_assert!(!d.summary().contains('\n'));
        }
    }

    #[test]
    fn random_sequences_never_panic(seq in vec(input(), 0..40)) {
        let mut cap = Capture::new();
        for (sop, header, body, ev) in seq {
            let inp = match ev {
                0 => Input::HardReset,
                1 => Input::Detach,
                2 => Input::Attach,
                3 => Input::CableReset,
                _ => Input::Message { sop, header, body: &body },
            };
            if let Some(d) = cap.feed(inp) {
                touch(&d);
            }
        }
    }

    #[test]
    fn random_extended_chunks_never_panic(
        ty in 0u16..32,
        chunk_no in 0u16..16,
        req in any::<bool>(),
        size in 0u16..512,
        data in vec(any::<u8>(), 0..30),
        rounds in 1usize..6,
    ) {
        let mut cap = Capture::new();
        for r in 0..rounds {
            let (h, body) = extended(ty, ext_header(true, (chunk_no + r as u16) & 0xF, req, size), &data, r % 2 == 0);
            if let Some(d) = cap.feed(Input::Message { sop: SOP, header: h, body: &body }) {
                touch(&d);
            }
        }
    }
}
