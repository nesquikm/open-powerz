//! AC-AXHK2Q.6 and the last line of .8: `powerz list` and `powerz info` take
//! `--hide-serial`, in text and JSON, and print `hidden` for the serial and
//! the UID; nothing else in their output changes. Everything runs on the
//! simulator, whose serial and UID are synthetic.

use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

const DEMO_SERIAL: &str = "123456";
const OTHER_SERIAL: &str = "SIM-B-0002";

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    c.env_remove("POWERZ_SIM_FAULT");
    c.env_remove("POWERZ_SIM_SERIAL");
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.timeout(Duration::from_secs(60));
    c
}

fn output(args: &[&str]) -> String {
    let assert = powerz().args(args).assert().success();
    String::from_utf8_lossy(&assert.get_output().stdout).into_owned()
}

const SIM: [&str; 4] = ["--transport", "sim", "--scenario", "load-5v-1a"];

fn sim_output(args: &[&str]) -> String {
    let all: Vec<&str> = SIM.iter().chain(args).copied().collect();
    output(&all)
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("valid JSON")
}

/// The value after `label` on its line of `powerz info`.
fn field<'a>(text: &'a str, label: &str) -> &'a str {
    text.lines()
        .find_map(|l| l.strip_prefix(label))
        .unwrap_or_else(|| panic!("no {label} in {text}"))
        .trim()
}

#[test]
fn both_commands_document_the_flag() {
    for command in ["list", "info"] {
        powerz()
            .args([command, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("--hide-serial"));
    }
}

#[test]
fn list_text_prints_hidden_and_changes_nothing_else() {
    let plain = output(&["list", "--include-demo"]);
    let hidden = output(&["list", "--include-demo", "--hide-serial"]);
    // Only the demo line: a real meter on the host joins both listings, and
    // its serial must not reach a failure message.
    let demo = |text: &str| {
        text.lines()
            .find(|l| l.contains("bus demo"))
            .unwrap_or_else(|| panic!("no demo line"))
            .to_owned()
    };
    assert!(demo(&plain).contains(DEMO_SERIAL));
    assert_eq!(demo(&hidden), demo(&plain).replace(DEMO_SERIAL, "hidden"));
    let lines: Vec<&str> = hidden.lines().collect();
    assert_eq!(lines.len(), plain.lines().count());
    assert!(lines.iter().all(|l| l.contains("serial hidden")));
}

#[test]
fn list_json_prints_hidden_and_changes_nothing_else() {
    let plain = json(&output(&["list", "--include-demo", "--json"]));
    let hidden = json(&output(&[
        "list",
        "--include-demo",
        "--json",
        "--hide-serial",
    ]));
    let mut expected = plain.clone();
    for entry in expected.as_array_mut().expect("an array") {
        entry["serial"] = "hidden".into();
    }
    assert_eq!(hidden, expected);
}

#[test]
fn list_hides_a_serial_set_for_another_meter() {
    let assert = powerz()
        .env("POWERZ_SIM_SERIAL", OTHER_SERIAL)
        .args(["list", "--include-demo", "--hide-serial"])
        .assert()
        .success();
    let text = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(!text.contains(OTHER_SERIAL), "{text}");
}

#[test]
fn list_with_the_flag_shows_no_port_name() {
    for args in [
        &["list", "--include-demo", "--hide-serial"][..],
        &["list", "--include-demo", "--hide-serial", "--json"][..],
    ] {
        let text = output(args);
        for needle in ["/dev/", "usbmodem", "ttyACM"] {
            assert!(!text.contains(needle), "{text}");
        }
    }
}

#[test]
fn info_text_prints_hidden_and_changes_nothing_else() {
    let plain = sim_output(&["info"]);
    let uid = field(&plain, "UID:").to_owned();
    let serial = field(&plain, "Serial:").to_owned();
    assert!(!uid.is_empty() && !serial.is_empty(), "{plain}");

    let hidden = sim_output(&["info", "--hide-serial"]);
    assert_eq!(field(&hidden, "Serial:"), "hidden");
    assert_eq!(field(&hidden, "UID:"), "hidden");
    assert_eq!(
        hidden,
        plain.replace(&uid, "hidden").replace(&serial, "hidden")
    );
}

#[test]
fn info_json_prints_hidden_and_changes_nothing_else() {
    let plain = json(&sim_output(&["info", "--json"]));
    let hidden = json(&sim_output(&["info", "--json", "--hide-serial"]));
    let mut expected = plain.clone();
    expected["serial"] = "hidden".into();
    expected["uid"] = "hidden".into();
    assert_eq!(hidden, expected);
}

#[test]
fn without_the_flag_both_commands_still_show_the_values() {
    assert!(output(&["list", "--include-demo"]).contains(DEMO_SERIAL));
    let info = json(&sim_output(&["info", "--json"]));
    assert_eq!(info["serial"], DEMO_SERIAL);
    assert_ne!(info["uid"], "hidden");
}
