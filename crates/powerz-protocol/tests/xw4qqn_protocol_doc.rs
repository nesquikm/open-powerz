//! FR XW4QQN (AC .8): `docs/protocol.md` states, each with a confidence
//! tag, that bootloader 2.1 answers with its own reply counter, answers the
//! first CONNECT late and fails the pipe recovery, and that the vendor app
//! matches no reply ids.
//!
//! A "chunk" is one table row, or one paragraph of prose (lines up to a
//! blank line); each fact must sit inside one chunk with its tag.

use std::path::PathBuf;

fn doc() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocol.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Table rows, list items and prose paragraphs outside fenced code blocks.
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
            if t.starts_with("- ") {
                flush(&mut para, &mut out);
            }
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

fn assert_tagged(c: &str) {
    assert!(
        ["[device]", "[static]", "[inferred]"]
            .iter()
            .any(|t| c.contains(t)),
        "no confidence tag in: {c}"
    );
}

#[test]
fn xw4qqn_doc_says_the_2_1_bootloader_answers_with_its_own_reply_counter() {
    let c = chunk_with(&["2.1", "own", "counter"])
        .expect("a chunk saying bootloader 2.1 numbers its replies with its own counter");
    assert_tagged(&c);
}

#[test]
fn xw4qqn_doc_says_the_2_1_bootloader_answers_the_first_connect_late() {
    let c = chunk_with(&["2.1", "first connect", "late"])
        .expect("a chunk saying bootloader 2.1 answers the first CONNECT late");
    assert_tagged(&c);
}

#[test]
fn xw4qqn_doc_says_the_2_1_bootloader_fails_the_pipe_recovery() {
    let c = chunk_with(&["2.1", "recovery", "fail"])
        .expect("a chunk saying the pipe recovery fails on bootloader 2.1");
    assert_tagged(&c);
}

#[test]
fn xw4qqn_doc_says_the_vendor_app_matches_no_reply_ids() {
    let c = chunk_with(&["vendor app", "matches no reply id"])
        .expect("a chunk saying the vendor app matches no reply ids");
    assert!(c.contains("[static]"), "{c}");
}
