//! FR NCH7DW, AC .3 (docs): the firmware how-to's old-bootloader section
//! and the CLI reference say that open-powerz never sends a write below
//! 0x4000, and that bootloader 2.1 keeping that layout and taking a retry
//! after a failed run is expected but not verified; neither states either
//! as fact.

use std::path::PathBuf;

const FLAG: &str = "--experimental-old-bootloader";

fn doc(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Markdown emphasis and code marks dropped, line breaks joined, lowercase.
fn plain(s: &str) -> String {
    s.replace(['*', '`'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The `## Old bootloader` section of the firmware how-to.
fn howto_old_bootloader() -> String {
    let howto = doc("docs/how-to/update-firmware.md");
    let start = howto
        .find("## Old bootloader")
        .expect("an `## Old bootloader` section");
    let rest = &howto[start + 3..];
    let end = rest.find("\n## ").map_or(rest.len(), |i| i + 3);
    howto[start..start + end].to_owned()
}

/// The CLI reference's text on the flag: the `firmware flash` row and the
/// `--experimental-old-bootloader` item of `## Firmware flashing`.
fn cli_flag_text() -> String {
    let cli = doc("docs/reference/cli.md");
    let row = cli
        .lines()
        .find(|l| l.starts_with("| `firmware flash`"))
        .expect("a `firmware flash` row in docs/reference/cli.md")
        .to_owned();
    let marker = format!("- **`{FLAG}`.**");
    let start = cli
        .find(&marker)
        .unwrap_or_else(|| panic!("a `{marker}` item in docs/reference/cli.md"));
    let rest = &cli[start + 2..];
    let end = [rest.find("\n- "), rest.find("\n\n")]
        .into_iter()
        .flatten()
        .min()
        .map_or(rest.len(), |i| i + 2);
    format!("{row}\n{}", &cli[start..start + end])
}

fn assert_hedged(name: &str, text: &str) {
    let t = plain(text);
    assert!(
        t.contains("never sends a write below 0x4000"),
        "{name}: does not say open-powerz never sends a write below 0x4000:\n{t}"
    );
    assert!(
        t.contains("not verified"),
        "{name}: does not say the 2.1 layout and retry are not verified:\n{t}"
    );
    assert!(
        t.contains("expected"),
        "{name}: does not say the 2.1 layout and retry are expected:\n{t}"
    );
}

fn assert_no_fact(name: &str, text: &str) {
    let t = plain(text);
    assert!(
        !t.contains("is never written"),
        "{name}: still states the region below 0x4000 is never written:\n{t}"
    );
    assert!(
        !t.contains("so a failed run can be retried") && !t.contains("so a failed run leaves"),
        "{name}: still states the retry as fact:\n{t}"
    );
}

#[test]
fn nch7dw_the_how_to_hedges_the_old_bootloader_layout_and_retry() {
    assert_hedged("how-to", &howto_old_bootloader());
}

#[test]
fn nch7dw_the_how_to_states_neither_as_fact() {
    assert_no_fact("how-to", &howto_old_bootloader());
}

#[test]
fn nch7dw_the_cli_reference_hedges_the_old_bootloader_layout_and_retry() {
    assert_hedged("cli reference", &cli_flag_text());
}

#[test]
fn nch7dw_the_cli_reference_states_neither_as_fact() {
    assert_no_fact("cli reference", &cli_flag_text());
}
