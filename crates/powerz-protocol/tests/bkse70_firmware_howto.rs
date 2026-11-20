//! FR BKSE70 (AC .6): the firmware how-to has the old-bootloader section the
//! old-bootloader error links (`#old-bootloader`). It says what the user
//! sees, how to get the meter back to normal, that the official Windows
//! software is the way to update for now, and which read-only commands to
//! send with a report.

use std::path::PathBuf;

fn howto() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/how-to/update-firmware.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// GitHub's anchor for a heading: lowercase, spaces to hyphens, other
/// punctuation dropped.
fn anchor(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// The body of the section whose heading anchors to `old-bootloader`, up to
/// the next heading of the same or a higher level.
fn old_bootloader_section() -> String {
    let text = howto();
    let mut level = None;
    let mut body = String::new();
    for line in text.lines() {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        let is_heading = hashes > 0 && line[hashes..].starts_with(' ');
        if let Some(l) = level {
            if is_heading && hashes <= l {
                break;
            }
            body.push_str(line);
            body.push('\n');
        } else if is_heading && anchor(&line[hashes..]) == "old-bootloader" {
            level = Some(hashes);
        }
    }
    assert!(
        level.is_some(),
        "docs/how-to/update-firmware.md has no heading with the anchor old-bootloader"
    );
    body
}

#[test]
fn bkse70_the_how_to_has_a_section_with_the_old_bootloader_anchor() {
    let body = old_bootloader_section();
    assert!(!body.trim().is_empty(), "the section is empty");
}

#[test]
fn bkse70_the_section_says_what_the_user_sees() {
    let body = old_bootloader_section().to_lowercase();
    assert!(
        body.contains("green"),
        "no flashing green bar named:\n{body}"
    );
    assert!(
        body.contains("not supported yet"),
        "no not-supported message:\n{body}"
    );
}

#[test]
fn bkse70_the_section_says_how_to_get_back_to_normal() {
    let body = old_bootloader_section().to_lowercase();
    assert!(body.contains("unplug"), "no unplug step:\n{body}");
    assert!(
        body.contains("application firmware"),
        "no return to the application firmware:\n{body}"
    );
}

#[test]
fn bkse70_the_section_names_the_official_windows_software_for_updating_for_now() {
    let body = old_bootloader_section().to_lowercase();
    assert!(
        body.contains("windows"),
        "no Windows software named:\n{body}"
    );
    assert!(
        body.contains("official"),
        "not called the official software:\n{body}"
    );
}

#[test]
fn bkse70_the_section_lists_the_read_only_commands_for_a_report() {
    let body = old_bootloader_section();
    assert!(
        body.contains("powerz firmware probe"),
        "no probe command:\n{body}"
    );
    assert!(
        body.to_lowercase().contains("report"),
        "no report named:\n{body}"
    );
    assert!(
        body.to_lowercase().contains("read-only"),
        "the commands are not called read-only:\n{body}"
    );
}
