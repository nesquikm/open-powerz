//! Firmware container parsing (FR V9SK53). Every container here is built by
//! the test suite ([`ContainerBuilder`]); no vendor firmware file is read,
//! except by the hand-run `#[ignore]`d check at the end, which reads the
//! file named by `POWERZ_FW_FILE` from outside the repo.

#[path = "support/fw_container.rs"]
mod fw_container;

use fw_container::*;
use powerz_protocol::firmware::container::{
    ContainerError, FirmwareContainer, MAX_CONTAINER_BYTES,
};
use proptest::collection::vec;
use proptest::prelude::*;

fn parse(bytes: &[u8]) -> Result<FirmwareContainer<'_>, ContainerError> {
    FirmwareContainer::parse(bytes)
}

// ---- AC-V9SK53.1: header fields and payload ----

#[test]
fn parses_every_header_field_of_a_synthetic_container() {
    let b = ContainerBuilder::new();
    let bytes = b.build();
    let c = parse(&bytes).expect("a valid synthetic container parses");
    assert_eq!(c.magic(), "Firmware file");
    assert_eq!(c.vendor(), "ChargerLab");
    assert_eq!(c.container_version(), "V1.0");
    assert_eq!(c.model(), "KM003C");
    assert_eq!(c.firmware_version(), "2.0.7");
    assert_eq!(c.date(), "2026.9.29");
    assert_eq!(c.crc(), 0xA175_E4A0);
    assert_eq!(c.size(), 48);
    assert_eq!(c.release_notes_cn(), CN_NOTES);
    assert_eq!(c.release_notes_en(), EN_NOTES);
    assert_eq!(c.payload(), b.payload.as_slice());
}

#[test]
fn crc_text_is_read_as_hex_and_size_as_decimal() {
    let bytes = ContainerBuilder::new()
        .field(CRC, "0000BEEF")
        .payload_sized(&[7u8; 456])
        .build();
    let c = parse(&bytes).expect("parses");
    assert_eq!(c.crc(), 0xBEEF);
    assert_eq!(c.size(), 456);
    assert_eq!(c.payload().len(), 456);
}

#[test]
fn lowercase_crc_hex_is_accepted() {
    let bytes = ContainerBuilder::new().field(CRC, "a175e4a0").build();
    assert_eq!(parse(&bytes).expect("parses").crc(), 0xA175_E4A0);
}

#[test]
fn other_models_and_versions_are_read_as_written() {
    let bytes = ContainerBuilder::new()
        .field(MODEL, "KM002C")
        .field(FIRMWARE_VERSION, "1.9.3")
        .field(DATE, "2025.1.2")
        .field(CONTAINER_VERSION, "V1.1")
        .build();
    let c = parse(&bytes).expect("parses");
    assert_eq!(c.model(), "KM002C");
    assert_eq!(c.firmware_version(), "1.9.3");
    assert_eq!(c.date(), "2025.1.2");
    assert_eq!(c.container_version(), "V1.1");
}

#[test]
fn a_payload_exactly_the_declared_size_is_accepted() {
    let bytes = ContainerBuilder::new().payload_sized(&[1, 2, 3, 4]).build();
    let c = parse(&bytes).expect("parses");
    assert_eq!(c.size(), 4);
    assert_eq!(c.payload(), &[1, 2, 3, 4]);
}

#[test]
fn an_empty_english_note_is_an_empty_string() {
    let bytes = ContainerBuilder::new().field(NOTES_EN, "").build();
    assert_eq!(parse(&bytes).expect("parses").release_notes_en(), "");
}

// ---- AC-V9SK53.2: refusals with a clear reason ----

/// Every refusal reads as a sentence: capitalised, ends with a full stop.
fn assert_sentence(e: &ContainerError) -> String {
    let msg = e.to_string();
    assert!(msg.len() > 10, "too short to be clear: {msg:?}");
    assert!(
        msg.chars().next().is_some_and(char::is_uppercase),
        "not a sentence: {msg:?}"
    );
    assert!(msg.ends_with('.'), "not a sentence: {msg:?}");
    msg
}

#[test]
fn wrong_magic_is_refused() {
    let bytes = ContainerBuilder::new()
        .field(MAGIC, "Not a firmware")
        .build();
    let e = parse(&bytes).expect_err("wrong magic");
    assert!(matches!(e, ContainerError::BadMagic { .. }), "{e:?}");
    let msg = assert_sentence(&e).to_lowercase();
    assert!(msg.contains("firmware"), "{msg}");
}

#[test]
fn null_magic_is_refused_as_bad_magic() {
    let bytes = ContainerBuilder::new().null_field(MAGIC).build();
    let e = parse(&bytes).expect_err("null magic");
    assert!(matches!(e, ContainerError::BadMagic { .. }), "{e:?}");
}

#[test]
fn empty_input_is_truncated() {
    let e = parse(&[]).expect_err("empty");
    assert!(matches!(e, ContainerError::Truncated { .. }), "{e:?}");
    assert_sentence(&e);
}

#[test]
fn every_truncation_of_a_valid_container_is_refused() {
    let bytes = ContainerBuilder::new().build();
    for n in 0..bytes.len() {
        let e = parse(&bytes[..n]).expect_err("truncated");
        assert!(
            matches!(
                e,
                ContainerError::Truncated { .. } | ContainerError::PayloadTooShort { .. }
            ),
            "cut at {n}: {e:?}"
        );
    }
}

#[test]
fn truncation_inside_the_header_is_truncated() {
    let b = ContainerBuilder::new();
    let bytes = b.build();
    // Cut in the middle of the strings, well before the byte array.
    let e = parse(&bytes[..b.payload_offset() / 2]).expect_err("cut");
    assert!(matches!(e, ContainerError::Truncated { .. }), "{e:?}");
}

#[test]
fn a_byte_array_longer_than_the_file_is_truncated() {
    let mut b = ContainerBuilder::new();
    b.payload_len = Some(b.payload.len() as u32 + 100);
    let e = parse(&b.build()).expect_err("byte array runs past the end");
    assert!(matches!(e, ContainerError::Truncated { .. }), "{e:?}");
}

#[test]
fn a_payload_shorter_than_the_declared_size_is_refused() {
    let bytes = ContainerBuilder::new()
        .payload(&[0u8; 40])
        .field(SIZE, "41")
        .build();
    let e = parse(&bytes).expect_err("payload too short");
    assert!(matches!(e, ContainerError::PayloadTooShort { .. }), "{e:?}");
    let msg = assert_sentence(&e);
    assert!(msg.contains("41"), "declared size in the reason: {msg}");
    assert!(msg.contains("40"), "actual length in the reason: {msg}");
}

#[test]
fn unparseable_crc_text_is_refused() {
    for bad in ["", "XYZ", "A175E4A", "A175E4A00", "-175E4A0"] {
        let bytes = ContainerBuilder::new().field(CRC, bad).build();
        let e = parse(&bytes).expect_err("bad CRC");
        assert!(matches!(e, ContainerError::BadCrc { .. }), "{bad:?}: {e:?}");
        assert_sentence(&e);
    }
}

#[test]
fn unparseable_size_text_is_refused() {
    for bad in ["", "abc", "-1", "4.5", "99999999999"] {
        let bytes = ContainerBuilder::new().field(SIZE, bad).build();
        let e = parse(&bytes).expect_err("bad size");
        assert!(
            matches!(e, ContainerError::BadSize { .. }),
            "{bad:?}: {e:?}"
        );
        assert_sentence(&e);
    }
}

#[test]
fn a_wrong_string_count_is_refused() {
    let e = parse(&ContainerBuilder::new().strings_only(9).build()).expect_err("9 strings");
    assert!(matches!(e, ContainerError::StringCount { .. }), "{e:?}");
    assert_sentence(&e);
    let e = parse(&ContainerBuilder::new().count(11).build()).expect_err("count 11");
    assert!(matches!(e, ContainerError::StringCount { .. }), "{e:?}");
}

#[test]
fn model_check_accepts_the_same_model() {
    let bytes = ContainerBuilder::new().build();
    let c = parse(&bytes).expect("parses");
    assert!(c.check_model("KM003C").is_ok());
}

#[test]
fn model_check_refuses_another_model_naming_both() {
    let bytes = ContainerBuilder::new().build();
    let c = parse(&bytes).expect("parses");
    let e = c.check_model("KM002C").expect_err("other model");
    match &e {
        ContainerError::ModelMismatch { file, meter } => {
            assert_eq!(file, "KM003C");
            assert_eq!(meter, "KM002C");
        }
        other => panic!("expected ModelMismatch, got {other:?}"),
    }
    let msg = assert_sentence(&e);
    assert!(msg.contains("KM003C") && msg.contains("KM002C"), "{msg}");
}

#[test]
fn every_refusal_has_a_distinct_clear_reason() {
    let c_bytes = ContainerBuilder::new().build();
    let c = parse(&c_bytes).expect("parses");
    let errors = [
        parse(&ContainerBuilder::new().field(MAGIC, "x").build()).expect_err("magic"),
        parse(&c_bytes[..3]).expect_err("truncated"),
        parse(&ContainerBuilder::new().field(SIZE, "1000").build()).expect_err("short"),
        parse(&ContainerBuilder::new().field(CRC, "zz").build()).expect_err("crc"),
        parse(&ContainerBuilder::new().field(SIZE, "zz").build()).expect_err("size"),
        parse(&ContainerBuilder::new().strings_only(3).build()).expect_err("count"),
        c.check_model("KM002C").expect_err("model"),
        parse(&vec![0u8; MAX_CONTAINER_BYTES + 1]).expect_err("too large"),
        parse(
            &ContainerBuilder::new()
                .raw_field(VENDOR, &[0xD800, 0x0041])
                .build(),
        )
        .expect_err("bad string"),
        parse(&c_bytes[..ContainerBuilder::new().string_offset(MODEL) + 6]).expect_err("header"),
    ];
    let msgs: Vec<String> = errors.iter().map(assert_sentence).collect();
    for (i, a) in msgs.iter().enumerate() {
        for b in &msgs[i + 1..] {
            assert_ne!(a, b, "two refusals read the same");
        }
    }
}

// ---- AC-V9SK53.2 (review fixes): size cap, strict UTF-16, echo cap ----

#[test]
fn the_size_cap_is_eight_mebibytes() {
    assert_eq!(MAX_CONTAINER_BYTES, 8 * 1024 * 1024);
}

/// A valid container exactly `total` bytes long.
fn container_of_total_len(total: usize) -> Vec<u8> {
    let overhead = ContainerBuilder::new().payload(&[]).build().len();
    let payload = vec![0x5Au8; total - overhead];
    // Keep the declared size (48) so the size text length is unchanged.
    let bytes = ContainerBuilder::new().payload(&payload).build();
    assert_eq!(bytes.len(), total);
    bytes
}

#[test]
fn a_container_of_exactly_the_cap_is_accepted() {
    let bytes = container_of_total_len(MAX_CONTAINER_BYTES);
    let c = parse(&bytes).expect("a container at the cap parses");
    assert_eq!(c.model(), "KM003C");
}

#[test]
fn input_longer_than_the_cap_is_refused_as_too_large() {
    let bytes = container_of_total_len(MAX_CONTAINER_BYTES + 1);
    let e = parse(&bytes).expect_err("over the cap");
    assert_eq!(
        e,
        ContainerError::TooLarge {
            len: MAX_CONTAINER_BYTES + 1,
            max: MAX_CONTAINER_BYTES,
        }
    );
    let msg = assert_sentence(&e);
    assert!(
        msg.contains(&(MAX_CONTAINER_BYTES + 1).to_string()),
        "the length in the reason: {msg}"
    );
    assert!(
        msg.contains(&MAX_CONTAINER_BYTES.to_string()),
        "the cap in the reason: {msg}"
    );
    assert!(msg.to_lowercase().contains("too large"), "{msg}");
}

#[test]
fn an_unpaired_high_surrogate_is_refused_as_a_bad_string() {
    let bytes = ContainerBuilder::new()
        .raw_field(FIRMWARE_VERSION, &[0x0032, 0xD800, 0x0041])
        .build();
    let e = parse(&bytes).expect_err("unpaired surrogate");
    assert_eq!(
        e,
        ContainerError::BadString {
            index: FIRMWARE_VERSION
        }
    );
    let msg = assert_sentence(&e);
    assert!(msg.contains("string 4"), "names the string index: {msg}");
}

#[test]
fn a_trailing_lone_surrogate_is_refused_with_its_index() {
    for (index, units) in [
        (MAGIC, vec![0x0046, 0xDBFF]),
        (NOTES_EN, vec![0xDC00]),
        (MODEL, vec![0xDC00, 0xD800]),
    ] {
        let bytes = ContainerBuilder::new().raw_field(index, &units).build();
        let e = parse(&bytes).expect_err("lone surrogate");
        assert_eq!(e, ContainerError::BadString { index }, "{units:04X?}");
    }
}

#[test]
fn a_valid_surrogate_pair_still_decodes() {
    let bytes = ContainerBuilder::new()
        .field(NOTES_EN, "fix \u{1F600}")
        .build();
    assert_eq!(
        parse(&bytes)
            .expect("a paired surrogate is valid")
            .release_notes_en(),
        "fix \u{1F600}"
    );
}

/// The Display of `e` echoes at most 32 characters of `long` (a run of
/// `ch`), keeps its first characters, and stays short.
fn assert_echo_capped(e: &ContainerError, ch: char) {
    let msg = assert_sentence(e);
    let run = |n: usize| std::iter::repeat_n(ch, n).collect::<String>();
    assert!(msg.contains(&run(16)), "keeps the first characters: {msg}");
    assert!(!msg.contains(&run(33)), "echoes over 32 characters: {msg}");
    assert!(msg.chars().count() < 160, "the reason stays short: {msg}");
}

#[test]
fn a_long_bad_magic_is_echoed_cut_short() {
    let long = "M".repeat(1000);
    let e = parse(&ContainerBuilder::new().field(MAGIC, &long).build()).expect_err("magic");
    assert!(matches!(e, ContainerError::BadMagic { .. }), "{e:?}");
    assert_echo_capped(&e, 'M');
}

#[test]
fn a_long_bad_crc_is_echoed_cut_short() {
    let long = "Z".repeat(1000);
    let e = parse(&ContainerBuilder::new().field(CRC, &long).build()).expect_err("crc");
    assert!(matches!(e, ContainerError::BadCrc { .. }), "{e:?}");
    assert_echo_capped(&e, 'Z');
}

#[test]
fn a_long_bad_size_is_echoed_cut_short() {
    let long = "q".repeat(1000);
    let e = parse(&ContainerBuilder::new().field(SIZE, &long).build()).expect_err("size");
    assert!(matches!(e, ContainerError::BadSize { .. }), "{e:?}");
    assert_echo_capped(&e, 'q');
}

#[test]
fn a_short_bad_value_is_echoed_whole() {
    let e = parse(&ContainerBuilder::new().field(CRC, "XYZ").build()).expect_err("crc");
    assert!(e.to_string().contains("XYZ"), "{e}");
}

#[test]
fn truncation_inside_a_header_string_names_its_index() {
    let b = ContainerBuilder::new();
    let bytes = b.build();
    let start = b.string_offset(MODEL);
    // Inside the length prefix, then inside the code units, of string 3.
    for cut in [start + 2, start + 4 + 3] {
        let e = parse(&bytes[..cut]).expect_err("cut");
        assert!(matches!(e, ContainerError::Truncated { .. }), "{e:?}");
        let msg = assert_sentence(&e);
        assert!(msg.contains("header string 3"), "cut at {cut}: {msg}");
    }
}

#[test]
fn truncation_inside_the_magic_names_string_0() {
    let bytes = ContainerBuilder::new().build();
    let e = parse(&bytes[..10]).expect_err("cut");
    let msg = assert_sentence(&e);
    assert!(msg.contains("header string 0"), "{msg}");
}

// ---- AC-V9SK53.4: no input panics ----

/// Touch every accessor so a panic anywhere surfaces.
fn touch(c: &FirmwareContainer<'_>) -> usize {
    c.magic().len()
        + c.vendor().len()
        + c.container_version().len()
        + c.model().len()
        + c.firmware_version().len()
        + c.date().len()
        + c.crc() as usize % 7
        + c.size() as usize % 7
        + c.release_notes_cn().len()
        + c.release_notes_en().len()
        + c.payload().len()
        + usize::from(c.check_model("KM003C").is_ok())
}

proptest! {
    #[test]
    fn parse_never_panics_on_arbitrary_bytes(bytes in vec(any::<u8>(), 0..512)) {
        if let Ok(c) = FirmwareContainer::parse(&bytes) {
            touch(&c);
        }
    }

    #[test]
    fn parse_never_panics_on_a_mutated_container(
        flips in vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        cut in any::<prop::sample::Index>(),
        do_cut in any::<bool>(),
    ) {
        let mut bytes = ContainerBuilder::new().build();
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] = v;
        }
        if do_cut {
            bytes.truncate(cut.index(bytes.len() + 1));
        }
        if let Ok(c) = FirmwareContainer::parse(&bytes) {
            touch(&c);
        }
    }

    #[test]
    fn parse_never_panics_on_huge_declared_lengths(
        field in 0usize..10,
        len in prop_oneof![Just(u32::MAX - 1), Just(u32::MAX), Just(1u32), any::<u32>()],
    ) {
        // Overwrite the length prefix of string `field` (or the count).
        let b = ContainerBuilder::new();
        let mut bytes = b.build();
        let mut at = 4usize;
        for s in &b.strings[..field] {
            at += 4 + s.as_ref().map_or(0, |s| s.encode_utf16().count() * 2);
        }
        bytes[at..at + 4].copy_from_slice(&len.to_be_bytes());
        if let Ok(c) = FirmwareContainer::parse(&bytes) {
            touch(&c);
        }
        bytes[0..4].copy_from_slice(&len.to_be_bytes());
        if let Ok(c) = FirmwareContainer::parse(&bytes) {
            touch(&c);
        }
    }

    // ---- AC-V9SK53.5: the payload is passed on byte for byte ----

    #[test]
    fn payload_is_the_input_bytes_unchanged(
        payload in vec(any::<u8>(), 0..2048),
        extra in 0usize..32,
    ) {
        let declared = payload.len().saturating_sub(extra);
        let b = ContainerBuilder::new()
            .payload(&payload)
            .field(SIZE, &declared.to_string());
        let bytes = b.build();
        let c = FirmwareContainer::parse(&bytes).expect("valid container parses");
        prop_assert_eq!(c.payload(), payload.as_slice());
        prop_assert_eq!(c.payload().len(), payload.len());
        // A borrow of the input itself, not a decrypted or copied buffer.
        let off = b.payload_offset();
        prop_assert_eq!(c.payload().as_ptr(), bytes[off..].as_ptr());
    }
}

#[test]
fn payload_of_a_parsed_container_is_not_decrypted() {
    // A payload that is plain text stays plain text: nothing is decrypted.
    let plain = b"plain synthetic payload bytes!!!".to_vec();
    let bytes = ContainerBuilder::new().payload_sized(&plain).build();
    let before = bytes.clone();
    let c = parse(&bytes).expect("parses");
    assert_eq!(c.payload(), plain.as_slice());
    drop(c);
    assert_eq!(bytes, before, "the input is unchanged");
}

// ---- Hand-run check against the real file, kept outside the repo ----

#[test]
#[ignore = "reads the vendor firmware file named by POWERZ_FW_FILE (never committed)"]
fn real_2_0_7_container_from_env_path() {
    let Ok(path) = std::env::var("POWERZ_FW_FILE") else {
        panic!("set POWERZ_FW_FILE to the vendor KM003C 2.0.7 firmware file");
    };
    let bytes = std::fs::read(&path).expect("read POWERZ_FW_FILE");
    let c = parse(&bytes).expect("the real container parses");
    assert_eq!(c.model(), "KM003C");
    assert_eq!(c.firmware_version(), "2.0.7");
    assert_eq!(c.vendor(), "ChargerLab");
    assert!(c.payload().len() >= c.size() as usize);
    assert!(!c.release_notes_en().is_empty());
}
