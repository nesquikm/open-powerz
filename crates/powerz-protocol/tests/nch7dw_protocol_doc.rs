//! FR NCH7DW, AC .4: `docs/protocol.md` tags the late first CONNECT and
//! the failed pipe recovery on bootloader 2.1 as inferred from the trace,
//! with the reasoning, and its bootloader 2.1 rows no longer say that
//! CONNECT and HEAD time out or that flashing is unsupported.
//!
//! A "chunk" is one table row, or one paragraph of prose (lines up to a
//! blank line).

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

/// Every chunk holding all of `needles` (case-insensitive).
fn chunks_with(needles: &[&str]) -> Vec<String> {
    chunks(&doc())
        .into_iter()
        .filter(|c| {
            let lower = c.to_lowercase();
            needles.iter().all(|n| lower.contains(&n.to_lowercase()))
        })
        .collect()
}

fn one_chunk_with(needles: &[&str]) -> String {
    let found = chunks_with(needles);
    assert!(!found.is_empty(), "no chunk holding {needles:?}");
    found[0].clone()
}

/// Tagged inferred, not device, and reasoning from the trace.
fn assert_inferred_from_the_trace(c: &str) {
    assert!(c.contains("[inferred]"), "not tagged [inferred]: {c}");
    assert!(!c.contains("[device]"), "still tagged [device]: {c}");
    assert!(
        c.to_lowercase().contains("trace"),
        "no reasoning from the trace: {c}"
    );
}

#[test]
fn nch7dw_doc_tags_the_late_first_connect_on_2_1_as_inferred_from_the_trace() {
    let c = one_chunk_with(&["2.1", "first connect", "late"]);
    assert_inferred_from_the_trace(&c);
}

#[test]
fn nch7dw_doc_tags_the_failed_pipe_recovery_on_2_1_as_inferred_from_the_trace() {
    let c = one_chunk_with(&["2.1", "recovery", "fail"]);
    assert_inferred_from_the_trace(&c);
}

#[test]
fn nch7dw_doc_gives_the_reasoning_for_the_failed_recovery() {
    // The trace logs no recovery itself: what shows is that no second
    // CONNECT followed.
    let c = one_chunk_with(&["2.1", "recovery", "fail"]).to_lowercase();
    assert!(
        c.contains("second connect") || c.contains("does not log") || c.contains("not logged"),
        "no reasoning for the inference: {c}"
    );
}

#[test]
fn nch7dw_doc_2_1_table_row_no_longer_says_connect_and_head_time_out() {
    let doc = doc();
    let row = doc
        .lines()
        .find(|l| l.trim_start().starts_with("| 2.1"))
        .expect("the bootloader 2.1 table row");
    let lower = row.to_lowercase();
    assert!(
        !lower.contains("timed out") && !lower.contains("time out"),
        "the 2.1 row still says CONNECT and HEAD time out: {row}"
    );
}

#[test]
fn nch7dw_doc_no_2_1_text_says_flashing_is_unsupported() {
    let stale: Vec<String> = chunks(&doc())
        .into_iter()
        .filter(|c| {
            let lower = c.to_lowercase();
            (lower.contains("2.1") || lower.contains("legacy_bootloader"))
                && (lower.contains("not supported yet") || lower.contains("no flash, no jump"))
        })
        .collect();
    assert!(
        stale.is_empty(),
        "bootloader 2.1 still called unsupported in: {stale:#?}"
    );
}
