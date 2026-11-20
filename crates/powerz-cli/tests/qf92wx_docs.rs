//! FR QF92WX, AC .7: the CLI reference documents the new probe output
//! (endpoints, string descriptors, timeout direction, the JSON keys) and the
//! read-only variants; the firmware how-to's old-bootloader section asks for
//! the probe output with `--trace` and `--json`, naming what it now holds.

use std::path::PathBuf;

fn doc(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn cli_reference() -> String {
    doc("docs/reference/cli.md")
}

/// The `firmware probe` row of the command table.
fn probe_row(cli: &str) -> String {
    cli.lines()
        .find(|l| l.starts_with("| `firmware probe`"))
        .expect("a `firmware probe` row in docs/reference/cli.md")
        .to_owned()
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

#[test]
fn qf92wx_cli_reference_probe_row_documents_endpoints_and_strings() {
    let row = probe_row(&cli_reference()).to_lowercase();
    assert!(row.contains("endpoint"), "endpoints: {row}");
    assert!(
        row.contains("string descriptor"),
        "string descriptors: {row}"
    );
    assert!(row.contains("max packet"), "endpoint fields: {row}");
    assert!(row.contains("interval"), "endpoint fields: {row}");
}

#[test]
fn qf92wx_cli_reference_documents_the_timeout_direction() {
    let cli = cli_reference();
    assert!(cli.contains("OUT write"), "OUT write");
    assert!(cli.contains("IN read"), "IN read");
}

#[test]
fn qf92wx_cli_reference_documents_the_variants() {
    let cli = cli_reference();
    for name in [
        "`no-drain-no-clear-halt`",
        "`in-before-out`",
        "`all-in-endpoints`",
    ] {
        assert!(cli.contains(name), "variant {name} documented");
    }
    assert!(
        cli.contains("First variant to answer"),
        "the first-answer line documented"
    );
}

#[test]
fn qf92wx_cli_reference_documents_the_new_json_keys() {
    let row = probe_row(&cli_reference());
    for key in [
        "`endpoints`",
        "`strings`",
        "`connect_timeout`",
        "`head_timeout`",
        "`variants`",
        "`variant_answered`",
    ] {
        assert!(row.contains(key), "JSON key {key} in the probe row: {row}");
    }
}

#[test]
fn qf92wx_cli_reference_documents_the_probe_answer_sim_knob() {
    let cli = cli_reference();
    assert!(
        cli.contains("`POWERZ_SIM_PROBE_ANSWER`"),
        "the test-only knob is in the sim table"
    );
}

#[test]
fn qf92wx_howto_old_bootloader_asks_for_trace_and_json_probe_output() {
    let section = old_bootloader_section();
    assert!(
        section.contains("powerz firmware probe --json"),
        "{section}"
    );
    assert!(
        section.contains("powerz --trace firmware probe"),
        "{section}"
    );
}

#[test]
fn qf92wx_howto_old_bootloader_names_what_the_probe_output_holds() {
    let section = old_bootloader_section().to_lowercase();
    assert!(section.contains("endpoint"), "endpoints: {section}");
    assert!(section.contains("string descriptor"), "strings: {section}");
    assert!(section.contains("variant"), "variants: {section}");
}
