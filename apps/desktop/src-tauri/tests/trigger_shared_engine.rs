//! AC-W8SHXC.2: the trigger engine is one Rust implementation, the
//! `powerz-trigger` crate, used by the desktop app and by the browser's wasm
//! module. The desktop app's own trigger tests (in `src/trigger.rs` and the
//! `trigger_*` integration tests) are the behaviour proof; this file checks
//! the structure: the crate is a workspace member that builds featureless (no
//! native transports, so it compiles for wasm32), the desktop trigger module
//! re-exports its types instead of defining its own engine, and the wasm
//! binding depends on it.

use std::path::{Path, PathBuf};

use powerz_sim::{SimCharger, SimClock, SimPdm};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The `[dependencies]` table of a Cargo manifest, as text.
fn dependencies(manifest: &str) -> String {
    let start = manifest
        .find("\n[dependencies]")
        .expect("a [dependencies] table");
    let rest = &manifest[start + 1..];
    let end = rest[1..].find("\n[").map_or(rest.len(), |i| i + 1);
    rest[..end].to_owned()
}

#[test]
fn the_engine_crate_is_a_workspace_member_and_a_default_member() {
    let workspace = read("Cargo.toml");
    let members = workspace
        .split("default-members")
        .next()
        .expect("members first");
    assert!(
        members.contains("\"crates/powerz-trigger\""),
        "powerz-trigger is not a workspace member"
    );
    let default_members = workspace
        .lines()
        .find(|l| l.starts_with("default-members"))
        .expect("default-members");
    assert!(
        default_members.contains("\"crates/powerz-trigger\""),
        "powerz-trigger is not a default member"
    );
}

#[test]
fn the_engine_crate_builds_without_native_transports() {
    let manifest = read("crates/powerz-trigger/Cargo.toml");
    let deps = dependencies(&manifest);
    let protocol = deps
        .lines()
        .find(|l| l.trim_start().starts_with("powerz-protocol"))
        .expect("the engine depends on powerz-protocol");
    // The workspace dependency is featureless (default-features = false).
    assert!(protocol.contains("workspace = true"), "{protocol}");
    for native in ["\"bulk\"", "\"hid\"", "\"cdc\"", "default-features = true"] {
        assert!(!protocol.contains(native), "{protocol}");
    }
    for native in ["serialport", "nusb", "hidapi", "tauri"] {
        assert!(
            !deps.lines().any(|l| l.trim_start().starts_with(native)),
            "the engine depends on {native}"
        );
    }
}

#[test]
fn the_wasm_binding_runs_the_same_engine() {
    let manifest = read("crates/powerz-wasm/Cargo.toml");
    let deps = dependencies(&manifest);
    assert!(
        deps.lines()
            .any(|l| l.trim_start().starts_with("powerz-trigger")),
        "powerz-wasm does not depend on powerz-trigger"
    );
}

#[test]
fn the_desktop_trigger_module_reexports_the_engine_and_defines_none() {
    let src = read("apps/desktop/src-tauri/src/trigger.rs");
    assert!(
        src.contains("powerz_trigger"),
        "trigger.rs does not use powerz_trigger"
    );
    for own in [
        "struct Session<",
        "trait Engine",
        "pub enum TriggerRequest",
        "pub enum RiskClass",
        "enum ArmedEnd",
        "fn check_vbus(",
    ] {
        assert!(!src.contains(own), "trigger.rs still defines `{own}`");
    }
}

/// The desktop's types are the engine crate's: a value of one is a value of
/// the other (this fails to compile when they are two copies).
#[test]
fn the_desktop_types_are_the_engine_types() {
    use open_powerz_desktop_lib::trigger as desktop;

    let request: powerz_trigger::TriggerRequest = desktop::TriggerRequest::Pd {
        position: 2,
        voltage_mv: None,
        current_ma: None,
    };
    let _: desktop::TriggerProtocol = powerz_trigger::TriggerProtocol::Pd;
    let _: desktop::Confirm = powerz_trigger::Confirm::YesRemember;
    let _: desktop::RiskClass = powerz_trigger::RiskClass::Armed;
    let reading: desktop::VbusReading = powerz_trigger::VbusReading {
        volts: 9.0,
        cc1_v: 1.5,
        cc2_v: 0.0,
        time_ms: 0,
        period: std::time::Duration::from_millis(20),
    };
    assert_eq!(reading.volts, 9.0);

    // The desktop trigger thread answers what the engine answers directly.
    let clock = SimClock::new(1_790_000_000_000);
    let sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
    let worker = desktop::TriggerWorker::spawn(sim.open_port(), clock).expect("desktop session");
    let c = worker.control();
    let from_desktop = c.list(desktop::TriggerProtocol::Pd).expect("desktop list");
    let desktop_risk = c.risk(request).expect("desktop risk");
    worker.close().expect("close");

    let clock = SimClock::new(1_790_000_000_000);
    let sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
    let mut engine = powerz_trigger::Session::open(sim.open_port(), clock).expect("engine session");
    use powerz_trigger::Engine as _;
    let from_engine = engine
        .list(powerz_trigger::TriggerProtocol::Pd)
        .expect("engine list");
    assert_eq!(from_desktop, from_engine);
    assert_eq!(desktop_risk, engine.risk(request).expect("engine risk"));
    engine.shutdown().expect("close");
}

/// Engine errors keep the desktop's kinds and wording through `UiError`.
#[test]
fn engine_errors_become_the_desktop_ui_errors() {
    use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};

    let clock = SimClock::new(1_790_000_000_000);
    let sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
    let mut engine = powerz_trigger::Session::open(sim.open_port(), clock).expect("engine session");
    use powerz_trigger::Engine as _;
    engine
        .list(powerz_trigger::TriggerProtocol::Pd)
        .expect("list");
    let err = engine
        .request(
            powerz_trigger::TriggerRequest::Pd {
                position: 2,
                voltage_mv: None,
                current_ma: None,
            },
            powerz_trigger::Confirm::No,
        )
        .expect_err("needs a confirmation");
    assert_eq!(
        serde_json::to_value(&err.kind).expect("kind"),
        serde_json::json!("usage")
    );
    let ui: UiError = err.into();
    assert_eq!(ui.kind, ErrorKind::Usage);
    assert_eq!(ui.message, "A request above 5 V needs your confirmation.");
    assert_eq!(ui.hint, None);
    assert!(ui.sessions.is_empty());
    engine.shutdown().expect("close");
}
