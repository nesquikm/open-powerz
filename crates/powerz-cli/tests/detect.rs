//! `powerz detect` end to end against the simulated meter (FR DGSQ6R).
//! No hardware is touched.
//!
//! `powerz detect [--list | --list-plus] [--stall-secs N] [--json]`:
//! `entry bc` and `entry apple` always, `entry list` with `--list`,
//! `entry list+` with `--list-plus`, D+ / D- from one ADC snapshot for the
//! hint, and the session always ends with `pdm close` (after a stalled
//! list, `reset` first).
//!
//! Knobs as in `trigger_pd.rs`: `POWERZ_SIM_CLOCK=fake` (the 45 s stall
//! passes instantly), `POWERZ_SIM_CHARGER`; `--trace` logs each PDM command
//! as a stderr line `> <command>`. The demo scenario `load-5v-1a` puts
//! 0.6 V on D+ and D-.
//!
//! Test-only knobs for AC-DGSQ6R.7 / .8 (read by the CLI's sim transport,
//! applied by `SimPdm::with_lag_after_list` / `SimPdm::with_entry_bc_fail`):
//! - `POWERZ_SIM_PDM_LAG=after-list`: once `reset` has ended a stalled list,
//!   the simulated meter answers every command with the previous command's
//!   reply (one late), as the real meter did on 2026-10-23.
//! - `POWERZ_SIM_ENTRY_BC=fail`: the simulated meter answers `entry bc` with
//!   `fail` (device trace 2026-10-23) instead of session 2's `ok`.

use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

const ENV_KNOBS: [&str; 6] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_CHARGER",
    "POWERZ_SIM_PDM_LAG",
    "POWERZ_SIM_ENTRY_BC",
];

/// `powerz --transport sim --scenario load-5v-1a --trace detect <args>`
/// on the fake clock.
fn detect(args: &[&str]) -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(60));
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.args([
        "--transport",
        "sim",
        "--scenario",
        "load-5v-1a",
        "--trace",
        "detect",
    ]);
    c.args(args);
    c
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

/// PDM text commands written, in order, from the `--trace` lines (hex
/// frame lines of the ADC read are left out).
fn sent(a: &assert_cmd::assert::Assert) -> Vec<String> {
    String::from_utf8_lossy(&a.get_output().stderr)
        .lines()
        .filter_map(|l| l.strip_prefix("> "))
        .map(|l| l.trim().to_string())
        // Frame lines are `[len] <hex bytes>`.
        .filter(|l| !l.starts_with('['))
        .collect()
}

fn line_with<'a>(out: &'a str, fragments: &[&str]) -> Option<&'a str> {
    out.lines()
        .find(|l| fragments.iter().all(|f| l.contains(f)))
}

fn index_of(sent: &[String], cmd: &str) -> usize {
    sent.iter()
        .position(|c| c == cmd)
        .unwrap_or_else(|| panic!("{cmd:?} not sent; sent {sent:?}"))
}

/// Only detection and session control reach the meter.
fn only_safe_commands(sent: &[String]) {
    // `pdm parm` is the read-only sync check after an incomplete list
    // (AC-DGSQ6R.7).
    const SAFE: [&str; 8] = [
        "pdm parm",
        "pdm open",
        "pdm close",
        "reset",
        "entry bc",
        "entry apple",
        "entry list",
        "entry list+",
    ];
    for c in sent {
        assert!(SAFE.contains(&c.as_str()), "unexpected command {c:?}");
    }
}

mod detect {
    use super::*;

    // ---- AC-DGSQ6R.4: text output ----

    #[test]
    fn text_reports_bc_apple_and_the_hint() {
        let a = detect(&[]).assert().success();
        let out = stdout_of(&a);
        let bc = line_with(&out, &["BC1.2"]).unwrap_or_else(|| panic!("{out}"));
        assert!(bc.contains("unknown"), "{bc}");
        let apple = line_with(&out, &["Apple"]).unwrap_or_else(|| panic!("{out}"));
        assert!(
            apple.contains("not") && apple.contains("supported"),
            "{apple}"
        );
        let hint = out
            .lines()
            .find(|l| l.to_lowercase().contains("hint"))
            .unwrap_or_else(|| panic!("no hint line: {out}"));
        assert!(hint.contains("DCP"), "{hint}");
        let s = sent(&a);
        index_of(&s, "entry bc");
        index_of(&s, "entry apple");
        assert!(!s.iter().any(|c| c.starts_with("entry list")), "{s:?}");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
        only_safe_commands(&s);
    }

    #[test]
    fn text_list_shows_each_line_and_marks_the_stall_incomplete() {
        let a = detect(&["--list"]).assert().success();
        let out = stdout_of(&a);
        let bc = line_with(&out, &["bc1.2"]).unwrap_or_else(|| panic!("{out}"));
        assert!(bc.contains("supported") && bc.contains("5203"), "{bc}");
        let ufcs = line_with(&out, &["ufcs"]).unwrap_or_else(|| panic!("{out}"));
        assert!(ufcs.contains("not"), "{ufcs}");
        assert!(line_with(&out, &["vfcp"]).is_some(), "{out}");
        assert!(out.contains("incomplete"), "{out}");
        let s = sent(&a);
        let list = index_of(&s, "entry list");
        let reset = s
            .iter()
            .rposition(|c| c == "reset")
            .unwrap_or_else(|| panic!("no reset; sent {s:?}"));
        assert!(reset > list, "{s:?}");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
        only_safe_commands(&s);
    }

    // ---- AC-DGSQ6R.4: JSON output ----

    #[test]
    fn json_has_results_list_and_hint() {
        let a = detect(&["--list", "--stall-secs", "5", "--json"])
            .assert()
            .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("one JSON object");
        assert_eq!(v["bc12"], "unknown", "{v}");
        assert_eq!(v["apple"], "not-supported", "{v}");
        let list = &v["list"];
        assert_eq!(list["complete"], false, "{v}");
        let entries = list["entries"].as_array().expect("entries");
        let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
        assert_eq!(names, ["bc1.2", "ufcs", "vfcp"]);
        assert_eq!(entries[0]["support"], "supported");
        assert_eq!(entries[0]["vbus_mv"], 5203);
        assert_eq!(entries[0]["ibus_ma"], 0);
        assert_eq!(entries[1]["support"], "not-supported");
        assert!(entries[1]["vbus_mv"].is_null(), "{v}");
        let hint = &v["hint"];
        let dp = hint["dp_mv"].as_u64().expect("dp_mv");
        let dm = hint["dm_mv"].as_u64().expect("dm_mv");
        assert!(dp.abs_diff(600) <= 10 && dm.abs_diff(600) <= 10, "{hint}");
        let modes: Vec<&str> = hint["modes"]
            .as_array()
            .expect("modes")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(modes.contains(&"DCP"), "{modes:?}");
        assert!(
            hint["label"]
                .as_str()
                .is_some_and(|l| l.starts_with("hint")),
            "{hint}"
        );
        // stdout is the JSON object only.
        assert!(stdout_of(&a).trim_start().starts_with('{'));
    }

    #[test]
    fn json_without_list_has_null_list() {
        let a = detect(&["--json"]).assert().success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("one JSON object");
        assert!(v["list"].is_null(), "{v}");
        assert_eq!(v["bc12"], "unknown");
        assert_eq!(v["apple"], "not-supported");
    }

    // ---- AC-DGSQ6R.2: entry list+ only on request ----

    #[test]
    fn list_warns_that_the_meter_may_raise_vbus_while_probing() {
        let a = detect(&["--list", "--json"]).assert().success();
        let err = stderr_of(&a);
        assert!(err.contains("VBUS") && err.contains("output"), "{err}");
        // Without --list nothing probes, so no warning.
        let b = detect(&["--json"]).assert().success();
        assert!(!stderr_of(&b).contains("VBUS"), "{}", stderr_of(&b));
    }

    #[test]
    fn list_plus_sends_entry_list_plus() {
        let a = detect(&["--list-plus", "--json"]).assert().success();
        let s = sent(&a);
        index_of(&s, "entry list+");
        assert!(!s.contains(&"entry list".to_string()), "{s:?}");
        let v: Value = serde_json::from_str(&stdout_of(&a)).unwrap();
        assert_eq!(v["list"]["complete"], false);
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
    }

    // ---- AC-DGSQ6R.5: never asks for a confirmation ----

    #[test]
    fn detect_needs_no_high_voltage_flag_and_rejects_one() {
        // Runs to completion without --allow-high-voltage ...
        detect(&["--list", "--json"]).assert().success();
        // ... and has no such flag at all.
        detect(&["--allow-high-voltage"]).assert().failure().code(2);
    }

    #[test]
    fn detect_on_the_65w_charger_also_closes_cleanly() {
        let a = detect(&["--list", "--stall-secs", "1"])
            .env("POWERZ_SIM_CHARGER", "pps-65w")
            .assert()
            .success();
        let s = sent(&a);
        assert_eq!(s.first().map(String::as_str), Some("pdm open"), "{s:?}");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
        only_safe_commands(&s);
    }

    // ---- AC-DGSQ6R.7: the sync check after an incomplete list ----

    fn says_power_cycle(text: &str) -> bool {
        let t = text.to_lowercase();
        t.contains("power cycle") || t.contains("power-cycle")
    }

    #[test]
    fn an_incomplete_list_is_followed_by_reset_then_pdm_parm() {
        // AC-DGSQ6R.7: in step → success, the partial list as before.
        let a = detect(&["--list", "--stall-secs", "2"]).assert().success();
        assert!(stdout_of(&a).contains("incomplete"), "{}", stdout_of(&a));
        let s = sent(&a);
        let reset = s
            .iter()
            .rposition(|c| c == "reset")
            .unwrap_or_else(|| panic!("no reset; sent {s:?}"));
        assert_eq!(
            s.get(reset + 1).map(String::as_str),
            Some("pdm parm"),
            "the check comes right after reset: {s:?}"
        );
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
        only_safe_commands(&s);
    }

    #[test]
    fn a_lagging_meter_after_the_list_fails_with_a_power_cycle_message() {
        // AC-DGSQ6R.7: the meter answers one command late after the cut list.
        let a = detect(&["--list", "--stall-secs", "2"])
            .env("POWERZ_SIM_PDM_LAG", "after-list")
            .assert()
            .failure();
        let err = stderr_of(&a);
        assert!(says_power_cycle(&err), "{err}");
        let s = sent(&a);
        let parm = index_of(&s, "pdm parm");
        assert_eq!(s.get(parm - 1).map(String::as_str), Some("reset"), "{s:?}");
        // Nothing but (at most) the closing `pdm close` after the failed check.
        let rest: Vec<&str> = s[parm + 1..].iter().map(String::as_str).collect();
        assert!(
            rest.iter().all(|c| *c == "pdm close"),
            "sent after the failed check: {rest:?}"
        );
    }

    #[test]
    fn a_lagging_meter_fails_in_json_mode_too() {
        // AC-DGSQ6R.7: no JSON result that looks like a normal partial list.
        let a = detect(&["--list", "--stall-secs", "2", "--json"])
            .env("POWERZ_SIM_PDM_LAG", "after-list")
            .assert()
            .failure();
        assert!(says_power_cycle(&stderr_of(&a)), "{}", stderr_of(&a));
    }

    // ---- AC-DGSQ6R.8: the entry checks and the list disagree ----
    //
    // Rule: every output line that says `not-supported` about BC1.2 names its
    // source (`entry bc`), and one line names both checks with what each said:
    // `entry bc: not-supported` and `list: supported`.

    #[test]
    fn text_names_which_check_said_what_when_they_disagree() {
        // AC-DGSQ6R.8, device trace 2026-10-23: `entry bc` → `fail`,
        // list → `bc1.2:true`.
        let a = detect(&["--list", "--stall-secs", "2"])
            .env("POWERZ_SIM_ENTRY_BC", "fail")
            .assert()
            .success();
        let out = stdout_of(&a);
        let both = line_with(&out, &["entry bc: not-supported", "list: supported"])
            .unwrap_or_else(|| panic!("no line naming both checks: {out}"));
        assert!(both.to_lowercase().contains("bc1.2"), "{both}");
        for line in out.lines() {
            if line.to_lowercase().contains("bc1.2") && line.contains("not-supported") {
                assert!(
                    line.contains("entry bc"),
                    "an unqualified not-supported for BC1.2: {line:?}\n{out}"
                );
            }
        }
    }

    #[test]
    fn json_lists_the_disagreement_with_both_sources() {
        // AC-DGSQ6R.8
        let a = detect(&["--list", "--stall-secs", "2", "--json"])
            .env("POWERZ_SIM_ENTRY_BC", "fail")
            .assert()
            .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("one JSON object");
        assert_eq!(v["bc12"], "not-supported", "{v}");
        let d = v["disagreements"].as_array().expect("disagreements array");
        assert_eq!(d.len(), 1, "{v}");
        assert_eq!(d[0]["check"], "entry bc", "{v}");
        assert_eq!(d[0]["check_says"], "not-supported", "{v}");
        assert_eq!(d[0]["entry"], "bc1.2", "{v}");
        assert_eq!(d[0]["list_says"], "supported", "{v}");
    }

    #[test]
    fn json_without_a_disagreement_has_an_empty_list_of_them() {
        // AC-DGSQ6R.8: `entry bc` → `fail` and no list → nothing to compare.
        let a = detect(&["--json"])
            .env("POWERZ_SIM_ENTRY_BC", "fail")
            .assert()
            .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("one JSON object");
        assert_eq!(v["bc12"], "not-supported", "{v}");
        assert_eq!(v["disagreements"], serde_json::json!([]), "{v}");
    }
}
