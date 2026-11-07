//! AC-Z39H24.11: `docs/protocol.md` describes SET_DATA 0x1C as the startup
//! duration, the stale block A check value after it, and the menu save that
//! repairs it, with the device findings tagged `[device]`.

use std::path::PathBuf;

fn doc() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocol.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Paragraphs, bullets and table rows outside code fences.
fn units(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut fence = false;
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.trim().is_empty() {
            out.push(std::mem::take(cur));
        }
        cur.clear();
    };
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        let t = line.trim_start();
        if t.is_empty() || t.starts_with("- ") || t.starts_with('|') || t.starts_with('#') {
            flush(&mut cur, &mut out);
        }
        cur.push_str(line);
        cur.push('\n');
        if t.starts_with('|') {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn units_with(all: &[&str]) -> Vec<String> {
    units(&doc())
        .into_iter()
        .filter(|u| {
            let l = u.to_lowercase();
            all.iter().all(|n| l.contains(&n.to_lowercase()))
        })
        .collect()
}

#[test]
fn z39h24_11_the_0x1c_path_is_the_startup_duration_tagged_device() {
    let u = units_with(&["0x1C", "SET_DATA", "startup duration"]);
    assert!(
        !u.is_empty(),
        "no paragraph calls the SET_DATA 0x1C write the startup duration"
    );
    let all = u.join("\n");
    assert!(all.contains("[device]"), "untagged:\n{all}");
    assert!(all.contains("3 s"), "2 sets 3 s:\n{all}");
    assert!(all.to_lowercase().contains("off"), "0 turns it off:\n{all}");
    assert!(
        !doc().contains("show the custom logo"),
        "the old flag reading is gone"
    );
}

#[test]
fn z39h24_11_the_stale_check_value_and_the_menu_save_are_described() {
    let u: Vec<String> = units(&doc())
        .into_iter()
        .filter(|u| {
            let l = u.to_lowercase();
            l.contains("0x1c")
                && (l.contains("crc a") || l.contains("block a"))
                && l.contains("menu")
        })
        .collect();
    assert!(
        !u.is_empty(),
        "no paragraph says the 0x1C write leaves block A's CRC stale until a menu save"
    );
    let all = u.join("\n");
    assert!(all.contains("[device]"), "untagged:\n{all}");
    assert!(
        all.to_lowercase().contains("stale") || all.to_lowercase().contains("old"),
        "{all}"
    );
}

#[test]
fn z39h24_11_the_settings_table_ties_bits_14_15_to_0x1c() {
    let rows = units_with(&["bits 14..15", "startup"]);
    assert!(!rows.is_empty(), "no SETTINGS row for bits 14..15");
    let all = rows.join("\n");
    assert!(all.contains("0x1C"), "the row names the 0x1C write:\n{all}");
    assert!(all.contains("[device]"), "{all}");
}
