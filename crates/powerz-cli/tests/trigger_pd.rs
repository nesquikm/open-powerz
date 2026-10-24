//! `powerz trigger pd list|request|settings|reset` end to end against the
//! simulated meter (FR DGSQ6P, AC .1-.6). No hardware is touched.
//!
//! Test-only knobs for `--transport sim` (besides the e2e_sim ones):
//!
//! - `POWERZ_SIM_CLOCK=fake`: the PDM session runs on the simulated clock,
//!   so reply timeouts and pacing never sleep.
//! - `POWERZ_SIM_CHARGER=<name>`: the charger behind the simulated meter,
//!   `epr-140w` (default, session 2) or `pps-65w` (SPR only, cannot enter EPR).
//!
//! `--trace` logs every PDM command written as a stderr line `> <command>`,
//! which is how these tests see what reached the meter.

use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

const ENV_KNOBS: [&str; 4] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_CHARGER",
];

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(60));
    c
}

/// `powerz --transport sim --trace trigger pd <args>` on the fake clock.
fn trigger(args: &[&str]) -> Command {
    let mut c = powerz();
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.args(["--transport", "sim", "--trace", "trigger", "pd"]);
    c.args(args);
    c
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

/// PDM commands written, in order, from the `--trace` lines.
fn sent(a: &assert_cmd::assert::Assert) -> Vec<String> {
    stderr_of(a)
        .lines()
        .filter_map(|l| l.strip_prefix("> "))
        .map(|l| l.trim().to_string())
        .collect()
}

/// The first stdout line containing every fragment.
fn line_with<'a>(out: &'a str, fragments: &[&str]) -> Option<&'a str> {
    out.lines()
        .find(|l| fragments.iter().all(|f| l.contains(f)))
}

/// Index of `cmd` in `sent`, panicking with the whole list otherwise.
fn index_of(sent: &[String], cmd: &str) -> usize {
    sent.iter()
        .position(|c| c == cmd)
        .unwrap_or_else(|| panic!("{cmd:?} not sent; sent {sent:?}"))
}

/// The session ends at 5 V and closed: `pd req=1` after `after`, then
/// `pdm close` last.
fn ends_safely(sent: &[String], after: usize) {
    assert_eq!(
        sent.last().map(String::as_str),
        Some("pdm close"),
        "{sent:?}"
    );
    let back = sent
        .iter()
        .rposition(|c| c == "pd req=1")
        .unwrap_or_else(|| panic!("no return to 5 V; sent {sent:?}"));
    assert!(back > after, "{sent:?}");
}

mod trigger_pd {
    use super::*;

    // ---- AC-DGSQ6P.6: the subcommand and its help ----

    #[test]
    fn help_lists_the_trigger_pd_subcommands() {
        let a = powerz()
            .args(["trigger", "pd", "--help"])
            .assert()
            .success();
        let out = stdout_of(&a);
        for sub in ["list", "request", "settings", "reset"] {
            assert!(out.contains(sub), "{sub} missing from help:\n{out}");
        }
        let a = powerz()
            .args(["trigger", "pd", "request", "--help"])
            .assert()
            .success();
        let out = stdout_of(&a);
        for flag in [
            "--allow-high-voltage",
            "--voltage-mv",
            "--current-ma",
            "--hold",
            "--json",
        ] {
            assert!(out.contains(flag), "{flag} missing from help:\n{out}");
        }
    }

    // ---- AC-DGSQ6P.1: list ----

    #[test]
    fn list_shows_decoded_offers() {
        let a = trigger(&["list"]).assert().success();
        let out = stdout_of(&a);
        assert!(
            line_with(&out, &["1", "Fixed", "5.00 V", "3.00 A"]).is_some(),
            "{out}"
        );
        assert!(
            line_with(&out, &["4", "Fixed", "20.00 V", "5.00 A"]).is_some(),
            "{out}"
        );
        assert!(
            line_with(&out, &["8", "Fixed", "28.00 V", "5.00 A"]).is_some(),
            "{out}"
        );
        assert!(
            line_with(&out, &["9", "AVS", "15.00-28.00 V", "140 W"]).is_some(),
            "{out}"
        );
        let s = sent(&a);
        index_of(&s, "pdm open");
        index_of(&s, "entry pd");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
    }

    #[test]
    fn list_json_has_position_kind_range_and_current() {
        let a = trigger(&["list", "--json"]).assert().success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["epr"], Value::Bool(true));
        let offers = v["offers"].as_array().expect("offers array");
        let positions: Vec<u64> = offers
            .iter()
            .map(|o| o["position"].as_u64().unwrap())
            .collect();
        assert_eq!(positions, [1, 2, 3, 4, 8, 9]);
        let o8 = &offers[4];
        assert_eq!(o8["kind"], "fixed");
        assert_eq!(o8["min_voltage_mv"], 28_000);
        assert_eq!(o8["max_voltage_mv"], 28_000);
        assert_eq!(o8["max_current_ma"], 5000);
        let o9 = &offers[5];
        assert_eq!(o9["kind"], "epr-avs");
        assert_eq!(o9["min_voltage_mv"], 15_000);
        assert_eq!(o9["max_voltage_mv"], 28_000);
        assert_eq!(o9["max_power_mw"], 140_000);
    }

    #[test]
    fn list_on_a_pps_charger_shows_the_pps_range() {
        let a = trigger(&["list", "--json"])
            .env("POWERZ_SIM_CHARGER", "pps-65w")
            .assert()
            .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["epr"], Value::Bool(false));
        let pps = v["offers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["kind"] == "pps")
            .expect("a pps offer");
        assert_eq!(pps["position"], 5);
        assert_eq!(pps["min_voltage_mv"], 3300);
        assert_eq!(pps["max_voltage_mv"], 21_000);
        assert_eq!(pps["max_current_ma"], 3250);
    }

    // ---- AC-DGSQ6P.2 / .6: requests, high-voltage flag ----

    #[test]
    fn request_above_5v_without_flag_fails_and_writes_no_request() {
        let a = trigger(&["request", "8"]).assert().failure();
        let err = stderr_of(&a);
        assert!(err.contains("--allow-high-voltage"), "{err}");
        let s = sent(&a);
        assert!(!s.iter().any(|c| c.starts_with("pd req=8")), "{s:?}");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
    }

    #[test]
    fn avs_request_above_5v_without_flag_fails() {
        let a = trigger(&[
            "request",
            "9",
            "--voltage-mv",
            "15000",
            "--current-ma",
            "1000",
        ])
        .assert()
        .failure();
        assert!(stderr_of(&a).contains("--allow-high-voltage"));
        assert!(!sent(&a).iter().any(|c| c.starts_with("pd req=9")));
    }

    #[test]
    fn request_5v_needs_no_flag() {
        let a = trigger(&["request", "1", "--hold", "0"]).assert().success();
        let s = sent(&a);
        index_of(&s, "pd req=1");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
    }

    #[test]
    fn fixed_request_reports_requested_and_delivered_then_returns_to_5v() {
        let a = trigger(&["request", "2", "--allow-high-voltage", "--hold", "0"])
            .assert()
            .success();
        let out = stdout_of(&a);
        assert!(
            line_with(&out, &["Requested", "9.00 V", "3.00 A"]).is_some(),
            "{out}"
        );
        assert!(line_with(&out, &["Delivered", "V"]).is_some(), "{out}");
        let s = sent(&a);
        let req = index_of(&s, "pd req=2");
        ends_safely(&s, req);
    }

    #[test]
    fn avs_request_json_reports_the_replies() {
        let a = trigger(&[
            "request",
            "9",
            "--voltage-mv",
            "15000",
            "--current-ma",
            "1000",
            "--allow-high-voltage",
            "--hold",
            "0",
            "--json",
        ])
        .assert()
        .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["command"], "pd req=9,volt=15000,cur=1000");
        assert_eq!(v["requested_mv"], 15_000);
        assert_eq!(v["requested_ma"], 1000);
        let delivered = v["delivered_mv"].as_u64().expect("delivered_mv");
        assert!(delivered.abs_diff(15_000) * 20 <= 15_000, "{delivered}");
        assert!(v["delivered_ma"].is_u64());
        let s = sent(&a);
        let req = index_of(&s, "pd req=9,volt=15000,cur=1000");
        ends_safely(&s, req);
    }

    #[test]
    fn pps_request_on_the_pps_charger() {
        let a = trigger(&[
            "request",
            "5",
            "--voltage-mv",
            "9000",
            "--current-ma",
            "2000",
            "--allow-high-voltage",
            "--hold",
            "0",
            "--json",
        ])
        .env("POWERZ_SIM_CHARGER", "pps-65w")
        .assert()
        .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["requested_mv"], 9000);
        assert_eq!(v["requested_ma"], 2000);
        let s = sent(&a);
        let req = index_of(&s, "pd req=5,volt=9000,cur=2000");
        ends_safely(&s, req);
    }

    #[test]
    fn out_of_range_request_fails_before_writing() {
        let a = trigger(&[
            "request",
            "9",
            "--voltage-mv",
            "30000",
            "--current-ma",
            "1000",
            "--allow-high-voltage",
            "--hold",
            "0",
        ])
        .assert()
        .failure();
        let err = stderr_of(&a);
        assert!(err.contains("30000"), "{err}");
        let s = sent(&a);
        assert!(!s.iter().any(|c| c.starts_with("pd req=9")), "{s:?}");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
    }

    // ---- AC-DGSQ6P.3: EPR ----

    #[test]
    fn epr_28v_request() {
        let a = trigger(&["request", "8", "--allow-high-voltage", "--hold", "0"])
            .assert()
            .success();
        let out = stdout_of(&a);
        assert!(
            line_with(&out, &["Requested", "28.00 V", "5.00 A"]).is_some(),
            "{out}"
        );
        let s = sent(&a);
        let req = index_of(&s, "pd req=8");
        ends_safely(&s, req);
    }

    #[test]
    fn epr_request_on_a_charger_without_epr_is_reported() {
        let a = trigger(&["request", "8", "--allow-high-voltage", "--hold", "0"])
            .env("POWERZ_SIM_CHARGER", "pps-65w")
            .assert()
            .failure();
        let err = stderr_of(&a);
        assert!(err.contains("EPR"), "{err}");
        assert!(!sent(&a).iter().any(|c| c.starts_with("pd req=8")));
    }

    // ---- AC-DGSQ6P.4: settings with named values ----

    #[test]
    fn settings_shows_named_values() {
        let a = trigger(&["settings"]).assert().success();
        let out = stdout_of(&a);
        assert!(out.contains("auto"), "{out}");
        assert!(out.contains("50v-240w-epr"), "{out}");
        assert!(out.contains("pps-3a"), "{out}");
        assert!(sent(&a).contains(&"pdm parm".to_string()));
        let a = trigger(&["settings", "--json"]).assert().success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["type"], "auto");
        assert_eq!(v["em"], "50v-240w-epr");
        assert_eq!(v["sink"], "pps-3a");
    }

    #[test]
    fn settings_change_sends_pdm_set_with_codes() {
        let a = trigger(&["settings", "--em", "off", "--sink", "pps-5a", "--json"])
            .assert()
            .success();
        let s = sent(&a);
        // `--type` not given: the current value (auto) is kept.
        index_of(&s, "pdm set type=0,em=0,sink=1");
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["type"], "auto");
        assert_eq!(v["em"], "off");
        assert_eq!(v["sink"], "pps-5a");
    }

    #[test]
    fn settings_reject_unknown_names() {
        let a = trigger(&["settings", "--em", "3"]).assert().failure();
        let err = stderr_of(&a);
        assert!(err.contains("la135"), "possible values listed: {err}");
        assert!(!sent(&a).iter().any(|c| c.starts_with("pdm set")));
    }

    // ---- AC-DGSQ6P.5: hard reset ----

    #[test]
    fn reset_sends_hard_reset_and_lists_new_offers() {
        let a = trigger(&["reset"]).assert().success();
        let out = stdout_of(&a);
        assert!(
            line_with(&out, &["8", "Fixed", "28.00 V"]).is_some(),
            "{out}"
        );
        assert!(line_with(&out, &["VBUS", "5.1"]).is_some(), "{out}");
        let s = sent(&a);
        let hrst = index_of(&s, "pd hrst");
        assert!(index_of(&s, "entry pd") < hrst);
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
    }
}
