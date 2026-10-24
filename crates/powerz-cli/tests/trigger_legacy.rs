//! `powerz trigger <qc|qc3|fcp|afc|sfcp|scp|vfcp|tfcp|ufcs> ...` end to end
//! against the simulated meter (FR DGSQ6Q). No hardware is touched.
//!
//! Grammar (consistent with `trigger pd`):
//!
//! - `trigger qc|fcp|afc|sfcp request <VOLTS>` (QC 2.0: 5/9/12/20; the
//!   others 5/9/12)
//! - `trigger qc3 request --voltage-mv <MV>`, `trigger qc3 inc <N>`,
//!   `trigger qc3 dec <N>`
//! - `trigger scp|vfcp|tfcp list`, `trigger scp|vfcp|tfcp request
//!   --voltage-mv <MV> --current-ma <MA>`
//! - `trigger ufcs list`, `trigger ufcs request <N> --voltage-mv <MV>
//!   --current-ma <MA>`, `trigger ufcs cmd <N>`
//!
//! Every action that sends a request (and `ufcs cmd`) takes
//! `--allow-high-voltage`, `--hold <SECS>` (default: until Ctrl-C) and
//! `--json`; `list` takes `--json`. After a request the meter goes back to
//! 5 V with `reset`, then `pdm close`.
//!
//! Test-only knobs: `POWERZ_SIM_CLOCK=fake`; `POWERZ_SIM_CHARGER=<name>`:
//! `epr-140w` (default, PD only: every legacy entry times out) or
//! `multi-protocol` (speaks every protocol here, synthetic blocks).
//! `--trace` logs every PDM command written as a stderr line `> <command>`.

use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

const ENV_KNOBS: [&str; 4] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_CHARGER",
];

const PROTOCOLS: [&str; 9] = [
    "qc", "qc3", "fcp", "afc", "sfcp", "scp", "vfcp", "tfcp", "ufcs",
];

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(60));
    c
}

/// `powerz --transport sim --trace trigger <args>` on the fake clock, with
/// the multi-protocol charger.
fn trigger(args: &[&str]) -> Command {
    let mut c = powerz();
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.env("POWERZ_SIM_CHARGER", "multi-protocol");
    c.args(["--transport", "sim", "--trace", "trigger"]);
    c.args(args);
    c
}

/// As [`trigger`], with the session-2 140 W charger (PD only).
fn trigger_on_pd_only(args: &[&str]) -> Command {
    let mut c = trigger(args);
    c.env("POWERZ_SIM_CHARGER", "epr-140w");
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

/// Index of `cmd` in `sent`, panicking with the whole list otherwise.
fn index_of(sent: &[String], cmd: &str) -> usize {
    sent.iter()
        .position(|c| c == cmd)
        .unwrap_or_else(|| panic!("{cmd:?} not sent; sent {sent:?}"))
}

/// The session ends at 5 V and closed: `reset` after `after`, then
/// `pdm close` last.
fn ends_safely(sent: &[String], after: usize) {
    assert_eq!(
        sent.last().map(String::as_str),
        Some("pdm close"),
        "{sent:?}"
    );
    let back = sent
        .iter()
        .rposition(|c| c == "reset")
        .unwrap_or_else(|| panic!("no return to 5 V; sent {sent:?}"));
    assert!(back > after, "{sent:?}");
}

/// No command starting with `prefix` was written; the session closed.
fn nothing_like(sent: &[String], prefix: &str) {
    assert!(!sent.iter().any(|c| c.starts_with(prefix)), "{sent:?}");
    assert_eq!(
        sent.last().map(String::as_str),
        Some("pdm close"),
        "{sent:?}"
    );
}

mod trigger_legacy {
    use super::*;

    // ---- AC-DGSQ6Q.6: one subcommand per protocol, with the flags ----

    #[test]
    fn help_lists_a_subcommand_per_protocol() {
        let a = powerz().args(["trigger", "--help"]).assert().success();
        let out = stdout_of(&a);
        for p in PROTOCOLS {
            assert!(
                out.lines().any(|l| l.split_whitespace().next() == Some(p)),
                "{p} missing from help:\n{out}"
            );
        }
    }

    #[test]
    fn request_actions_take_the_high_voltage_hold_and_json_flags() {
        let cases: [&[&str]; 9] = [
            &["qc", "request"],
            &["qc3", "request"],
            &["qc3", "inc"],
            &["fcp", "request"],
            &["afc", "request"],
            &["sfcp", "request"],
            &["scp", "request"],
            &["vfcp", "request"],
            &["ufcs", "request"],
        ];
        for path in cases {
            let a = powerz()
                .arg("trigger")
                .args(path)
                .arg("--help")
                .assert()
                .success();
            let out = stdout_of(&a);
            for flag in ["--allow-high-voltage", "--hold", "--json"] {
                assert!(
                    out.contains(flag),
                    "{flag} missing from {path:?} help:\n{out}"
                );
            }
        }
        let a = powerz()
            .args(["trigger", "tfcp", "request", "--help"])
            .assert()
            .success();
        let out = stdout_of(&a);
        for flag in ["--voltage-mv", "--current-ma"] {
            assert!(out.contains(flag), "{flag} missing:\n{out}");
        }
    }

    // ---- AC-DGSQ6Q.1: QC 2.0 and QC 3.0 ----

    #[test]
    fn qc_9v_with_the_flag_is_sent_then_back_to_5v() {
        let a = trigger(&["qc", "request", "9", "--allow-high-voltage", "--hold", "0"])
            .assert()
            .success();
        let s = sent(&a);
        let entry = index_of(&s, "entry qc");
        let req = index_of(&s, "qc 9V");
        assert!(entry < req, "{s:?}");
        ends_safely(&s, req);
    }

    // ---- AC-DGSQ6S.13: the CLI enters its protocol before the request, once ----

    #[test]
    fn qc_request_enters_qc_once_before_the_request() {
        let a = trigger(&["qc", "request", "9", "--allow-high-voltage", "--hold", "0"])
            .assert()
            .success();
        let s = sent(&a);
        let req = index_of(&s, "qc 9V");
        let entries: Vec<usize> = s
            .iter()
            .enumerate()
            .filter(|(_, c)| c.starts_with("entry "))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            entries.iter().filter(|&&i| i < req).count(),
            1,
            "exactly one entry before the request: {s:?}"
        );
        let last_entry = *entries.iter().rfind(|&&i| i < req).expect("an entry");
        assert_eq!(s[last_entry], "entry qc", "{s:?}");
        ends_safely(&s, req);
    }

    #[test]
    fn qc_20v_json_names_the_command() {
        let a = trigger(&[
            "qc",
            "request",
            "20",
            "--allow-high-voltage",
            "--hold",
            "0",
            "--json",
        ])
        .assert()
        .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["command"], "qc 20V");
        let s = sent(&a);
        ends_safely(&s, index_of(&s, "qc 20V"));
    }

    #[test]
    fn qc_rejects_a_voltage_it_does_not_have() {
        let a = trigger(&["qc", "request", "15", "--allow-high-voltage", "--hold", "0"])
            .assert()
            .failure();
        let err = stderr_of(&a);
        assert!(err.contains("15"), "{err}");
        nothing_like(&sent(&a), "qc 15V");
    }

    #[test]
    fn qc3_voltage_on_the_200_mv_grid() {
        let a = trigger(&[
            "qc3",
            "request",
            "--voltage-mv",
            "7200",
            "--allow-high-voltage",
            "--hold",
            "0",
        ])
        .assert()
        .success();
        let s = sent(&a);
        index_of(&s, "entry qc");
        ends_safely(&s, index_of(&s, "qc3 volt=7200"));
    }

    #[test]
    fn qc3_voltage_off_the_grid_is_refused_before_writing() {
        let a = trigger(&[
            "qc3",
            "request",
            "--voltage-mv",
            "7300",
            "--allow-high-voltage",
            "--hold",
            "0",
        ])
        .assert()
        .failure();
        let err = stderr_of(&a);
        assert!(err.contains("7300"), "{err}");
        nothing_like(&sent(&a), "qc3 volt=");
    }

    #[test]
    fn qc3_inc_needs_the_flag_and_dec_does_not() {
        let a = trigger(&["qc3", "inc", "2", "--hold", "0"])
            .assert()
            .failure();
        assert!(
            stderr_of(&a).contains("--allow-high-voltage"),
            "{}",
            stderr_of(&a)
        );
        nothing_like(&sent(&a), "qc3 inc");

        let a = trigger(&["qc3", "inc", "2", "--allow-high-voltage", "--hold", "0"])
            .assert()
            .success();
        let s = sent(&a);
        ends_safely(&s, index_of(&s, "qc3 inc=2"));

        let a = trigger(&["qc3", "dec", "1", "--hold", "0"])
            .assert()
            .success();
        let s = sent(&a);
        ends_safely(&s, index_of(&s, "qc3 dec=1"));
    }

    // ---- AC-DGSQ6Q.2: FCP, AFC, SFCP ----

    #[test]
    fn fcp_afc_sfcp_requests() {
        for (p, v) in [("fcp", "9"), ("afc", "12"), ("sfcp", "9")] {
            let a = trigger(&[p, "request", v, "--allow-high-voltage", "--hold", "0"])
                .assert()
                .success();
            let s = sent(&a);
            index_of(&s, &format!("entry {p}"));
            ends_safely(&s, index_of(&s, &format!("{p} {v}V")));
        }
    }

    #[test]
    fn fcp_has_no_20v() {
        let a = trigger(&[
            "fcp",
            "request",
            "20",
            "--allow-high-voltage",
            "--hold",
            "0",
        ])
        .assert()
        .failure();
        assert!(stderr_of(&a).contains("20"), "{}", stderr_of(&a));
        nothing_like(&sent(&a), "fcp 20V");
    }

    // ---- AC-DGSQ6Q.3: SCP / VFCP / TFCP limits and requests ----

    #[test]
    fn scp_list_shows_the_decoded_limits() {
        let a = trigger(&["scp", "list"]).assert().success();
        let out = stdout_of(&a);
        for fragment in ["3.40", "20.00", "0.10", "4.00", "40"] {
            assert!(out.contains(fragment), "{fragment} missing:\n{out}");
        }
        let s = sent(&a);
        index_of(&s, "entry scp");
        assert_eq!(s.last().map(String::as_str), Some("pdm close"), "{s:?}");
    }

    #[test]
    fn list_json_carries_the_limits() {
        let a = trigger(&["tfcp", "list", "--json"]).assert().success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["protocol"], "tfcp");
        let l = &v["limits"][0];
        assert_eq!(l["min_voltage_mv"], 5000);
        assert_eq!(l["max_voltage_mv"], 20_000);
        assert_eq!(l["min_current_ma"], 100);
        assert_eq!(l["max_current_ma"], 3000);
        assert_eq!(l["max_power_mw"], 60_000);

        let a = trigger(&["vfcp", "list", "--json"]).assert().success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["protocol"], "vfcp");
        assert_eq!(v["limits"][0]["max_voltage_mv"], 12_000);
        assert_eq!(v["limits"][0]["max_power_mw"], 18_000);
    }

    #[test]
    fn scp_vfcp_tfcp_requests_inside_the_limits() {
        for (p, mv, ma) in [
            ("scp", "9000", "2000"),
            ("vfcp", "9000", "1000"),
            ("tfcp", "12000", "2000"),
        ] {
            let a = trigger(&[
                p,
                "request",
                "--voltage-mv",
                mv,
                "--current-ma",
                ma,
                "--allow-high-voltage",
                "--hold",
                "0",
                "--json",
            ])
            .assert()
            .success();
            let want = format!("{p} volt={mv},cur={ma}");
            let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
            assert_eq!(v["command"], want.as_str());
            let s = sent(&a);
            ends_safely(&s, index_of(&s, &want));
        }
    }

    #[test]
    fn scp_request_outside_the_limits_fails_before_writing() {
        let a = trigger(&[
            "scp",
            "request",
            "--voltage-mv",
            "21000",
            "--current-ma",
            "1000",
            "--allow-high-voltage",
            "--hold",
            "0",
        ])
        .assert()
        .failure();
        let err = stderr_of(&a);
        assert!(err.contains("21000"), "{err}");
        nothing_like(&sent(&a), "scp volt=");
    }

    // ---- AC-DGSQ6Q.4: UFCS ----

    #[test]
    fn ufcs_list_shows_each_capability_by_number() {
        let a = trigger(&["ufcs", "list", "--json"]).assert().success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON");
        assert_eq!(v["protocol"], "ufcs");
        let limits = v["limits"].as_array().expect("limits array");
        let idx: Vec<u64> = limits
            .iter()
            .map(|l| l["index"].as_u64().unwrap())
            .collect();
        assert_eq!(idx, [1, 2]);
        assert_eq!(limits[0]["max_voltage_mv"], 11_000);
        assert_eq!(limits[1]["max_voltage_mv"], 21_000);
        assert_eq!(limits[1]["max_current_ma"], 3000);
    }

    #[test]
    fn ufcs_request_by_number() {
        let a = trigger(&[
            "ufcs",
            "request",
            "2",
            "--voltage-mv",
            "15000",
            "--current-ma",
            "2000",
            "--allow-high-voltage",
            "--hold",
            "0",
        ])
        .assert()
        .success();
        let s = sent(&a);
        index_of(&s, "entry ufcs");
        ends_safely(&s, index_of(&s, "ufcs req=2,volt=15000,cur=2000"));
    }

    #[test]
    fn ufcs_request_outside_its_capability_fails_before_writing() {
        // 15 V is inside #2 but not #1 (3.4-11 V).
        let a = trigger(&[
            "ufcs",
            "request",
            "1",
            "--voltage-mv",
            "15000",
            "--current-ma",
            "2000",
            "--allow-high-voltage",
            "--hold",
            "0",
        ])
        .assert()
        .failure();
        assert!(stderr_of(&a).contains("15000"), "{}", stderr_of(&a));
        nothing_like(&sent(&a), "ufcs req=");
    }

    #[test]
    fn ufcs_control_command_needs_the_flag() {
        // UFCS control numbers are not proven safe (AC-PR41FT.4).
        let refused = trigger(&["ufcs", "cmd", "6", "--hold", "0"])
            .assert()
            .failure();
        assert!(!sent(&refused).iter().any(|c| c == "ufcs cmd=6"));
        let a = trigger(&["ufcs", "cmd", "6", "--allow-high-voltage", "--hold", "0"])
            .assert()
            .success();
        let s = sent(&a);
        index_of(&s, "entry ufcs");
        ends_safely(&s, index_of(&s, "ufcs cmd=6"));
    }

    // ---- AC-DGSQ6Q.5: unsupported protocols ----

    #[test]
    fn unsupported_protocol_is_reported_and_no_request_is_sent() {
        let cases: [(&[&str], &str, &str); 6] = [
            (&["qc", "request", "9"], "entry qc", "qc 9V"),
            (
                &["qc3", "request", "--voltage-mv", "9000"],
                "entry qc",
                "qc3",
            ),
            (&["fcp", "request", "9"], "entry fcp", "fcp 9V"),
            (
                &[
                    "scp",
                    "request",
                    "--voltage-mv",
                    "9000",
                    "--current-ma",
                    "1000",
                ],
                "entry scp",
                "scp volt",
            ),
            (
                &[
                    "vfcp",
                    "request",
                    "--voltage-mv",
                    "9000",
                    "--current-ma",
                    "1000",
                ],
                "entry vfcp",
                "vfcp volt",
            ),
            (
                &[
                    "ufcs",
                    "request",
                    "1",
                    "--voltage-mv",
                    "9000",
                    "--current-ma",
                    "1000",
                ],
                "entry ufcs",
                "ufcs req",
            ),
        ];
        for (args, entry, request) in cases {
            let mut all: Vec<&str> = args.to_vec();
            all.extend(["--allow-high-voltage", "--hold", "0"]);
            let a = trigger_on_pd_only(&all).assert().failure();
            let err = stderr_of(&a).to_lowercase();
            assert!(err.contains("does not support"), "{args:?}: {err}");
            let s = sent(&a);
            index_of(&s, entry);
            nothing_like(&s, request);
        }
    }

    #[test]
    fn unsupported_list_is_reported_too() {
        for p in ["scp", "tfcp", "ufcs"] {
            let a = trigger_on_pd_only(&[p, "list"]).assert().failure();
            let err = stderr_of(&a).to_lowercase();
            assert!(err.contains("does not support"), "{p}: {err}");
            assert!(err.contains(p), "{p}: {err}");
        }
    }

    // ---- AC-DGSQ6Q.6: above 5 V needs --allow-high-voltage ----

    #[test]
    fn requests_above_5v_without_the_flag_fail_and_write_nothing() {
        let cases: [(&[&str], &str); 6] = [
            (&["qc", "request", "9"], "qc 9V"),
            (&["qc3", "request", "--voltage-mv", "9000"], "qc3 volt"),
            (&["afc", "request", "12"], "afc 12V"),
            (
                &[
                    "scp",
                    "request",
                    "--voltage-mv",
                    "9000",
                    "--current-ma",
                    "1000",
                ],
                "scp volt",
            ),
            (
                &[
                    "tfcp",
                    "request",
                    "--voltage-mv",
                    "9000",
                    "--current-ma",
                    "1000",
                ],
                "tfcp volt",
            ),
            (
                &[
                    "ufcs",
                    "request",
                    "1",
                    "--voltage-mv",
                    "9000",
                    "--current-ma",
                    "1000",
                ],
                "ufcs req",
            ),
        ];
        for (args, request) in cases {
            let mut all: Vec<&str> = args.to_vec();
            all.extend(["--hold", "0"]);
            let a = trigger(&all).assert().failure();
            let err = stderr_of(&a);
            assert!(err.contains("--allow-high-voltage"), "{args:?}: {err}");
            nothing_like(&sent(&a), request);
        }
    }

    #[test]
    fn requests_at_5v_need_no_flag() {
        for (args, cmd) in [
            (&["qc", "request", "5"][..], "qc 5V"),
            (&["sfcp", "request", "5"][..], "sfcp 5V"),
            (
                &[
                    "scp",
                    "request",
                    "--voltage-mv",
                    "5000",
                    "--current-ma",
                    "1000",
                ][..],
                "scp volt=5000,cur=1000",
            ),
        ] {
            let mut all: Vec<&str> = args.to_vec();
            all.extend(["--hold", "0"]);
            let a = trigger(&all).assert().success();
            let s = sent(&a);
            ends_safely(&s, index_of(&s, cmd));
        }
    }
}
