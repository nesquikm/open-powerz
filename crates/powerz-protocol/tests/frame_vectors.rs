//! Frame codec over captured replies (FR BXYTJX, AC .5 to .7).
//!
//! Fixtures in `tests/vectors/*.hex` are reply frames from our own on-device
//! session 1 (firmware 2.0.6); each file names its source log. They carry no
//! device identifiers.

use powerz_protocol::frame::{
    Attribute, FrameError, MainHeader, Section, SectionHeader, parse_put_data,
};
use proptest::prelude::*;

fn vector(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let hex: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(|l| l.chars().filter(|c| !c.is_whitespace()))
        .collect();
    assert!(hex.len().is_multiple_of(2), "odd hex length in {name}");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn atts(sections: &[Section<'_>]) -> Vec<u16> {
    sections.iter().map(|s| s.att.0).collect()
}

// ---- AC-BXYTJX.5: section walker over captured replies ----

#[test]
fn parses_captured_adc_reply() {
    let frame = vector("adc_52.hex");
    assert_eq!(frame.len(), 52);
    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x001]);
    assert_eq!(sections[0].att, Attribute::ADC);
    assert_eq!(sections[0].payload.len(), 44);
    assert_eq!(sections[0].payload, &frame[8..52]);
}

#[test]
fn parses_captured_settings_reply() {
    let frame = vector("settings_188.hex");
    assert_eq!(frame.len(), 188);
    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x008]);
    assert_eq!(sections[0].att, Attribute::SETTINGS);
    assert_eq!(sections[0].payload, &frame[8..188]);
}

#[test]
fn parses_captured_adc_settings_chained_reply() {
    let frame = vector("adc_settings_236.hex");
    assert_eq!(frame.len(), 236);
    // ADC comes first and carries next = 1.
    let first = SectionHeader::decode(&frame[4..8]).unwrap();
    assert_eq!((first.att, first.next, first.size), (0x001, true, 44));

    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x001, 0x008]);
    assert_eq!(sections[0].payload, &frame[8..52]);
    assert_eq!(sections[1].payload, &frame[56..236]);
}

#[test]
fn parses_captured_queue_reply_with_63_records() {
    let frame = vector("queue_1268.hex");
    assert_eq!(frame.len(), 1268);
    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x002]);
    assert_eq!(sections[0].att, Attribute::ADC_QUEUE);
    // chunk (63) x size (20), not size alone.
    assert_eq!(sections[0].payload.len(), 63 * 20);
    assert_eq!(sections[0].payload.len() / 20, 63);
    assert_eq!(sections[0].payload, &frame[8..1268]);
}

/// Build a frame: PUT_DATA main header then raw section bytes.
fn put_data(body: &[u8]) -> Vec<u8> {
    let mut f = vec![0x41, 0x01, 0x00, 0x00];
    f.extend_from_slice(body);
    f
}

fn section_hdr(att: u16, next: bool, chunk: u8, size: u16) -> [u8; 4] {
    let raw = u32::from(att & 0x7FFF)
        | (u32::from(next) << 15)
        | (u32::from(chunk & 0x3F) << 16)
        | (u32::from(size & 0x3FF) << 22);
    raw.to_le_bytes()
}

#[test]
fn queue_length_is_chunk_times_size() {
    let mut body = Vec::new();
    body.extend_from_slice(&section_hdr(0x002, true, 3, 20));
    body.extend((0..60u8).collect::<Vec<_>>());
    body.extend_from_slice(&section_hdr(0x008, false, 0, 4));
    body.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
    let frame = put_data(&body);
    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x002, 0x008]);
    assert_eq!(sections[0].payload.len(), 60);
    assert_eq!(sections[0].payload, &frame[8..68]);
    assert_eq!(sections[1].payload, &[0xAA, 0xBB, 0xCC, 0xDD]);
}

#[test]
fn non_queue_length_ignores_chunk() {
    // A non-queue section with a stray chunk value is still `size` long.
    let mut body = Vec::new();
    body.extend_from_slice(&section_hdr(0x001, true, 5, 4));
    body.extend_from_slice(&[1, 2, 3, 4]);
    body.extend_from_slice(&section_hdr(0x008, false, 0, 2));
    body.extend_from_slice(&[9, 9]);
    let frame = put_data(&body);
    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x001, 0x008]);
    assert_eq!(sections[0].payload, &[1, 2, 3, 4]);
    assert_eq!(sections[1].payload, &[9, 9]);
}

#[test]
fn unknown_attribute_is_skipped_by_length() {
    let mut body = Vec::new();
    // Unknown attribute 0x4000 with an 8-byte body that looks like a header.
    body.extend_from_slice(&section_hdr(0x4000, true, 0, 8));
    body.extend_from_slice(&section_hdr(0x001, false, 0, 0x3FF));
    body.extend_from_slice(&[0xEE; 4]);
    body.extend_from_slice(&section_hdr(0x008, false, 0, 3));
    body.extend_from_slice(&[7, 8, 9]);
    let frame = put_data(&body);
    let sections = parse_put_data(&frame).unwrap();
    let settings: Vec<_> = sections
        .iter()
        .filter(|s| s.att == Attribute::SETTINGS)
        .collect();
    assert_eq!(settings.len(), 1);
    assert_eq!(settings[0].payload, &[7, 8, 9]);
    // Nothing inside the unknown body was taken for an ADC section.
    assert!(sections.iter().all(|s| s.att != Attribute::ADC));
    // If the unknown section is reported, its payload is exactly its 8 bytes.
    if let Some(u) = sections.iter().find(|s| s.att.0 == 0x4000) {
        assert_eq!(u.payload, &frame[8..16]);
    }
}

#[test]
fn pd_status_takes_rest_of_frame() {
    // PD_STATUS declares size 0, yet 12 bytes follow it.
    let mut body = Vec::new();
    body.extend_from_slice(&section_hdr(0x001, true, 0, 4));
    body.extend_from_slice(&[1, 2, 3, 4]);
    body.extend_from_slice(&section_hdr(0x020, false, 0, 0));
    body.extend((100..112u8).collect::<Vec<_>>());
    let frame = put_data(&body);
    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x001, 0x020]);
    assert_eq!(sections[1].att, Attribute::PD_STATUS);
    assert_eq!(sections[1].payload, &frame[16..28]);
    assert_eq!(sections[1].payload.len(), 12);
}

// ---- AC-BXYTJX.6: never frame by obj ----

#[test]
fn walker_ignores_obj_on_captured_242_byte_reply() {
    let frame = vector("adc_settings_pdstatus_242.hex");
    assert_eq!(frame.len(), 242);
    let obj = MainHeader::decode(&frame).unwrap().data().obj;
    assert_eq!(obj, 56);
    // obj disagrees with the real length in either reading.
    assert_ne!(4 + usize::from(obj) * 4, frame.len());
    assert_ne!(usize::from(obj) * 4, frame.len());

    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x001, 0x008, 0x020]);
    assert_eq!(sections[0].payload, &frame[8..52]);
    assert_eq!(sections[1].payload, &frame[56..236]);
    // PD_STATUS is the last 2 bytes, the rest of the frame.
    assert_eq!(sections[2].payload, &frame[240..242]);
}

#[test]
fn walker_ignores_obj_on_synthetic_frame() {
    let adc = vector("adc_52.hex");
    let mut frame = adc.clone();
    // Claim obj = 1023 words: must not change the result.
    let raw = u32::from_le_bytes(frame[0..4].try_into().unwrap()) | (0x3FF << 22);
    frame[0..4].copy_from_slice(&raw.to_le_bytes());
    let sections = parse_put_data(&frame).unwrap();
    assert_eq!(atts(&sections), vec![0x001]);
    assert_eq!(sections[0].payload, &adc[8..52]);
}

// ---- AC-BXYTJX.7: typed errors and no panics ----

#[test]
fn truncated_main_header_is_error() {
    for len in 0..4 {
        let frame = [0x41u8, 0x01, 0x80][..len].to_vec();
        assert!(
            matches!(
                parse_put_data(&frame),
                Err(FrameError::TruncatedHeader { .. })
            ),
            "len {len}"
        );
        assert!(matches!(
            MainHeader::decode(&frame),
            Err(FrameError::TruncatedHeader { .. })
        ));
        assert!(matches!(
            SectionHeader::decode(&frame),
            Err(FrameError::TruncatedHeader { .. })
        ));
    }
}

#[test]
fn truncated_section_header_is_error() {
    // Main header then only 2 bytes of a section header.
    let frame = put_data(&[0x01, 0x00]);
    assert!(matches!(
        parse_put_data(&frame),
        Err(FrameError::TruncatedHeader { .. })
    ));
    // After a section with next = 1, a partial header.
    let mut body = section_hdr(0x001, true, 0, 2).to_vec();
    body.extend_from_slice(&[5, 6, 0x08, 0x00, 0x00]);
    assert!(matches!(
        parse_put_data(&put_data(&body)),
        Err(FrameError::TruncatedHeader { .. })
    ));
}

#[test]
fn section_length_past_end_is_error() {
    let adc = vector("adc_52.hex");
    // Cut the captured ADC reply short.
    assert!(matches!(
        parse_put_data(&adc[..30]),
        Err(FrameError::SectionOverrun { .. })
    ));
    // Queue section whose chunk x size overruns the buffer.
    let queue = vector("queue_1268.hex");
    assert!(matches!(
        parse_put_data(&queue[..1267]),
        Err(FrameError::SectionOverrun { .. })
    ));
}

#[test]
fn next_set_with_no_following_header_is_error() {
    let mut frame = vector("adc_52.hex");
    // Set the next bit on the only section.
    frame[5] |= 0x80;
    assert!(matches!(
        parse_put_data(&frame),
        Err(FrameError::MissingNextSection { .. })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..=2048)) {
        let _ = parse_put_data(&bytes);
        let _ = MainHeader::decode(&bytes);
        let _ = SectionHeader::decode(&bytes);
    }

    #[test]
    fn random_section_chains_never_panic(
        heads in proptest::collection::vec((any::<u16>(), any::<bool>(), 0u8..64, 0u16..1024), 0..8),
        tail in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let mut body = Vec::new();
        for (att, next, chunk, size) in heads {
            body.extend_from_slice(&section_hdr(att, next, chunk, size));
        }
        body.extend_from_slice(&tail);
        let frame = put_data(&body);
        if let Ok(sections) = parse_put_data(&frame) {
            // Every payload is a slice of the input.
            let total: usize = sections.iter().map(|s| s.payload.len()).sum();
            prop_assert!(total <= frame.len());
        }
    }
}
