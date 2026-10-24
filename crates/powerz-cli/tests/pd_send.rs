//! `powerz pd send` end to end against the simulated meter (FR DGSQ6T,
//! AC .1, .2, .3, .5, .6). No hardware is touched.
//!
//! `pd send <HEX>` takes a PD message as hex (PD header LE, then data
//! objects LE; no SOP byte) and `--sop sop|sop1|sop2` picks SOP, SOP' or
//! SOP'' (default SOP); `--preset discover-identity-cable` sends Discover
//! Identity to the cable (SOP'). A control message on SOP goes out as
//! `pd cmd=<type>`, everything else as `pd data=<SOP byte><hex>`.
//!
//! Test-only knob: `POWERZ_SIM_CABLE=epr-240w` puts the session-2 e-marked
//! 240 W cable on the simulated meter (no cable when unset).
//!
//! `--trace` logs every PDM command written as a stderr line `> <command>`.

use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

const ENV_KNOBS: [&str; 5] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_CHARGER",
    "POWERZ_SIM_CABLE",
];

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(60));
    c
}

/// `powerz --transport sim --trace pd send <args>` on the fake clock.
fn send(args: &[&str]) -> Command {
    let mut c = powerz();
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.args(["--transport", "sim", "--trace", "pd", "send"]);
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

fn closes_last(sent: &[String]) {
    assert_eq!(
        sent.last().map(String::as_str),
        Some("pdm close"),
        "{sent:?}"
    );
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

mod pd_send {
    use super::*;

    // ---- AC-DGSQ6T.6: the subcommand and its flags ----

    #[test]
    fn help_lists_pd_send_and_its_flags() {
        let a = powerz().args(["pd", "--help"]).assert().success();
        assert!(stdout_of(&a).contains("send"), "{}", stdout_of(&a));
        let a = powerz().args(["pd", "send", "--help"]).assert().success();
        let out = stdout_of(&a);
        for flag in [
            "--preset",
            "--sop",
            "--allow-high-voltage",
            "--json",
            "discover-identity-cable",
        ] {
            assert!(out.contains(flag), "{flag} missing from help:\n{out}");
        }
    }

    // ---- AC-DGSQ6T.1: control and data messages for SOP, SOP', SOP'' ----

    #[test]
    fn a_control_message_on_sop_goes_out_as_pd_cmd() {
        let a = send(&["8D00"]).assert().success();
        let s = sent(&a);
        assert!(s.contains(&"pd cmd=13".to_string()), "{s:?}");
        closes_last(&s);
    }

    #[test]
    fn messages_to_the_cable_go_out_as_pd_data_with_the_sop_byte() {
        for (sop, wire) in [("sop1", "pd data=018D00"), ("sop2", "pd data=028D00")] {
            let a = send(&["8D00", "--sop", sop]).assert().success();
            let s = sent(&a);
            assert!(s.contains(&wire.to_string()), "{sop}: {s:?}");
            closes_last(&s);
        }
    }

    #[test]
    fn a_data_message_goes_out_as_pd_data() {
        // Request, fixed 9 V, position 2; above 5 V, so confirmed by the flag.
        let a = send(&["82102CB10423", "--allow-high-voltage"])
            .assert()
            .success();
        let s = sent(&a);
        let at = s
            .iter()
            .position(|c| c == "pd data=0082102CB10423")
            .unwrap_or_else(|| panic!("request not sent: {s:?}"));
        // Back to 5 V before the session closes.
        assert!(
            s[at + 1..]
                .iter()
                .any(|c| c == "pd req=1" || c == "pd hrst" || c == "reset"),
            "no return to 5 V after the request: {s:?}"
        );
        closes_last(&s);
    }

    #[test]
    fn malformed_hex_is_a_usage_error_and_sends_no_message() {
        for bad in ["8D0", "zz00", "8210"] {
            let a = send(&[bad]).assert().failure().code(2);
            let s = sent(&a);
            assert!(
                !s.iter()
                    .any(|c| c.starts_with("pd data=") || c.starts_with("pd cmd=")),
                "{bad}: {s:?}"
            );
        }
    }

    // ---- AC-DGSQ6T.2: the Discover Identity preset and the cable's details ----

    #[test]
    fn the_cable_preset_prints_the_emarker_details() {
        let a = send(&[
            "--preset",
            "discover-identity-cable",
            "--allow-high-voltage",
        ])
        .env("POWERZ_SIM_CABLE", "epr-240w")
        .assert()
        .success();
        let s = sent(&a);
        let data = s
            .iter()
            .find_map(|c| c.strip_prefix("pd data="))
            .unwrap_or_else(|| panic!("no pd data sent: {s:?}"));
        let b = hex_bytes(data);
        assert_eq!(b[0], 1, "SOP'");
        assert_eq!(
            u16::from_le_bytes([b[1], b[2]]) & 0x1F,
            15,
            "Vendor_Defined"
        );
        let out = stdout_of(&a);
        for want in ["0x050D", "0x8254", "50 V", "5 A", "EPR"] {
            assert!(out.contains(want), "{want} missing:\n{out}");
        }
        closes_last(&s);
    }

    #[test]
    fn the_cable_preset_as_json() {
        let a = send(&[
            "--preset",
            "discover-identity-cable",
            "--allow-high-voltage",
            "--json",
        ])
        .env("POWERZ_SIM_CABLE", "epr-240w")
        .assert()
        .success();
        let v: Value = serde_json::from_str(&stdout_of(&a)).expect("JSON on stdout");
        assert!(
            v["command"]
                .as_str()
                .is_some_and(|c| c.starts_with("pd data=01")),
            "{v}"
        );
        assert_eq!(v["cable"]["vid"], 0x050D, "{v}");
        assert_eq!(v["cable"]["pid"], 0x8254, "{v}");
        assert_eq!(v["cable"]["max_voltage_v"], 50, "{v}");
        assert_eq!(v["cable"]["max_current_ma"], 5000, "{v}");
        assert_eq!(v["cable"]["epr_capable"], true, "{v}");
    }

    #[test]
    fn no_answer_from_the_cable_is_reported() {
        let a = send(&[
            "--preset",
            "discover-identity-cable",
            "--allow-high-voltage",
        ])
        .assert()
        .failure();
        let err = stderr_of(&a).to_lowercase();
        assert!(err.contains("cable"), "{err}");
        closes_last(&sent(&a));
    }

    #[test]
    fn a_preset_and_hex_together_are_a_usage_error() {
        powerz()
            .args(["--transport", "sim", "pd", "send", "8D00"])
            .args(["--preset", "discover-identity-cable"])
            .assert()
            .failure()
            .code(2);
    }

    #[test]
    fn a_preset_with_another_sop_is_a_usage_error_and_sends_no_message() {
        for sop in ["sop1", "sop2"] {
            let a = send(&[
                "--preset",
                "discover-identity-cable",
                "--sop",
                sop,
                "--allow-high-voltage",
            ])
            .env("POWERZ_SIM_CABLE", "epr-240w")
            .assert()
            .failure()
            .code(2);
            assert!(stderr_of(&a).contains("--sop"), "{}", stderr_of(&a));
            let s = sent(&a);
            assert!(
                !s.iter()
                    .any(|c| c.starts_with("pd data=") || c.starts_with("pd cmd=")),
                "{sop}: {s:?}"
            );
        }
    }

    #[test]
    fn hex_with_a_sign_is_a_usage_error() {
        let a = send(&["+D00"]).assert().failure().code(2);
        let s = sent(&a);
        assert!(!s.iter().any(|c| c.starts_with("pd ")), "{s:?}");
    }

    // ---- AC-DGSQ6T.3: GoodCRC is refused with a reason ----

    #[test]
    fn goodcrc_is_refused_with_a_reason() {
        for args in [
            vec!["4100"],
            vec!["4100", "--sop", "sop1", "--allow-high-voltage"],
        ] {
            let a = send(&args).assert().failure();
            assert!(stderr_of(&a).contains("GoodCRC"), "{}", stderr_of(&a));
            let s = sent(&a);
            assert!(
                !s.iter()
                    .any(|c| c == "pd cmd=1" || c.starts_with("pd data=")),
                "{s:?}"
            );
        }
    }

    // ---- AC-DGSQ6T.5: above 5 V needs --allow-high-voltage ----

    #[test]
    fn a_request_above_5v_without_the_flag_fails_and_sends_nothing() {
        let a = send(&["82102CB10423"]).assert().failure();
        let err = stderr_of(&a);
        assert!(err.contains("--allow-high-voltage"), "{err}");
        let s = sent(&a);
        assert!(!s.iter().any(|c| c.starts_with("pd data=")), "{s:?}");
        closes_last(&s);
    }
}
