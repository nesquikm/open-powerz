//! FR JT38C8 (AC .6): `docs/protocol.md` states the mode facts the old
//! bootloader fix rests on, each with its confidence tag: the vendor app's
//! rule (string index 2), the 2.1 bootloader descriptor from the field
//! report, the vendor WinUSB INF binding the whole non-composite 0x0063
//! device, and our own classification rule.
//!
//! A "chunk" is one table row, or one paragraph of prose (lines up to a
//! blank line); each fact must sit inside one chunk with its tag.

use std::path::PathBuf;

fn doc() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocol.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Table rows and prose paragraphs outside fenced code blocks.
fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut para = String::new();
    let mut in_fence = false;
    let flush = |para: &mut String, out: &mut Vec<String>| {
        if !para.trim().is_empty() {
            out.push(std::mem::take(para));
        }
        para.clear();
    };
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            flush(&mut para, &mut out);
            continue;
        }
        if in_fence {
            continue;
        }
        let t = line.trim();
        if t.is_empty() {
            flush(&mut para, &mut out);
        } else if t.starts_with('|') {
            flush(&mut para, &mut out);
            out.push(t.to_owned());
        } else {
            para.push(' ');
            para.push_str(t);
        }
    }
    flush(&mut para, &mut out);
    out
}

/// The first chunk holding every one of `needles` (case-insensitive).
fn chunk_with(needles: &[&str]) -> Option<String> {
    chunks(&doc()).into_iter().find(|c| {
        let lower = c.to_lowercase();
        needles.iter().all(|n| lower.contains(&n.to_lowercase()))
    })
}

#[test]
fn jt38c8_doc_states_the_vendor_app_mode_rule_as_static() {
    let c = chunk_with(&[
        "string",
        "2",
        "`APP`",
        "application",
        "anything else",
        "bootloader",
    ])
    .expect("a chunk stating the vendor app's string-2 rule");
    assert!(c.contains("[static]"), "{c}");
}

#[test]
fn jt38c8_doc_states_the_2_1_bootloader_descriptor_as_device() {
    let c = chunk_with(&["2.1", "POWER-Z KM003C", "0xFF", "subclass", "HID"])
        .expect("a chunk giving the 2.1 bootloader descriptor (class, subclass, string, no HID)");
    assert!(c.contains("[device]"), "{c}");
}

#[test]
fn jt38c8_doc_states_the_inf_binds_the_whole_non_composite_device_as_static() {
    let c = chunk_with(&["INF", "0x0063", "composite"])
        .expect("a chunk on the vendor WinUSB INF binding the non-composite 0x0063 device");
    assert!(c.to_lowercase().contains("winusb"), "{c}");
    assert!(c.contains("[static]"), "{c}");
}

#[test]
fn jt38c8_doc_states_our_rule_for_the_old_bootloader() {
    let c = chunk_with(&["old bootloader", "HID", "`APP`", "boot", "dfu"]).expect(
        "a chunk stating our rule: a non-APP, non-boot/dfu string and no HID is the old bootloader",
    );
    assert!(c.contains("legacy_bootloader"), "names the mode: {c}");
}

#[test]
fn jt38c8_doc_no_longer_calls_every_other_string_unknown() {
    let text = doc();
    assert!(
        !text.contains("BOOT or DFU strings as bootloader and anything else as unknown"),
        "the old rule is still stated"
    );
}
