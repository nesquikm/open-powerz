//! FR DJYNRP, AC .7: the firmware how-to's old-bootloader section and the
//! CLI reference describe `--experimental-old-bootloader`, that the path is
//! tested only in simulation, and how to retry a failed run (hold Back while
//! plugging the meter in).

use std::path::PathBuf;

const FLAG: &str = "--experimental-old-bootloader";

fn doc(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Markdown emphasis and code marks dropped, lowercase.
fn plain(s: &str) -> String {
    s.replace(['*', '`'], "").to_lowercase()
}

/// The `## Old bootloader` section of the firmware how-to.
fn old_bootloader_section() -> String {
    let howto = doc("docs/how-to/update-firmware.md");
    let start = howto
        .find("## Old bootloader")
        .expect("an `## Old bootloader` section");
    let rest = &howto[start + 3..];
    let end = rest.find("\n## ").map_or(rest.len(), |i| i + 3);
    howto[start..start + end].to_owned()
}

/// The `firmware flash` row of the CLI reference's command table, plus its
/// `## Firmware flashing` section.
fn cli_flash_text() -> String {
    let cli = doc("docs/reference/cli.md");
    let row = cli
        .lines()
        .find(|l| l.starts_with("| `firmware flash`"))
        .expect("a `firmware flash` row in docs/reference/cli.md")
        .to_owned();
    let start = cli
        .find("## Firmware flashing")
        .expect("a `## Firmware flashing` section");
    let rest = &cli[start + 3..];
    let end = rest.find("\n## ").map_or(rest.len(), |i| i + 3);
    format!("{row}\n{}", &cli[start..start + end])
}

#[test]
fn djynrp_the_how_to_names_the_flag() {
    let s = old_bootloader_section();
    assert!(
        s.contains(FLAG),
        "the old-bootloader section lacks {FLAG}:\n{s}"
    );
}

#[test]
fn djynrp_the_how_to_says_the_path_is_tested_only_in_simulation() {
    let s = plain(&old_bootloader_section());
    assert!(s.contains("simulat"), "no simulation-only note:\n{s}");
}

#[test]
fn djynrp_the_how_to_says_how_to_retry() {
    let s = plain(&old_bootloader_section());
    assert!(
        s.contains("hold back while plugging"),
        "no hold-Back retry in the old-bootloader section:\n{s}"
    );
}

#[test]
fn djynrp_the_cli_reference_flash_row_names_the_flag() {
    let cli = doc("docs/reference/cli.md");
    let row = cli
        .lines()
        .find(|l| l.starts_with("| `firmware flash`"))
        .expect("a `firmware flash` row");
    assert!(row.contains(FLAG), "the flash row lacks {FLAG}:\n{row}");
}

#[test]
fn djynrp_the_cli_reference_says_the_path_is_tested_only_in_simulation() {
    let t = plain(&cli_flash_text());
    assert!(t.contains("simulat"), "no simulation-only note:\n{t}");
}

#[test]
fn djynrp_the_cli_reference_says_how_to_retry_the_old_bootloader_flash() {
    let t = plain(&cli_flash_text());
    assert!(
        t.contains("hold back while plugging"),
        "no hold-Back retry:\n{t}"
    );
}
