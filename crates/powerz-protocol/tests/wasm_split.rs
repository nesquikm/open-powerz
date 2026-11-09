//! FR QFTCWX: the protocol core builds for the browser.
//!
//! - AC-QFTCWX.1: `bulk` / `hid` / `cdc` features, all default; the
//!   featureless crate builds for wasm32-unknown-unknown.
//! - AC-QFTCWX.3: on wasm32 the auth random comes from getrandom's web
//!   backend (`wasm_js`), configured for that target only.
//! - AC-QFTCWX.4: the CLI and desktop crates keep every native transport.
//! - AC-QFTCWX.5: CI's wasm step adds the target and builds all three crates.
//!
//! The wasm builds shell out to cargo with a separate target directory. They
//! run when the wasm32-unknown-unknown target is installed and are skipped
//! otherwise (CI's native test steps do not install it), unless
//! `POWERZ_REQUIRE_WASM=1` makes a missing target a failure.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const WASM: &str = "wasm32-unknown-unknown";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/powerz-protocol sits two levels below the workspace")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn cargo() -> Command {
    let mut c = Command::new(env!("CARGO"));
    c.current_dir(workspace_root());
    c
}

/// `cargo metadata --no-deps` entry for `name`.
fn package(name: &str) -> Value {
    let out = cargo()
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Value = serde_json::from_slice(&out.stdout).expect("metadata json");
    meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("package {name} in the workspace"))
        .clone()
}

fn deps<'a>(pkg: &'a Value, name: &str) -> Vec<&'a Value> {
    pkg["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .filter(|d| d["name"] == name && d["kind"].is_null())
        .collect()
}

fn feature_list(pkg: &Value, feature: &str) -> Vec<String> {
    pkg["features"][feature]
        .as_array()
        .unwrap_or_else(|| panic!("feature `{feature}` declared"))
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_owned())
        .collect()
}

/// Whether the active toolchain has the wasm32 target's std installed.
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

/// Build `args` for wasm32 in a dedicated target directory; `None` when the
/// target is missing and not required.
fn wasm_build(args: &[&str]) -> Option<(bool, String)> {
    if !wasm_target_installed() {
        assert!(
            std::env::var("POWERZ_REQUIRE_WASM").as_deref() != Ok("1"),
            "POWERZ_REQUIRE_WASM=1 but the {WASM} target is not installed"
        );
        eprintln!("skipped: the {WASM} target is not installed");
        return None;
    }
    let out = cargo()
        .arg("build")
        .args(args)
        .args(["--target", WASM, "--locked"])
        .env(
            "CARGO_TARGET_DIR",
            workspace_root().join("target/wasm-check"),
        )
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("run cargo build");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Some((out.status.success(), stderr))
}

// ---- AC-QFTCWX.1 ----------------------------------------------------------

#[test]
fn transport_features_are_declared_and_all_on_by_default() {
    let p = package("powerz-protocol");
    let default = feature_list(&p, "default");
    for f in ["bulk", "hid", "cdc"] {
        assert!(
            default.iter().any(|d| d == f),
            "`{f}` in default: {default:?}"
        );
    }
    assert!(
        feature_list(&p, "bulk").iter().any(|d| d == "dep:nusb"),
        "`bulk` enables nusb"
    );
    assert!(
        feature_list(&p, "hid").iter().any(|d| d == "dep:hidapi"),
        "`hid` enables hidapi"
    );
    assert!(
        feature_list(&p, "cdc")
            .iter()
            .any(|d| d == "dep:serialport"),
        "`cdc` enables serialport"
    );
    for native in ["nusb", "hidapi", "serialport"] {
        let d = deps(&p, native);
        assert!(!d.is_empty(), "{native} is a dependency");
        assert!(
            d.iter().all(|d| d["optional"] == true),
            "{native} is optional"
        );
    }
}

#[test]
fn featureless_protocol_crate_builds_for_wasm32() {
    let Some((ok, stderr)) = wasm_build(&["-p", "powerz-protocol", "--no-default-features"]) else {
        return;
    };
    assert!(ok, "featureless powerz-protocol for {WASM}:\n{stderr}");
}

/// The attribute line right above `decl` in `src` names a cargo feature.
fn gated(src: &str, decl: &str) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let at = lines.iter().position(|l| l.trim() == decl)?;
    let above = lines.get(at.checked_sub(1)?)?.trim();
    (above.starts_with("#[cfg(") && above.contains("feature")).then(|| above.to_owned())
}

#[test]
fn native_transport_modules_are_gated_on_their_features() {
    let transport = read("crates/powerz-protocol/src/transport/mod.rs");
    for decl in ["pub mod bulk;", "pub mod cdc;", "pub mod discovery;"] {
        assert!(
            gated(&transport, decl).is_some(),
            "transport `{decl}` sits under a #[cfg(feature …)] attribute"
        );
    }
    assert!(
        gated(&transport, "pub mod bulk;").is_some_and(|a| a.contains("\"bulk\"")),
        "transport::bulk is gated on `bulk`"
    );
    assert!(
        gated(&transport, "pub mod cdc;").is_some_and(|a| a.contains("\"cdc\"")),
        "transport::cdc is gated on `cdc`"
    );
    let pdm = read("crates/powerz-protocol/src/pdm/mod.rs");
    assert!(
        gated(&pdm, "pub mod port;").is_some_and(|a| a.contains("\"cdc\"")),
        "pdm::port is gated on `cdc`"
    );
}

// ---- AC-QFTCWX.3 ----------------------------------------------------------

#[test]
fn getrandom_uses_the_web_backend_on_wasm32_only() {
    let p = package("powerz-protocol");
    let g = deps(&p, "getrandom");
    let wasm: Vec<_> = g
        .iter()
        .filter(|d| d["target"].as_str().is_some_and(|t| t.contains("wasm32")))
        .collect();
    assert!(
        !wasm.is_empty(),
        "getrandom has a wasm32 target dependency: {g:?}"
    );
    assert!(
        wasm.iter().any(|d| d["features"]
            .as_array()
            .is_some_and(|f| f.iter().any(|f| f == "wasm_js"))),
        "the wasm32 getrandom enables `wasm_js`: {wasm:?}"
    );
    assert!(
        g.iter()
            .filter(|d| d["target"].is_null())
            .all(|d| !d["features"]
                .as_array()
                .is_some_and(|f| f.iter().any(|f| f == "wasm_js"))),
        "native getrandom stays on the OS backend"
    );
    // Any rustflags (the getrandom_backend cfg) apply to the wasm32 target only.
    if let Ok(cfg) = std::fs::read_to_string(workspace_root().join(".cargo/config.toml")) {
        let build = cfg.find("[build]");
        assert!(
            build.is_none_or(|i| !cfg[i..]
                .split("\n[")
                .next()
                .unwrap_or("")
                .contains("rustflags")),
            "no workspace-wide rustflags in .cargo/config.toml"
        );
        if cfg.contains("getrandom_backend") {
            let sect = cfg
                .find("[target.wasm32-unknown-unknown]")
                .expect("the getrandom_backend cfg lives under [target.wasm32-unknown-unknown]");
            assert!(cfg[sect..].contains("wasm_js"));
        }
    }
}

// ---- AC-QFTCWX.4 ----------------------------------------------------------

/// powerz-protocol features resolved for `pkg`'s native build.
fn resolved_protocol_features(pkg: &str) -> String {
    let out = cargo()
        .args([
            "tree",
            "-p",
            pkg,
            "-e",
            "features,normal",
            "--prefix",
            "none",
            "--locked",
        ])
        .output()
        .expect("run cargo tree");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("powerz-protocol feature"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn cli_and_desktop_keep_every_native_transport() {
    for pkg in ["powerz-cli", "open-powerz-desktop"] {
        let f = resolved_protocol_features(pkg);
        for feature in ["bulk", "hid", "cdc", "image-files"] {
            assert!(
                f.contains(&format!("powerz-protocol feature \"{feature}\"")),
                "{pkg} builds powerz-protocol with `{feature}`:\n{f}"
            );
        }
    }
}

// ---- AC-QFTCWX.5 ----------------------------------------------------------

#[test]
fn ci_wasm_step_builds_pd_protocol_and_sim_for_wasm32() {
    let ci = read(".github/workflows/ci.yml");
    assert_eq!(
        ci.matches("rustup target add wasm32-unknown-unknown")
            .count(),
        1,
        "the wasm32 target is added in one step only"
    );
    let at = ci
        .find("rustup target add wasm32-unknown-unknown")
        .expect("wasm step");
    // The step's run block: up to the next step or job.
    let step = ci[at..].split("\n      - ").next().unwrap_or_default();
    let builds: Vec<&str> = step
        .lines()
        .filter(|l| l.contains("cargo build") && l.contains("--target wasm32-unknown-unknown"))
        .collect();
    let has = |pkg: &str, extra: &str| {
        builds.iter().any(|l| {
            l.contains(&format!("-p {pkg}")) && l.contains(extra) && l.contains("--locked")
        })
    };
    assert!(has("powerz-pd", ""), "builds powerz-pd: {builds:?}");
    assert!(
        has("powerz-protocol", "--no-default-features"),
        "builds featureless powerz-protocol: {builds:?}"
    );
    assert!(has("powerz-sim", ""), "builds powerz-sim: {builds:?}");

    // Native commands stay as they are.
    assert!(ci.contains("cargo test --workspace --locked"));
    assert!(
        !ci.lines()
            .filter(|l| l.contains("cargo test") || l.contains("cargo clippy"))
            .any(|l| l.contains("wasm32")),
        "no native test/lint step targets wasm32"
    );
    let toolchain = read("rust-toolchain.toml");
    assert!(
        !toolchain.contains("wasm32"),
        "rust-toolchain.toml does not add the wasm32 target"
    );
}
