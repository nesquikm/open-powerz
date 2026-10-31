//! `powerz watch`, `powerz pd` and `powerz pd decode FILE` end quietly with exit 0 when their output
//! pipe closes (FR 4PJRVK, AC .2), as `powerz watch | head` closes it.
//!
//! The real binary runs against the demo device on real time, so the reader
//! closes the pipe well before the next line is due. Test-only knob (see
//! `e2e_sim.rs`): `POWERZ_SIM_CTRL_C_AFTER=N` bounds a run that never notices
//! the closed pipe. No hardware is touched.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// More GET_DATA polls than a test lets the binary make.
const POLLS: &str = "100000000";

/// How long the binary gets to end after the pipe closed.
const END_WAIT: Duration = Duration::from_secs(30);

fn powerz(args: &[&str]) -> Child {
    Command::new(assert_cmd::cargo::cargo_bin("powerz"))
        .args(["--transport", "sim", "--scenario", "load-5v-1a"])
        .args(args)
        .env_remove("POWERZ_SIM_CLOCK")
        .env_remove("POWERZ_SIM_FAULT")
        .env("POWERZ_SIM_CTRL_C_AFTER", POLLS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("powerz starts")
}

/// Read one line of `child`'s output, close the pipe and wait for the end:
/// its exit code (`None`: killed by a signal) and what it wrote to stderr.
fn after_the_pipe_closes(mut child: Child) -> (Option<i32>, String) {
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut lines = BufReader::new(stdout);
    let mut line = String::new();
    lines.read_line(&mut line).expect("a first line");
    assert!(!line.is_empty(), "the binary printed nothing");
    drop(lines);

    let deadline = Instant::now() + END_WAIT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("the binary can be waited for") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the binary went on after its output pipe closed");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr is piped")
        .read_to_string(&mut stderr)
        .expect("stderr is read");
    (status.code(), stderr)
}

fn assert_quiet_end((code, stderr): (Option<i32>, String)) {
    assert_eq!(code, Some(0), "exit code; stderr:\n{stderr}");
    assert!(
        !stderr.contains("Broken pipe") && !stderr.contains("write failed"),
        "a closed pipe is not an error:\n{stderr}"
    );
    assert_eq!(stderr, "", "the end is quiet");
}

#[test]
fn watch_ends_quietly_when_its_output_pipe_closes() {
    assert_quiet_end(after_the_pipe_closes(powerz(&["watch"])));
}

#[test]
fn pd_ends_quietly_when_its_output_pipe_closes() {
    assert_quiet_end(after_the_pipe_closes(powerz(&["pd"])));
}

#[test]
fn pd_json_ends_quietly_when_its_output_pipe_closes() {
    assert_quiet_end(after_the_pipe_closes(powerz(&["pd", "--json"])));
}

/// A scratch path under this test target's directory.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("closed-pipe");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn pd_decode_ends_quietly_when_its_output_pipe_closes() {
    // A short real capture, then its records repeated until the output is
    // far bigger than a pipe buffer, so the writer meets the closed pipe.
    let small = scratch("small.pdcap");
    let made = Command::new(assert_cmd::cargo::cargo_bin("powerz"))
        .args(["--transport", "sim", "--scenario", "load-5v-1a"])
        .env("POWERZ_SIM_CLOCK", "fake")
        .env("POWERZ_SIM_CTRL_C_AFTER", "24000")
        .env_remove("POWERZ_SIM_FAULT")
        .args(["pd", "--save"])
        .arg(&small)
        .output()
        .expect("powerz runs");
    assert!(made.status.success());
    let text = std::fs::read_to_string(&small).expect("saved");
    let mut lines = text.lines();
    let header = lines.next().expect("a header line");
    let body: Vec<&str> = lines.filter(|l| l.contains("\"record\"")).collect();
    assert!(!body.is_empty(), "the capture holds records");
    let big = scratch("big.pdcap");
    let mut file = String::from(header);
    file.push('\n');
    for _ in 0..4000 {
        for l in &body {
            file.push_str(l);
            file.push('\n');
        }
    }
    assert!(file.len() > 1 << 20, "smaller than a pipe buffer can hold");
    std::fs::write(&big, file).expect("big file written");

    let child = Command::new(assert_cmd::cargo::cargo_bin("powerz"))
        .args(["pd", "decode"])
        .arg(&big)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("powerz starts");
    assert_quiet_end(after_the_pipe_closes(child));
}

#[test]
fn pd_save_finishes_its_file_when_the_output_pipe_closes() {
    let path = scratch("closed-pipe-save.pdcap");
    let child = powerz(&["pd", "--save", path.to_str().expect("utf-8 path")]);
    assert_quiet_end(after_the_pipe_closes(child));
    let file = std::fs::File::open(&path).expect("the save file exists");
    let loaded = powerz_protocol::pdcap::load(std::io::BufReader::new(file));
    assert!(loaded.header.is_some(), "the file has its header");
    assert!(
        loaded.error.is_none(),
        "the file is whole: {:?}",
        loaded.error
    );
    assert!(
        loaded
            .entries
            .iter()
            .any(|e| matches!(e, powerz_protocol::pdcap::Entry::Record { .. })),
        "the records shown before the pipe closed are in the file"
    );
}
