//! End-to-end `powerz pd` against the demo device (FR TGE83A, AC .6, .8):
//! the simulator's PD capture scenario streams through the real binary as
//! JSON Lines or text, and the stop command (0x11) is the last frame sent,
//! on Ctrl-C and after a device error. No hardware is touched.
//!
//! Test-only knobs (see `e2e_sim.rs`): `POWERZ_SIM_CLOCK=fake`,
//! `POWERZ_SIM_FAULT=<plan>`, `POWERZ_SIM_CTRL_C_AFTER=N` (N GET_DATA
//! requests, then Ctrl-C).

use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

const ENV_KNOBS: [&str; 3] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
];

/// `powerz --transport sim pd ...` on the simulated clock, Ctrl-C after
/// `polls` GET_DATA requests.
fn sim_pd(polls: usize) -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in ENV_KNOBS {
        c.env_remove(k);
    }
    c.timeout(Duration::from_secs(120));
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.env("POWERZ_SIM_CTRL_C_AFTER", polls.to_string());
    c.args(["--transport", "sim"]);
    c
}

fn stdout_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

fn header(v: &Value) -> Option<u64> {
    v["header"].as_u64()
}

fn is_data(v: &Value, ty: u64) -> bool {
    header(v).is_some_and(|h| h & 0x8000 == 0 && (h >> 12) & 7 != 0 && h & 0x1F == ty)
}

fn is_control(v: &Value, ty: u64) -> bool {
    header(v).is_some_and(|h| h & 0x8000 == 0 && (h >> 12) & 7 == 0 && h & 0x1F == ty)
}

fn is_extended(v: &Value, ty: u64) -> bool {
    header(v).is_some_and(|h| h & 0x8000 != 0 && h & 0x1F == ty)
}

/// The last frame sent (`>` lines of `--trace`) has message type `ty`.
fn last_sent_type(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .rfind(|l| l.starts_with('>'))
        .and_then(|l| l[1..].split_whitespace().next().map(str::to_lowercase))
}

#[test]
fn pd_json_against_the_demo_device_shows_the_scripted_capture() {
    // 24 000 polls at 5 ms = 2 minutes of simulated capture.
    let a = sim_pd(24_000).args(["pd", "--json"]).assert().success();
    let out = stdout_of(&a);
    let recs: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}")))
        .collect();
    assert!(!recs.is_empty(), "the demo device plays a PD capture");

    let elapsed: Vec<u64> = recs
        .iter()
        .map(|r| r["elapsed_ms"].as_u64().unwrap())
        .collect();
    assert!(
        elapsed.windows(2).all(|w| w[0] <= w[1]),
        "monotonic elapsed_ms"
    );

    assert_eq!(recs[0]["kind"], "event");
    assert_eq!(recs[0]["event"], "attach");
    for (what, found) in [
        ("Source_Capabilities", recs.iter().any(|r| is_data(r, 1))),
        ("Request", recs.iter().any(|r| is_data(r, 2))),
        ("Accept", recs.iter().any(|r| is_control(r, 3))),
        ("PS_RDY", recs.iter().any(|r| is_control(r, 6))),
        ("EPR_Mode", recs.iter().any(|r| is_data(r, 10))),
        (
            "EPR_Source_Capabilities",
            recs.iter().any(|r| is_extended(r, 17)),
        ),
        ("EPR_Request", recs.iter().any(|r| is_data(r, 9))),
        (
            "Hard Reset",
            recs.iter()
                .any(|r| r["kind"] == "event" && r["event"] == "hard_reset"),
        ),
    ] {
        assert!(found, "{what} in the demo capture");
    }
}

#[test]
fn pd_text_against_the_demo_device() {
    let a = sim_pd(24_000).arg("pd").assert().success();
    let out = stdout_of(&a);
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(lines.len() > 10, "text lines: {}", lines.len());
    assert!(
        serde_json::from_str::<Value>(lines[0]).is_err(),
        "{}",
        lines[0]
    );
    assert!(lines[0].to_lowercase().contains("attach"), "{}", lines[0]);
    assert!(
        lines
            .iter()
            .any(|l| l.to_lowercase().contains("hard reset")
                || l.to_lowercase().contains("hard_reset")),
        "hard reset line"
    );
}

#[test]
fn ctrl_c_sends_stop_last() {
    let a = sim_pd(40)
        .args(["--trace", "pd", "--json"])
        .assert()
        .success();
    let err = stderr_of(&a);
    assert!(
        err.lines().any(|l| l.starts_with("> 10 ")),
        "0x10 start sent:\n{err}"
    );
    assert_eq!(
        last_sent_type(&err).as_deref(),
        Some("11"),
        "0x11 last:\n{err}"
    );
}

#[test]
fn device_error_sends_stop_last_and_exits_5() {
    // Requests: CONNECT, memory read, auth, start, then polls; the 12th
    // request (a poll) times out.
    let a = sim_pd(1_000)
        .env("POWERZ_SIM_FAULT", "timeout@12")
        .args(["--trace", "pd"])
        .assert()
        .code(5);
    let err = stderr_of(&a);
    assert!(err.contains("timed out"), "{err}");
    assert_eq!(
        last_sent_type(&err).as_deref(),
        Some("11"),
        "0x11 last:\n{err}"
    );
}
