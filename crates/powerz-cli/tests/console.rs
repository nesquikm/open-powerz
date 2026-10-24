//! `powerz console` end to end against the simulated meter (FR DGSQ6T,
//! AC .3, .4, .5, .6). No hardware is touched.
//!
//! The console reads command lines from stdin, sends each through the PDM
//! session (guard, pacing, GoodCRC refusal and risk gate unchanged) and
//! prints the transcript on stdout, one line per item:
//! `HH:MM:SS.mmm > <sent line>` and `HH:MM:SS.mmm < <reply line>`; a binary
//! block is printed parsed (its tag and decoded contents). A line refused
//! (GoodCRC, or above 5 V without `--allow-high-voltage`) is reported on
//! stderr and not sent; the session still ends back at 5 V with `pdm close`.
//!
//! `--trace` logs every PDM command written as a stderr line `> <command>`.

use std::time::Duration;

use assert_cmd::Command;

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

/// `powerz --transport sim --trace console <args>` on the fake clock, with
/// `input` on stdin.
fn console(args: &[&str], input: &str) -> Command {
    let mut c = powerz();
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.args(["--transport", "sim", "--trace", "console"]);
    c.args(args);
    c.write_stdin(input.to_string());
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

/// A transcript line: `HH:MM:SS.mmm <dir> <text>`; returns (dir, text).
fn transcript_line(line: &str) -> Option<(char, &str)> {
    let b = line.as_bytes();
    let stamp_ok = b.len() > 15
        && b[2] == b':'
        && b[5] == b':'
        && b[8] == b'.'
        && [0, 1, 3, 4, 6, 7, 9, 10, 11]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
        && b[12] == b' '
        && b[14] == b' ';
    if !stamp_ok {
        return None;
    }
    let dir = b[13] as char;
    matches!(dir, '>' | '<').then(|| (dir, &line[15..]))
}

fn transcript(out: &str) -> Vec<(char, String)> {
    out.lines()
        .filter_map(transcript_line)
        .map(|(d, t)| (d, t.to_string()))
        .collect()
}

fn closes_last(sent: &[String]) {
    assert_eq!(
        sent.last().map(String::as_str),
        Some("pdm close"),
        "{sent:?}"
    );
}

mod console {
    use super::*;

    // ---- AC-DGSQ6T.6: the subcommand and its flag ----

    #[test]
    fn help_lists_the_console_and_its_flag() {
        let a = powerz().arg("--help").assert().success();
        assert!(stdout_of(&a).contains("console"), "{}", stdout_of(&a));
        let a = powerz().args(["console", "--help"]).assert().success();
        assert!(
            stdout_of(&a).contains("--allow-high-voltage"),
            "{}",
            stdout_of(&a)
        );
    }

    // ---- AC-DGSQ6T.4: sent and received lines with times ----

    #[test]
    fn the_console_prints_sent_and_received_lines_with_times() {
        let a = console(&[], "pdm parm\n").assert().success();
        let t = transcript(&stdout_of(&a));
        assert!(
            t.contains(&('>', "pdm parm".to_string())),
            "{t:?}\n{}",
            stdout_of(&a)
        );
        let tx = t.iter().position(|e| e == &('>', "pdm parm".to_string()));
        let rx = t
            .iter()
            .position(|e| e == &('<', "type=0,em=2,sink=0".to_string()));
        assert!(matches!((tx, rx), (Some(x), Some(r)) if r > x), "{t:?}");
        let s = sent(&a);
        assert!(s.contains(&"pdm parm".to_string()), "{s:?}");
        closes_last(&s);
    }

    #[test]
    fn the_console_prints_parsed_blocks() {
        let a = console(&[], "entry pd\n").assert().success();
        let t = transcript(&stdout_of(&a));
        assert!(t.contains(&('<', "ok".to_string())), "{t:?}");
        // The pdo: block is shown parsed: its tag and the 9 V offer.
        assert!(
            t.iter()
                .any(|(d, l)| *d == '<' && l.contains("pdo") && l.contains("9.00 V")),
            "{t:?}\n{}",
            stdout_of(&a)
        );
    }

    #[test]
    fn several_lines_are_sent_in_order_and_blank_lines_skipped() {
        let a = console(&[], "pdm parm\n\nentry pd\n").assert().success();
        let s = sent(&a);
        let parm = s.iter().position(|c| c == "pdm parm");
        let entry = s.iter().position(|c| c == "entry pd");
        assert!(
            matches!((parm, entry), (Some(p), Some(e)) if p < e),
            "{s:?}"
        );
        assert!(!s.iter().any(String::is_empty), "{s:?}");
        closes_last(&s);
    }

    // ---- AC-DGSQ6T.3: GoodCRC is refused in the console ----

    #[test]
    fn goodcrc_is_refused_with_a_reason() {
        let a = console(&["--allow-high-voltage"], "pd cmd=1\npd data=014100\n").assert();
        assert!(stderr_of(&a).contains("GoodCRC"), "{}", stderr_of(&a));
        let s = sent(&a);
        assert!(
            !s.iter().any(|c| c == "pd cmd=1" || c == "pd data=014100"),
            "{s:?}"
        );
        closes_last(&s);
    }

    #[test]
    fn goodcrc_in_any_raw_spelling_is_refused_and_the_console_goes_on() {
        let lines = [
            "pd cmd=01",
            "pd cmd=+1",
            "pd cmd=0x1",
            "pd  cmd=1",
            "pd data=014100",
        ];
        let input = format!("{}\npdm parm\n", lines.join("\n"));
        let a = console(&["--allow-high-voltage"], &input)
            .assert()
            .success();
        let err = stderr_of(&a);
        assert_eq!(err.matches("GoodCRC").count(), lines.len(), "{err}");
        let s = sent(&a);
        for line in lines {
            assert!(!s.iter().any(|c| c == line.trim()), "{line}: {s:?}");
        }
        assert!(s.contains(&"pdm parm".to_string()), "{s:?}");
        closes_last(&s);
    }

    // ---- Review: guard refusals and session control are refusals too ----

    #[test]
    fn a_line_the_frame_guard_refuses_is_reported_and_the_console_goes_on() {
        // `H` (0x48) maps to a forbidden binary frame type.
        let a = console(&["--allow-high-voltage"], "Help\npdm parm\n")
            .assert()
            .success();
        let err = stderr_of(&a);
        assert!(err.contains("refused"), "{err}");
        let s = sent(&a);
        assert!(!s.contains(&"Help".to_string()), "{s:?}");
        assert!(s.contains(&"pdm parm".to_string()), "{s:?}");
        closes_last(&s);
    }

    #[test]
    fn pdm_close_and_pdm_open_typed_in_the_console_are_refused() {
        let a = console(&["--allow-high-voltage"], "pdm close\npdm open\npdm parm\n")
            .assert()
            .success();
        let err = stderr_of(&a);
        assert!(err.contains("pdm close"), "{err}");
        let s = sent(&a);
        // `pdm close` once, at the end; `pdm open` once, at the start.
        assert_eq!(s.iter().filter(|c| *c == "pdm close").count(), 1, "{s:?}");
        assert_eq!(s.iter().filter(|c| *c == "pdm open").count(), 1, "{s:?}");
        assert!(s.contains(&"pdm parm".to_string()), "{s:?}");
        closes_last(&s);
    }

    #[test]
    fn a_confirmed_raw_line_after_entry_pd_is_undone_through_pd() {
        let a = console(&["--allow-high-voltage"], "entry pd\nhello meter\n")
            .assert()
            .success();
        let s = sent(&a);
        let raw = s
            .iter()
            .position(|c| c == "hello meter")
            .unwrap_or_else(|| panic!("{s:?}"));
        let back = s
            .iter()
            .rposition(|c| c == "pd req=1" || c == "pd hrst")
            .unwrap_or_else(|| panic!("no return to 5 V: {s:?}"));
        assert!(back > raw, "{s:?}");
        closes_last(&s);
    }

    // ---- AC-DGSQ6T.5: above 5 V and unparsable lines need the flag ----

    #[test]
    fn an_unparsable_line_without_the_flag_is_refused_and_not_sent() {
        let a = console(&[], "hello meter\npdm parm\n").assert();
        assert!(
            stderr_of(&a).contains("--allow-high-voltage"),
            "{}",
            stderr_of(&a)
        );
        let s = sent(&a);
        assert!(!s.contains(&"hello meter".to_string()), "{s:?}");
        // The console goes on with the next line.
        assert!(s.contains(&"pdm parm".to_string()), "{s:?}");
        closes_last(&s);
    }

    #[test]
    fn a_request_above_5v_without_the_flag_is_refused() {
        let a = console(&[], "entry pd\npd req=2\n").assert();
        assert!(
            stderr_of(&a).contains("--allow-high-voltage"),
            "{}",
            stderr_of(&a)
        );
        let s = sent(&a);
        assert!(!s.contains(&"pd req=2".to_string()), "{s:?}");
        closes_last(&s);
    }

    #[test]
    fn with_the_flag_a_request_above_5v_is_sent_and_undone_before_closing() {
        let a = console(&["--allow-high-voltage"], "entry pd\npd req=2\n")
            .assert()
            .success();
        let s = sent(&a);
        let req = s
            .iter()
            .position(|c| c == "pd req=2")
            .unwrap_or_else(|| panic!("pd req=2 not sent: {s:?}"));
        let back = s
            .iter()
            .rposition(|c| c == "pd req=1" || c == "pd hrst")
            .unwrap_or_else(|| panic!("no return to 5 V: {s:?}"));
        assert!(back > req, "{s:?}");
        closes_last(&s);
        let t = transcript(&stdout_of(&a));
        assert!(
            t.iter().any(|(d, l)| *d == '<' && l.starts_with("ready:")),
            "{t:?}"
        );
    }

    #[test]
    fn with_the_flag_any_text_is_sent() {
        let a = console(&["--allow-high-voltage"], "hello meter\n")
            .assert()
            .success();
        let s = sent(&a);
        assert!(s.contains(&"hello meter".to_string()), "{s:?}");
        let t = transcript(&stdout_of(&a));
        assert!(t.contains(&('>', "hello meter".to_string())), "{t:?}");
    }
}
