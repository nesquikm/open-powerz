//! FR QFTCWX, AC-QFTCWX.2: the simulator builds for wasm32-unknown-unknown
//! against the featureless protocol crate.
//!
//! The build shells out to cargo with a separate target directory. It runs
//! when the wasm32-unknown-unknown target is installed and is skipped
//! otherwise (CI's native test steps do not install it), unless
//! `POWERZ_REQUIRE_WASM=1` makes a missing target a failure.

use std::path::{Path, PathBuf};
use std::process::Command;

const WASM: &str = "wasm32-unknown-unknown";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/powerz-sim sits two levels below the workspace")
        .to_path_buf()
}

fn wasm_target_installed() -> bool {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let Ok(out) = Command::new(rustc)
        .args(["--print", "target-libdir", "--target", WASM])
        .output()
    else {
        return false;
    };
    out.status.success() && Path::new(String::from_utf8_lossy(&out.stdout).trim()).is_dir()
}

#[test]
fn sim_depends_on_the_protocol_crate_without_its_native_transports() {
    let manifest = std::fs::read_to_string(workspace_root().join("crates/powerz-sim/Cargo.toml"))
        .expect("read powerz-sim/Cargo.toml");
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("powerz-protocol"))
        .expect("powerz-sim depends on powerz-protocol");
    // Either the member opts out itself, or it inherits a workspace entry
    // that does (cargo ignores a member's opt-out over a default-on entry).
    let featureless = if line.contains("default-features = false") {
        true
    } else {
        let root = std::fs::read_to_string(workspace_root().join("Cargo.toml"))
            .expect("read the workspace Cargo.toml");
        line.contains("workspace = true")
            && !line.contains("\"bulk\"")
            && !line.contains("\"cdc\"")
            && root
                .lines()
                .find(|l| l.trim_start().starts_with("powerz-protocol"))
                .is_some_and(|l| l.contains("default-features = false"))
    };
    assert!(
        featureless,
        "powerz-sim takes the featureless protocol crate: {line}"
    );
}

#[test]
fn sim_builds_for_wasm32() {
    if !wasm_target_installed() {
        assert!(
            std::env::var("POWERZ_REQUIRE_WASM").as_deref() != Ok("1"),
            "POWERZ_REQUIRE_WASM=1 but the {WASM} target is not installed"
        );
        eprintln!("skipped: the {WASM} target is not installed");
        return;
    }
    let out = Command::new(env!("CARGO"))
        .current_dir(workspace_root())
        .args(["build", "-p", "powerz-sim", "--target", WASM, "--locked"])
        .env(
            "CARGO_TARGET_DIR",
            workspace_root().join("target/wasm-check"),
        )
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("run cargo build");
    assert!(
        out.status.success(),
        "powerz-sim for {WASM}:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
