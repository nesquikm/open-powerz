//! Changing meter settings (FR JKWT3W, AC .1 to .4): the key table, the
//! exact type-0x0A frame a write sends, the range refusals, and the re-read
//! that must show the change and nothing else.
//!
//! Layout from `docs/protocol.md` "Settings write" `[static]`: main header
//! `0x0A | id << 8 | n << 23` (n entries), then one 8-byte entry per key in
//! ascending order: the key word `0x01008000 | key` (bit 15 `next` cleared on
//! the last entry) and the i32 value. Every writable key is sent: changed
//! ones at their new values, the others at the values just read, the
//! startup logo (0x06) always at its read value, then 0x7FFF = 0x1234.
//!
//! Every block here is the captured FW 2.0.6 SETTINGS block (no serial or
//! UID in it), possibly with fields rewritten and the CRCs fixed.

mod common;

use common::*;
use powerz_protocol::crypto;
use powerz_protocol::decode::settings::{
    MeterSettings, SAVE_MARKER, SETTINGS_WRITE_KEYS, WritableField, settings_write_request,
};
use powerz_protocol::session::Session;
use powerz_protocol::transport::mock::{MockTransport, Step};
use powerz_protocol::transport::{GuardedTransport, Transport};

// ---- Synthetic SETTINGS blocks ----

/// The 180-byte payload of the captured FW 2.0.6 block: English (1),
/// brightness 50, dim off (0), logo off (0), theme blue (0), auto-rotate on,
/// rotation 90° (1), rate 50 SPS (2), offset calibration 80.
fn captured() -> Vec<u8> {
    vector(SETTINGS_188)[8..].to_vec()
}

fn fix_crcs(p: &mut [u8]) {
    let a = crypto::crc32(&p[0x00..0x5C]);
    p[0x5C..0x60].copy_from_slice(&a.to_le_bytes());
    let b = crypto::crc32(&p[0x60..0xB0]);
    p[0xB0..0xB4].copy_from_slice(&b.to_le_bytes());
}

fn word(p: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(p[off..off + 4].try_into().unwrap())
}

/// Replace the `width`-bit field at `shift` of the u32 at `off` with `code`.
fn set_bits(p: &mut [u8], off: usize, shift: u32, width: u32, code: u32) {
    let mask = ((1u32 << width) - 1) << shift;
    let w = (word(p, off) & !mask) | ((code << shift) & mask);
    p[off..off + 4].copy_from_slice(&w.to_le_bytes());
}

/// Where each field lives in the block (decoder map, research notes §3.4).
fn place(field: &str) -> (usize, u32, u32) {
    match field {
        "language" => (0x00, 0, 1),
        "brightness" => (0x00, 3, 7),
        "dim_timeout" => (0x00, 10, 2),
        "startup_logo" => (0x00, 14, 2),
        "theme" => (0x00, 16, 3),
        "auto_rotate" => (0x00, 27, 1),
        "rotation" => (0x60, 0, 2),
        "sample_rate" => (0x60, 2, 2),
        other => panic!("no place for {other}"),
    }
}

/// The captured block with `fields` set to the given codes, CRCs fixed.
fn block(fields: &[(&str, u32)]) -> Vec<u8> {
    let mut p = captured();
    for &(f, code) in fields {
        let (off, shift, width) = place(f);
        set_bits(&mut p, off, shift, width, code);
    }
    fix_crcs(&mut p);
    p
}

fn decode(p: &[u8]) -> MeterSettings {
    MeterSettings::decode(p).unwrap_or_else(|e| panic!("decode failed: {e}"))
}

/// The PUT_DATA reply carrying `payload` as its SETTINGS section, with `id`.
fn settings_reply(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = with_id(vector(SETTINGS_188), id)[..8].to_vec();
    f.extend_from_slice(payload);
    f
}

/// The documented type-0x0A frame, built by hand from (key, value) pairs.
fn expected_write(id: u8, entries: &[(u16, i32)]) -> Vec<u8> {
    let n = entries.len() as u32;
    let mut f = (0x0Au32 | (u32::from(id) << 8) | (n << 23))
        .to_le_bytes()
        .to_vec();
    for (i, &(key, value)) in entries.iter().enumerate() {
        let next = if i + 1 < entries.len() { 1u32 << 15 } else { 0 };
        f.extend((0x0100_0000u32 | next | u32::from(key)).to_le_bytes());
        f.extend(value.to_le_bytes());
    }
    f
}

/// The captured block's codes in key order, with `changes` applied, then the
/// save marker.
fn captured_entries(changes: &[(u16, i32)]) -> Vec<(u16, i32)> {
    let mut e = vec![
        (0x01, 1),
        (0x03, 50),
        (0x04, 0),
        (0x06, 0),
        (0x07, 0),
        (0x14, 2),
        (0x15, 1),
        (0x7FFF, 0x1234),
    ];
    for &(k, v) in changes {
        e.iter_mut()
            .find(|(key, _)| *key == k)
            .unwrap_or_else(|| panic!("key {k:#x} not in the table"))
            .1 = v;
    }
    e
}

fn written_of_type(written: &[Vec<u8>], ty: u8) -> Vec<Vec<u8>> {
    written
        .iter()
        .filter(|f| f.first().map(|b| b & 0x7F) == Some(ty))
        .cloned()
        .collect()
}

// ---- AC-JKWT3W.1: the recovered key table, and only it ----

#[test]
fn ac1_writable_fields_carry_the_recovered_keys() {
    let keys: Vec<(&str, u16)> = WritableField::ALL
        .iter()
        .map(|f| (f.name(), f.key()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("language", 0x01),
            ("brightness", 0x03),
            ("dim_timeout", 0x04),
            ("theme", 0x07),
            ("sample_rate", 0x14),
            ("rotation", 0x15),
        ]
    );
    for f in WritableField::ALL {
        assert_eq!(WritableField::from_name(f.name()), Some(f), "{}", f.name());
    }
}

#[test]
fn ac1_the_frame_carries_every_recovered_key_in_order_then_the_save_marker() {
    assert_eq!(
        SETTINGS_WRITE_KEYS,
        [0x01, 0x03, 0x04, 0x06, 0x07, 0x14, 0x15, 0x7FFF]
    );
    assert_eq!(SAVE_MARKER, 0x1234);
}

#[test]
fn ac1_fields_without_a_recovered_key_are_not_writable() {
    // Auto-rotate has no key; the startup logo is sent at its read value
    // only; calibration and OTP are never written; key 0x08 is the factory
    // reset.
    for name in [
        "auto_rotate",
        "startup_logo",
        "voltage_cal",
        "current_cal",
        "offset_cal",
        "otp",
        "factory_reset",
        "name",
        "",
        "Brightness",
    ] {
        assert_eq!(WritableField::from_name(name), None, "{name:?}");
    }
    for f in WritableField::ALL {
        assert!(
            ![0x02, 0x05, 0x06, 0x08, 0x7FFF].contains(&f.key()),
            "{} has a key that is not editable",
            f.name()
        );
    }
}

#[test]
fn ac1_protocol_doc_lists_every_key_with_its_evidence_tag() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocol.md");
    let doc = std::fs::read_to_string(&path).expect("docs/protocol.md");
    let start = doc
        .find("### Settings write")
        .expect("a Settings write section");
    let section = &doc[start..];
    let section = &section[..section[4..].find("\n#").map_or(section.len(), |i| i + 4)];
    for key in SETTINGS_WRITE_KEYS {
        let cell = format!("| {key:#04x} |");
        let cell_upper = format!("| 0x{key:02X} |");
        let row = section
            .lines()
            .find(|l| l.starts_with(&cell) || l.starts_with(&cell_upper))
            .unwrap_or_else(|| panic!("no table row for key {key:#x} in Settings write"));
        assert!(row.contains("[static]"), "row without evidence tag: {row}");
    }
    assert!(section.contains("0x08"), "factory reset key is documented");
    assert!(
        section.contains("auto-rotate"),
        "auto-rotate gap documented"
    );
}

// ---- AC-JKWT3W.2: the frame builder ----

#[test]
fn ac2_builder_matches_the_documented_worked_example() {
    // id 1, Chinese, brightness 50, everything else 0. Built from a block
    // that differs in language, rotation and rate, so those are changes.
    let current = decode(&block(&[("rotation", 0), ("sample_rate", 0)]));
    let f =
        settings_write_request(1, &current, &[(WritableField::Language, 0)]).expect("valid change");
    let want: Vec<u8> = [
        &[0x0a, 0x01, 0x00, 0x04][..],
        &[0x01, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        &[0x03, 0x80, 0x00, 0x01, 0x32, 0x00, 0x00, 0x00],
        &[0x04, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        &[0x06, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        &[0x07, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        &[0x14, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        &[0x15, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        &[0xff, 0x7f, 0x00, 0x01, 0x34, 0x12, 0x00, 0x00],
    ]
    .concat();
    assert_eq!(f, want);
    assert_eq!(f.len(), 4 + 8 * 8);
}

#[test]
fn ac2_unchanged_keys_go_out_at_the_values_just_read() {
    let current = decode(&captured());
    let f = settings_write_request(7, &current, &[(WritableField::Brightness, 80)]).unwrap();
    assert_eq!(f, expected_write(7, &captured_entries(&[(0x03, 80)])));
    assert_eq!(&f[..4], &[0x0a, 0x07, 0x00, 0x04]);
}

#[test]
fn ac2_every_field_can_be_changed_at_once() {
    let current = decode(&captured());
    let f = settings_write_request(
        2,
        &current,
        &[
            (WritableField::Rotation, 3),
            (WritableField::Language, 0),
            (WritableField::Brightness, 100),
            (WritableField::DimTimeout, 3),
            (WritableField::Theme, 6),
            (WritableField::SampleRate, 0),
        ],
    )
    .unwrap();
    assert_eq!(
        f,
        expected_write(
            2,
            &[
                (0x01, 0),
                (0x03, 100),
                (0x04, 3),
                (0x06, 0),
                (0x07, 6),
                (0x14, 0),
                (0x15, 3),
                (0x7FFF, 0x1234),
            ]
        )
    );
}

#[test]
fn ac2_the_startup_logo_goes_out_at_its_read_value() {
    for logo in 0..=3u32 {
        let current = decode(&block(&[("startup_logo", logo)]));
        let f = settings_write_request(3, &current, &[(WritableField::Theme, 2)]).unwrap();
        assert_eq!(
            f,
            expected_write(3, &captured_entries(&[(0x06, logo as i32), (0x07, 2)])),
            "logo {logo}"
        );
    }
}

#[test]
fn ac2_every_read_code_is_sent_back_unchanged() {
    let current = decode(&block(&[
        ("language", 0),
        ("brightness", 17),
        ("dim_timeout", 2),
        ("theme", 5),
        ("rotation", 2),
        ("sample_rate", 3),
    ]));
    let f = settings_write_request(9, &current, &[(WritableField::Brightness, 18)]).unwrap();
    assert_eq!(
        f,
        expected_write(
            9,
            &[
                (0x01, 0),
                (0x03, 18),
                (0x04, 2),
                (0x06, 0),
                (0x07, 5),
                (0x14, 3),
                (0x15, 2),
                (0x7FFF, 0x1234),
            ]
        )
    );
}

// ---- AC-JKWT3W.3: ranges ----

#[test]
fn ac3_each_field_has_its_documented_range() {
    let ranges: Vec<(&str, i32, i32)> = WritableField::ALL
        .iter()
        .map(|f| (f.name(), *f.range().start(), *f.range().end()))
        .collect();
    assert_eq!(
        ranges,
        vec![
            ("language", 0, 1),
            ("brightness", 1, 100),
            ("dim_timeout", 0, 3),
            ("theme", 0, 6),
            ("sample_rate", 0, 3),
            ("rotation", 0, 3),
        ]
    );
}

#[test]
fn ac3_builder_refuses_values_out_of_range() {
    let current = decode(&captured());
    for f in WritableField::ALL {
        let (lo, hi) = (*f.range().start(), *f.range().end());
        for bad in [lo - 1, hi + 1, i32::MIN, i32::MAX] {
            let err = settings_write_request(1, &current, &[(f, bad)])
                .expect_err(&format!("{} = {bad} must be refused", f.name()));
            let msg = err.to_string();
            assert!(msg.contains(f.name()), "names the field: {msg}");
        }
        for good in [lo, hi] {
            assert!(
                settings_write_request(1, &current, &[(f, good)]).is_ok(),
                "{} = {good}",
                f.name()
            );
        }
    }
}

// ---- AC-JKWT3W.2 / .4: the session write and the re-read ----

type Guarded = Session<GuardedTransport<MockTransport>>;

/// Guarded session: SETTINGS read (id 1) of `before`, then `rest`.
fn session(before: &[u8], rest: Vec<Step>) -> Guarded {
    let script = [
        vec![Step::transact(
            get_data(1, 0x008),
            settings_reply(1, before),
        )],
        rest,
    ]
    .concat();
    Session::open(GuardedTransport::new(MockTransport::new(script)))
}

fn written(s: &Guarded) -> &[Vec<u8>] {
    s.transport().inner().written()
}

fn settings_writes(s: &Guarded) -> Vec<Vec<u8>> {
    written_of_type(written(s), 0x0A)
}

#[test]
fn ac2_write_reads_then_sends_one_frame_then_reads_again() {
    let before = captured();
    let after = block(&[("brightness", 80)]);
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut s = session(
        &before,
        vec![
            Step::transact(frame.clone(), accept(2)),
            Step::transact(get_data(3, 0x008), settings_reply(3, &after)),
        ],
    );
    let m = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect("write succeeds");
    assert_eq!(m.brightness.value, 80);
    assert_eq!(m, decode(&after));
    assert_eq!(
        written(&s),
        &[get_data(1, 0x008), frame, get_data(3, 0x008)]
    );
    assert_eq!(s.transport().inner().remaining(), 0);
}

#[test]
fn ac2_a_rate_change_is_written_and_read_back() {
    let after = block(&[("sample_rate", 1), ("theme", 3)]);
    let frame = expected_write(2, &captured_entries(&[(0x07, 3), (0x14, 1)]));
    let mut s = session(
        &captured(),
        vec![
            Step::transact(frame.clone(), accept(2)),
            Step::transact(get_data(3, 0x008), settings_reply(3, &after)),
        ],
    );
    s.write_settings(&[(WritableField::SampleRate, 1), (WritableField::Theme, 3)])
        .expect("write succeeds");
    assert_eq!(settings_writes(&s), vec![frame]);
}

#[test]
fn ac3_an_out_of_range_change_sends_no_write() {
    for (f, bad) in [
        (WritableField::Brightness, 0),
        (WritableField::Brightness, 101),
        (WritableField::Theme, 7),
        (WritableField::Language, 2),
        (WritableField::Rotation, -1),
        (WritableField::SampleRate, 4),
        (WritableField::DimTimeout, 4),
    ] {
        let mut s = session(&captured(), vec![]);
        let err = s.write_settings(&[(f, bad)]).expect_err("refused");
        assert!(err.to_string().contains(f.name()), "{err}");
        assert!(
            settings_writes(&s).is_empty(),
            "{}: a write was sent",
            f.name()
        );
        assert!(written(&s).len() <= 1, "at most the settings read");
    }
}

#[test]
fn ac3_the_write_passes_the_guard_only_on_its_own_path() {
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    // The shared path refuses it before any byte is written.
    let mut g = GuardedTransport::new(MockTransport::new(vec![]));
    assert!(
        g.transact(&frame, std::time::Duration::from_millis(10))
            .is_err()
    );
    assert!(g.inner().written().is_empty());
    // The settings path lets it through.
    let mut g = GuardedTransport::new(MockTransport::new(vec![Step::transact(
        frame.clone(),
        accept(2),
    )]));
    assert_eq!(
        g.transact_settings(&frame, std::time::Duration::from_millis(10)),
        Ok(accept(2))
    );
    assert_eq!(g.inner().written(), &[frame]);
}

#[test]
fn ac4_a_re_read_without_the_change_is_an_error_naming_the_field() {
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut s = session(
        &captured(),
        vec![
            Step::transact(frame, accept(2)),
            Step::transact(get_data(3, 0x008), settings_reply(3, &captured())),
        ],
    );
    let err = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect_err("the meter kept 50");
    let msg = err.to_string();
    assert!(msg.contains("brightness"), "{msg}");
    assert!(msg.contains("80") && msg.contains("50"), "{msg}");
    assert_eq!(settings_writes(&s).len(), 1, "the write is not retried");
    assert_eq!(written(&s).len(), 3, "nothing after the re-read");
    assert_eq!(s.transport().inner().remaining(), 0);
}

#[test]
fn ac4_a_re_read_where_another_field_moved_is_an_error_naming_it() {
    for (moved, code) in [("theme", 4u32), ("auto_rotate", 0), ("startup_logo", 1)] {
        let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
        let after = block(&[("brightness", 80), (moved, code)]);
        let mut s = session(
            &captured(),
            vec![
                Step::transact(frame, accept(2)),
                Step::transact(get_data(3, 0x008), settings_reply(3, &after)),
            ],
        );
        let err = s
            .write_settings(&[(WritableField::Brightness, 80)])
            .expect_err(&format!("{moved} moved"));
        assert!(err.to_string().contains(moved), "{moved}: {err}");
        assert_eq!(settings_writes(&s).len(), 1);
    }
}

#[test]
fn ac4_a_re_read_where_the_calibration_moved_is_an_error() {
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut after = block(&[("brightness", 80)]);
    after[0x58..0x5C].copy_from_slice(&81i32.to_le_bytes());
    fix_crcs(&mut after);
    let mut s = session(
        &captured(),
        vec![
            Step::transact(frame, accept(2)),
            Step::transact(get_data(3, 0x008), settings_reply(3, &after)),
        ],
    );
    let err = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect_err("offset calibration moved");
    assert!(err.to_string().contains("offset_cal"), "{err}");
}

#[test]
fn ac4_a_rejected_write_is_an_error_and_is_not_retried() {
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut s = session(&captured(), vec![Step::transact(frame, reject(2))]);
    assert!(
        s.write_settings(&[(WritableField::Brightness, 80)])
            .is_err()
    );
    assert_eq!(settings_writes(&s).len(), 1);
    assert_eq!(written(&s).len(), 2, "no re-read and no retry");
}

// ---- AC-JKWT3W.2 / .4: replies the meter may send to 0x0A ----
//
// The vendor app never reads a reply to the settings write `[static]`, and
// the meter answered the record-table write with FINISHED `[device]`. So
// ACCEPT and FINISHED both count, and no reply at all is followed by one
// read-back, which alone decides.

#[test]
fn ac2_a_finished_reply_is_success() {
    let after = block(&[("brightness", 80)]);
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut s = session(
        &captured(),
        vec![
            Step::transact(frame.clone(), finished(2)),
            Step::transact(get_data(3, 0x008), settings_reply(3, &after)),
        ],
    );
    let m = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect("FINISHED is success");
    assert_eq!(m, decode(&after));
    assert_eq!(s.transport().inner().remaining(), 0);
}

#[test]
fn ac2_no_reply_is_followed_by_one_read_back_that_decides() {
    let after = block(&[("brightness", 80)]);
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut s = session(
        &captured(),
        vec![
            Step::transact_err(frame.clone(), powerz_protocol::transport::Error::Timeout),
            Step::transact(get_data(3, 0x008), settings_reply(3, &after)),
        ],
    );
    let m = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect("a matching read-back after silence is success");
    assert_eq!(m, decode(&after));
    assert_eq!(
        written(&s),
        &[get_data(1, 0x008), frame, get_data(3, 0x008)],
        "one write, one read-back, nothing retried"
    );
    assert_eq!(s.transport().inner().remaining(), 0);
}

#[test]
fn ac4_no_reply_and_a_read_back_without_the_change_is_a_mismatch() {
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut s = session(
        &captured(),
        vec![
            Step::transact_err(frame, powerz_protocol::transport::Error::Timeout),
            Step::transact(get_data(3, 0x008), settings_reply(3, &captured())),
        ],
    );
    let err = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect_err("not applied");
    assert!(err.to_string().contains("brightness"), "{err}");
    assert_eq!(settings_writes(&s).len(), 1, "the write is not retried");
    assert_eq!(written(&s).len(), 3);
}

#[test]
fn ac2_a_late_reply_to_the_write_is_skipped_by_the_read_back() {
    // Silence within the timeout, then the FINISHED arrives in place of the
    // read-back's reply.
    let after = block(&[("brightness", 80)]);
    let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
    let mut s = session(
        &captured(),
        vec![
            Step::transact_err(frame, powerz_protocol::transport::Error::Timeout),
            Step::transact(get_data(3, 0x008), finished(2)),
            Step::read(settings_reply(3, &after)),
        ],
    );
    let m = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect("stale reply skipped");
    assert_eq!(m, decode(&after));
    assert_eq!(s.transport().inner().remaining(), 0);
}

#[test]
fn ac4_any_other_reply_to_the_write_is_an_error_without_a_read_back() {
    for reply in [vec![0x03, 2, 0, 0], vec![0x41, 2, 0, 0]] {
        let frame = expected_write(2, &captured_entries(&[(0x03, 80)]));
        let mut s = session(&captured(), vec![Step::transact(frame, reply.clone())]);
        assert!(
            s.write_settings(&[(WritableField::Brightness, 80)])
                .is_err(),
            "{reply:02x?}"
        );
        assert_eq!(written(&s).len(), 2, "{reply:02x?}: no re-read");
    }
}
