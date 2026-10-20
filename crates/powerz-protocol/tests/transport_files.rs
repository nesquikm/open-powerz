//! Repo files shipped with the transports (FR SJGK7E, AC .9): the Linux udev
//! rule and the Linux setup doc.

use std::path::PathBuf;

fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read_repo_file(rel: &str) -> String {
    let path = repo_file(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Non-comment, non-blank lines of a udev rules file, lowercased.
fn rule_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

fn has_uaccess_rule(lines: &[String], pid: &str) -> bool {
    lines.iter().any(|l| {
        l.contains("idvendor}==\"5fc9\"")
            && l.contains(&format!("idproduct}}==\"{pid}\""))
            && l.contains("tag+=\"uaccess\"")
    })
}

#[test]
fn transport_udev_rule_grants_uaccess_for_both_models() {
    let lines = rule_lines(&read_repo_file("packaging/linux/60-powerz.rules"));
    assert!(
        has_uaccess_rule(&lines, "0063"),
        "no uaccess rule for 5fc9:0063"
    );
    assert!(
        has_uaccess_rule(&lines, "0061"),
        "no uaccess rule for 5fc9:0061"
    );
}

#[test]
fn transport_udev_rule_sets_modem_manager_ignore() {
    let lines = rule_lines(&read_repo_file("packaging/linux/60-powerz.rules"));
    assert!(
        lines
            .iter()
            .any(|l| l.contains("idvendor}==\"5fc9\"")
                && l.contains("env{id_mm_device_ignore}=\"1\"")),
        "no ENV{{ID_MM_DEVICE_IGNORE}}=\"1\" for vendor 5fc9"
    );
}

#[test]
fn transport_linux_doc_explains_rule_install_and_hwmon_detach() {
    let doc = read_repo_file("docs/linux.md");
    let lower = doc.to_lowercase();
    assert!(
        doc.contains("60-powerz.rules"),
        "doc must name the rule file"
    );
    assert!(
        lower.contains("/etc/udev/rules.d"),
        "doc must say where to install the rule"
    );
    assert!(
        lower.contains("udevadm"),
        "doc must say how to reload udev rules"
    );
    assert!(
        lower.contains("powerz") && lower.contains("hwmon"),
        "doc must name the powerz hwmon driver"
    );
    assert!(
        lower.contains("unbind") || lower.contains("rmmod") || lower.contains("modprobe -r"),
        "doc must explain detaching the powerz hwmon driver"
    );
}
