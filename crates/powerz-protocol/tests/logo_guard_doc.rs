//! FR R1ST8Z, AC-R1ST8Z.8: `docs/protocol.md` lists the three guarded logo
//! paths (flash chunk, startup flag, read window) in its safe-command
//! section ("Never-send commands"), with their limits, each fact tagged.

use std::path::PathBuf;

const TAGS: [&str; 4] = ["[device]", "[static]", "[doc]", "[inferred]"];

fn doc() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocol.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The "Never-send commands" section, up to the next `## ` heading.
fn safe_section() -> String {
    let doc = doc();
    let start = doc
        .find("\n## Never-send commands")
        .expect("docs/protocol.md has a Never-send commands section");
    let rest = &doc[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |i| i + 3);
    rest[..end].to_owned()
}

/// Paragraphs and table rows of the section (a table row is its own unit).
fn units(section: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut para = String::new();
    for line in section.lines() {
        if line.trim_start().starts_with('|') {
            if !para.is_empty() {
                out.push(std::mem::take(&mut para));
            }
            out.push(line.to_owned());
        } else if line.trim().is_empty() {
            if !para.is_empty() {
                out.push(std::mem::take(&mut para));
            }
        } else {
            para.push_str(line);
            para.push(' ');
        }
    }
    if !para.is_empty() {
        out.push(para);
    }
    out
}

/// The units that mention any of `needles` (case-insensitive).
fn units_with(needles: &[&str]) -> Vec<String> {
    units(&safe_section())
        .into_iter()
        .filter(|u| {
            let l = u.to_lowercase();
            needles.iter().any(|n| l.contains(&n.to_lowercase()))
        })
        .collect()
}

fn assert_mentions(units: &[String], what: &str, facts: &[&str]) {
    let all = units.join("\n").to_lowercase();
    for fact in facts {
        assert!(
            all.contains(&fact.to_lowercase()),
            "safe-command section: the {what} path lacks {fact:?}\n{all}"
        );
    }
}

fn assert_tagged(units: &[String], what: &str) {
    assert!(
        !units.is_empty(),
        "safe-command section says nothing of the {what} path"
    );
    for u in units {
        assert!(
            TAGS.iter().any(|t| u.contains(t)),
            "untagged {what} fact in the safe-command section: {u}"
        );
    }
}

#[test]
fn r1st8z_doc_lists_the_logo_chunk_path_with_its_limits() {
    let u = units_with(&["0x300000", "0x31D500"]);
    assert_mentions(
        &u,
        "logo chunk",
        &["0x4A", "0x300000", "0x31D500", "256", "16", "CRC"],
    );
    assert_tagged(&u, "logo chunk");
}

#[test]
fn r1st8z_doc_lists_the_logo_flag_path_with_its_values() {
    let u = units_with(&["0x1C"]);
    assert_mentions(&u, "logo flag", &["0x48", "0x1C"]);
    let all = u.join("\n").to_lowercase();
    assert!(
        ["0 or 2", "0 and 2", "2 or 0", "0 (remove) or 2", "0 / 2"]
            .iter()
            .any(|v| all.contains(v)),
        "the logo flag path does not give its two values 0 and 2:\n{all}"
    );
    assert_tagged(&u, "logo flag");
}

#[test]
fn r1st8z_doc_lists_the_logo_read_window() {
    let u = units_with(&["0x98300000", "0x9831FFFF"]);
    assert_mentions(&u, "logo read", &["0x98300000", "0x9831FFFF", "256"]);
    assert_tagged(&u, "logo read");
}
