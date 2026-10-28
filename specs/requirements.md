<!--
  scope: cross-cutting only

  `specs/requirements.md` § 2 captures cross-cutting functional
  requirements (auth scheme, observability surface, accessibility posture,
  etc.). Per-FR detail lives in `specs/frs/<id>.md` exclusively. Do NOT add
  `### FR-N: [Feature Name]` heading blocks here — `/gate-check`
  `requirements-md-no-placeholder` flags them as drift.

  Tracker-ID placeholder convention: when drafting cross-cutting AC IDs in
  tracker mode, use the literal placeholder `<tracker-id>` until the
  tracker allocator assigns the real ID. Never guess the next sequential
  tracker number — the allocator decides, not the implementer. Substitute
  the real ID globally only after Provider.sync / importFromTracker
  returns it.

  AC IDs use `AC-<tracker-id>.<N>` in tracker mode (substituted by
  /spec-write via acPrefix(spec); same placeholder convention as
  filenames) or `AC-<short-ULID>.<N>` in mode: none (last 6 chars of the
  ULID random portion). Literal `AC-1.1` / `AC-2.1` / bare-digit-numbered
  shapes are rejected by /gate-check `guessed_tracker_id` and by
  /spec-write's post-write scan.

  See the plugin's documentation for the full rule and worked example. In
  mode: none, use the short-ULID tail instead; the placeholder convention
  does not apply.

  Example placeholder FR heading (lives in `specs/frs/<id>.md`, NOT here):
    ### <tracker-id>: <Title> {#<tracker-id>}

  Example traceability-matrix row (added by /spec-write or /implement
  once an AC is wired up; the rendered §6 table below is intentionally
  empty at /setup time):
    | AC-<tracker-id>.1 |          |       |
-->

# Requirements

## 1. Overview

open-powerz is an unofficial, open-source (MIT), cross-platform desktop app and CLI for the
ChargerLAB POWER-Z KM003C / KM002C USB-C power meter and PD analyzer. It replaces the
vendor's Windows-only "POWER-Z Lab" PC software (V2.5.2, `Mtools.exe`) for people on macOS,
Linux and Windows: hardware hobbyists, charger and cable reviewers, and firmware engineers
who need live USB-C power readings, high-rate recordings, and USB PD protocol capture.

It matches the vendor app's *features*, not its look. The protocol library is a reusable
Rust crate, so other tools can talk to the meter too.

Feature scope (from the research notes): **MVP** = discovery + live meter + recorder
(ADC queue up to 1000 SPS) + live chart with statistics + vendor-compatible CSV + CLI.
**Shipped after the MVP** = PD sniffer and analyzer, read-only import of vendor
`.db`/`.sqlite` PD logs, protocol trigger panel (CDC), firmware update, release pipeline,
offline recordings and the settings block (read-only), the ADCNEW serial stream and
multi-device. **Later** = record delete and settings writes (M6b), UFCS analyzer, vendor
`.db`/`.sqlite` export, polish (M7), then a browser edition. Each lands as its own FR in
`specs/frs/`.

Roadmap (one plan per milestone in `specs/plan/`; done ones move to `specs/plan/archive/`):
M_34BAXK Protocol Core and CLI (done) → M_R4WG4Q Desktop Meter (done) → M_NE3C7K PD Analyzer
(done) → M_X4VEQ3 Protocol Triggers (done) → M_MJ7KE9 Firmware Update (done) → M_WBBQ7F
Release Pipeline (done) → M_FW682R Release Publish Fix (done) → M_1HHSZZ Meter Data, read
side (done, v0.7.0) → M6b meter data writes → M7 polish → browser edition. Releases are
unsigned for now.

## 2. Functional Requirements (cross-cutting only)

- **Platforms:** every user-facing feature works on macOS (Apple silicon + Intel), Linux
  (x86_64; udev rule documented) and Windows 10/11 with no vendor driver installed.
- **Device identification:** devices are matched by USB VID `0x5FC9`; interfaces are chosen by
  class (vendor bulk 0xFF, HID 0x03, CDC 0x02/0x0A), never by interface number.
- **Transport fallback:** vendor bulk is the primary pipe; CDC (binary protocol on the virtual
  serial port) is the second pipe; HID is only a read-only fallback for the ADC snapshot, because any HID reply
  longer than 64 bytes hangs and reboots the meter (device session 1, FW 2.0.6).
- **Authentication:** authenticated features (ADC queue streaming, memory read / device
  info, offline records) work out of the box with the AES keys shipped in the repository.
- **Device safety:** the app never sends firmware-update, DFU jump, flash-write, calibration
  or factory-reset commands. Commands that can put up to 48 V on VBUS (protocol triggering)
  require an explicit user confirmation with a warning.
- **Units and signs:** all values shown and exported in SI units with documented precision;
  IBUS is signed (current direction is shown, not hidden).
- **Disclaimer:** the README and the app's About screen state that the project is unofficial
  and not affiliated with ChargerLAB.

## 3. Non-Functional Requirements

### NFR-1: Performance
- Sustained 1000 SPS ADC-queue streaming for ≥ 1 hour with no host-side sample loss
  (device-side gaps are marked, not filled).
- Live chart stays interactive (≥ 30 fps pan/zoom) with 10 million points per session via
  min/max decimation.
- Device data reaches the UI in batched IPC frames (20–60 Hz), never one message per sample.

### NFR-2: Security / clean room
- No vendor code, binaries, firmware, documents or assets in the repository. Protocol code
  is written from our own notes and public docs. The AES keys are the sole, deliberate
  exception.
- Decoders never panic on malformed or truncated device data (fuzz/property tests on the
  frame parsers).
- Dependency audit (`cargo deny`/`cargo audit`, `bun audit`) clean before each release.

### NFR-3: Reliability
- Unplugging or re-plugging the meter mid-session never crashes the app; the session ends
  cleanly, recorded data is kept, and the device is re-detected on hotplug.
- One request in flight per device; every request has a timeout (≈ 1–2 s).

## 4. Edge Cases

- Firmware variants: ADC section length 40 or 44 bytes (44 on FW 2.0.6); parse by section size.
- A stale DISCONNECT frame is often queued on the bulk IN endpoint at open: drain it.
- Auth level is lost on every close/reopen: authenticate again after each open.
- Memory read (and so device info and auth) is REJECTed on bulk until CONNECT has been
  sent (FW 2.0.6, found by the first real-device CLI run); the CLI sends CONNECT on bulk.
- At 1000 SPS a reply holds at most 63 queue records: poll at least every ~30–60 ms.
- On CDC, frame types 0x01/0x02/0x03 start or stop the ADCNEW push stream, so CONNECT must
  never be sent there.
- Linux `powerz` hwmon kernel driver bound to the bulk interface: detach it, or fall back to CDC (or HID for ADC snapshots only).
- Open PDM serial session holding the PD engine: PD capture over bulk may be rejected until
  `pdm close`.

## Security / Abuse Cases

| Attacker Goal | Attack Vector | Mitigation |
|--------------|---------------|------------|
| Crash or hang the host app | Malformed or malicious USB frames from a spoofed device | Length-checked parsers, no `unwrap` on device data, fuzz tests, per-request timeouts |
| Damage the connected device under test | Triggering high-voltage PD/QC profiles by accident | Explicit confirmation + 48 V warning; no trigger commands from the CLI without a flag |
| Brick the meter | DFU / flash / calibration commands | Not implemented (out of scope) |

## 5. Out of Scope

- Firmware flashing and online update (encrypted vendor images, vendor cloud credentials)
- Factory tools: calibration write, SN writer, reset-all, "Program to MCU"
- App self-update, tray/close dialog, Windows 7 driver installer, RAM indicator
- The vendor "Custom Battery" VDM decoder; VID 0x0483 legacy devices; KM004C
- Copying the vendor UI look

## 6. Traceability Matrix

<!--
  Rows are added by /spec-write (when a cross-cutting AC is authored) or
  by /implement (when an AC ships). At /setup time this table is
  intentionally empty — see the rendered-row example in the top-of-file
  HTML comment for the canonical row shape.
-->

| Requirement | Implementation | Tests |
|-------------|---------------|-------|
| AC-TGE839.1..7 | apps/desktop/src-tauri/src/devices.rs, apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/DevicePicker.svelte, apps/desktop/src/lib/components/TopBar.svelte, apps/desktop/src/lib/format.ts, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/session.svelte.ts, crates/powerz-cli/src/commands/pd.rs | apps/desktop/e2e/ui/replug.spec.ts, apps/desktop/src-tauri/tests/hil.rs, apps/desktop/src-tauri/tests/replug.rs, apps/desktop/src/lib/mockBackend.test.ts, apps/desktop/src/lib/session.svelte.test.ts, crates/powerz-cli/tests/e2e_pd_decoded.rs, crates/powerz-cli/tests/pd.rs |
| AC-TGE83A.1..10 | crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/pd.rs, crates/powerz-cli/src/lib.rs, crates/powerz-protocol/src/decode/mod.rs, crates/powerz-protocol/src/decode/pd_packet.rs, crates/powerz-protocol/src/frame.rs, crates/powerz-protocol/src/session/mod.rs, crates/powerz-protocol/src/session/pd.rs, crates/powerz-sim/src/device.rs, crates/powerz-sim/src/lib.rs, crates/powerz-sim/src/pd.rs | crates/powerz-cli/tests/e2e_pd.rs, crates/powerz-cli/tests/pd.rs, crates/powerz-protocol/tests/pd_packet.rs, crates/powerz-protocol/tests/pd_vectors.rs, crates/powerz-protocol/tests/session_pd.rs, crates/powerz-protocol/tests/vectors/pd_attach_26.hex, crates/powerz-protocol/tests/vectors/pd_detach_334.hex, crates/powerz-protocol/tests/vectors/pd_eprcaps_260.hex, crates/powerz-protocol/tests/vectors/pd_eprmode_60.hex, crates/powerz-protocol/tests/vectors/pd_hardreset_26.hex, crates/powerz-protocol/tests/vectors/pd_psrdy_36.hex, crates/powerz-protocol/tests/vectors/pd_srccap_144.hex, crates/powerz-sim/tests/sim_pd.rs, apps/desktop/src-tauri/tests/hil.rs, crates/powerz-cli/tests/e2e_pd_decoded.rs |
| AC-TGE83B.1..9 | .github/workflows/ci.yml, Cargo.lock, Cargo.toml, crates/powerz-pd/Cargo.toml, crates/powerz-pd/src/blocks.rs, crates/powerz-pd/src/capture.rs, crates/powerz-pd/src/extended.rs, crates/powerz-pd/src/field.rs, crates/powerz-pd/src/header.rs, crates/powerz-pd/src/lib.rs, crates/powerz-pd/src/pdo.rs, crates/powerz-pd/src/rdo.rs, crates/powerz-pd/src/vdm.rs, crates/powerz-protocol/Cargo.toml, crates/powerz-protocol/src/decode/pd_packet.rs, crates/powerz-protocol/src/lib.rs | crates/powerz-pd/tests/chunking.rs, crates/powerz-pd/tests/common/mod.rs, crates/powerz-pd/tests/extended.rs, crates/powerz-pd/tests/golden.rs, crates/powerz-pd/tests/header.rs, crates/powerz-pd/tests/no_io.rs, crates/powerz-pd/tests/pdo.rs, crates/powerz-pd/tests/request.rs, crates/powerz-pd/tests/tree_props.proptest-regressions, crates/powerz-pd/tests/tree_props.rs, crates/powerz-pd/tests/vdm.rs, crates/powerz-protocol/tests/pd_decode_golden.rs |
| AC-TGE83C.1..8 | apps/desktop/src-tauri/src/devices.rs, apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/pd_capture.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/PdAnalyzer.svelte, apps/desktop/src/lib/components/PdClearDialog.svelte, apps/desktop/src/lib/components/TopBar.svelte, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/mockPd.ts, apps/desktop/src/lib/pdCapture.svelte.ts, apps/desktop/src/lib/pdStore.ts, apps/desktop/src/lib/pdTable.ts, crates/powerz-protocol/src/session/pd.rs | apps/desktop/e2e/ui/pd-analyzer.spec.ts, apps/desktop/e2e/ui/pd-perf.spec.ts, apps/desktop/e2e/ui/pd.ts, apps/desktop/src-tauri/tests/common/mod.rs, apps/desktop/src-tauri/tests/hil.rs, apps/desktop/src-tauri/tests/pd_view.rs, apps/desktop/src/lib/backend.pd.test.ts, apps/desktop/src/lib/mockBackend.pd.test.ts, apps/desktop/src/lib/pdCapture.svelte.test.ts, apps/desktop/src/lib/pdStore.test.ts, apps/desktop/src/lib/pdTable.test.ts |
| AC-TGE83D.1..6 | apps/desktop/e2e/hook.ts, apps/desktop/src-tauri/src/pd_capture.rs, apps/desktop/src-tauri/src/store.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/Chart.svelte, apps/desktop/src/lib/components/PdAnalyzer.svelte, apps/desktop/src/lib/e2eHook.ts, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/pdCapture.svelte.ts, apps/desktop/src/lib/pdSelection.svelte.ts, apps/desktop/src/lib/pdStore.ts, apps/desktop/src/lib/pdTimeline.ts, apps/desktop/src/lib/shortcuts.ts, apps/desktop/src/lib/theme.ts | apps/desktop/e2e/ui/pd-timeline.spec.ts, apps/desktop/src-tauri/tests/hil.rs, apps/desktop/src-tauri/tests/pd_timeline.rs, apps/desktop/src/lib/mockBackend.pdTimeline.test.ts, apps/desktop/src/lib/pdCapture.svelte.test.ts, apps/desktop/src/lib/pdSelection.svelte.test.ts, apps/desktop/src/lib/pdTimeline.range.test.ts, apps/desktop/src/lib/pdTimeline.test.ts, apps/desktop/src/lib/shortcuts.test.ts |
| AC-TGE83E.1..6 | Cargo.lock, apps/desktop/src-tauri/Cargo.toml, apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/pd_capture.rs, apps/desktop/src-tauri/src/pd_import.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/PdAnalyzer.svelte, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/pdCapture.svelte.ts, apps/desktop/src/lib/pdSave.ts, apps/desktop/src/lib/pdTable.ts, crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/pd.rs, crates/powerz-protocol/Cargo.toml, crates/powerz-protocol/src/lib.rs, crates/powerz-protocol/src/pdcap.rs | apps/desktop/e2e/ui/pd-save-load.spec.ts, apps/desktop/src-tauri/tests/pd_save_load.proptest-regressions, apps/desktop/src-tauri/tests/pd_save_load.rs, apps/desktop/src-tauri/tests/pd_vendor_import.rs, apps/desktop/src/lib/pdCapture.svelte.test.ts, apps/desktop/src/lib/pdSave.test.ts, apps/desktop/src/lib/pdTable.test.ts, crates/powerz-cli/tests/e2e_pd_save.rs, crates/powerz-protocol/tests/pdcap.rs |
| AC-2BF5G6.1..7 | .gitattributes, .github/workflows/ci.yml | apps/desktop/e2e/ui/budget.test.ts, apps/desktop/e2e/ui/budget.ts, apps/desktop/e2e/ui/ciWorkflow.test.ts, apps/desktop/src-tauri/tests/replug.rs, apps/desktop/src/lib/mockBackend.test.ts |
| AC-PR41FT.1..7 | crates/powerz-cli/src/commands/trigger.rs, crates/powerz-cli/src/trace.rs, crates/powerz-protocol/src/lib.rs, crates/powerz-protocol/src/pdm/caps.rs, crates/powerz-protocol/src/pdm/command.rs, crates/powerz-protocol/src/pdm/detect.rs, crates/powerz-protocol/src/pdm/mod.rs, crates/powerz-protocol/src/pdm/pd.rs, crates/powerz-protocol/src/pdm/port.rs, crates/powerz-protocol/src/pdm/reply.rs, crates/powerz-protocol/src/pdm/risk.rs, crates/powerz-protocol/src/pdm/session.rs, crates/powerz-protocol/src/transport/cdc.rs, crates/powerz-sim/src/device.rs, crates/powerz-sim/src/lib.rs, crates/powerz-sim/src/pdm.rs | crates/powerz-protocol/tests/pdm_detect.rs, crates/powerz-protocol/tests/pdm_props.proptest-regressions, crates/powerz-protocol/tests/pdm_props.rs, crates/powerz-protocol/tests/pdm_session.rs, crates/powerz-sim/tests/pdm_pd.rs, crates/powerz-sim/tests/pdm_sim.rs |
| AC-DGSQ6P.1..7 | crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/trigger.rs, crates/powerz-cli/src/error.rs, crates/powerz-cli/src/trace.rs, crates/powerz-protocol/src/pdm/detect.rs, crates/powerz-protocol/src/pdm/mod.rs, crates/powerz-protocol/src/pdm/pd.rs, crates/powerz-protocol/src/pdm/reply.rs, crates/powerz-protocol/src/transport/cdc.rs, crates/powerz-sim/src/pdm.rs | crates/powerz-cli/tests/hil.rs, crates/powerz-cli/tests/trigger_pd.rs, crates/powerz-protocol/tests/pdm_detect.rs, crates/powerz-protocol/tests/pdm_pd.rs, crates/powerz-sim/tests/pdm_pd.rs |
| AC-DGSQ6Q.1..7 | crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/trigger.rs, crates/powerz-cli/src/error.rs, crates/powerz-cli/src/trace.rs, crates/powerz-protocol/src/pdm/detect.rs, crates/powerz-protocol/src/pdm/legacy.rs, crates/powerz-protocol/src/pdm/mod.rs, crates/powerz-protocol/src/pdm/pd.rs, crates/powerz-protocol/src/pdm/reply.rs, crates/powerz-protocol/src/pdm/risk.rs, crates/powerz-protocol/src/transport/cdc.rs, crates/powerz-sim/src/pdm.rs | crates/powerz-cli/tests/trigger_legacy.rs, crates/powerz-protocol/tests/pdm_detect.rs, crates/powerz-protocol/tests/pdm_legacy.rs, crates/powerz-sim/tests/pdm_legacy.rs, crates/powerz-sim/tests/pdm_pd.rs |
| AC-DGSQ6R.1..8 | apps/desktop/src-tauri/src/devices.rs, apps/desktop/src-tauri/src/trigger.rs, apps/desktop/src-tauri/src/ui_error.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/TriggerView.svelte, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/trigger.svelte.ts, crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/detect.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/trigger.rs, crates/powerz-cli/src/error.rs, crates/powerz-cli/src/trace.rs, crates/powerz-protocol/src/pdm/caps.rs, crates/powerz-protocol/src/pdm/detect.rs, crates/powerz-protocol/src/pdm/legacy.rs, crates/powerz-protocol/src/pdm/mod.rs, crates/powerz-protocol/src/pdm/pd.rs, crates/powerz-protocol/src/pdm/reply.rs, crates/powerz-protocol/src/pdm/session.rs, crates/powerz-protocol/src/transport/cdc.rs, crates/powerz-sim/src/pdm.rs | apps/desktop/e2e/ui/trigger.spec.ts, apps/desktop/src/lib/trigger.svelte.test.ts, crates/powerz-cli/tests/detect.rs, crates/powerz-cli/tests/hil.rs, crates/powerz-protocol/tests/pdm_detect.rs, crates/powerz-protocol/tests/pdm_legacy.rs, crates/powerz-protocol/tests/pdm_pd.rs, crates/powerz-sim/tests/pdm_detect.rs, crates/powerz-sim/tests/pdm_pd.rs |
| AC-DGSQ6S.1..13 | apps/desktop/src-tauri/src/devices.rs, apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/trigger.rs, apps/desktop/src-tauri/src/ui_error.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/PdConsoleView.svelte, apps/desktop/src/lib/components/TriggerConfirmDialog.svelte, apps/desktop/src/lib/components/TriggerView.svelte, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/trigger.svelte.ts, crates/powerz-cli/src/app.rs, crates/powerz-cli/src/commands/detect.rs, crates/powerz-cli/src/error.rs, crates/powerz-protocol/src/frame.rs, crates/powerz-protocol/src/pdm/caps.rs, crates/powerz-protocol/src/pdm/detect.rs, crates/powerz-protocol/src/pdm/legacy.rs, crates/powerz-protocol/src/pdm/mod.rs, crates/powerz-protocol/src/pdm/pd.rs, crates/powerz-protocol/src/pdm/reply.rs, crates/powerz-protocol/src/pdm/session.rs, crates/powerz-protocol/src/session/mod.rs, crates/powerz-protocol/src/session/stream.rs, crates/powerz-sim/src/pdm.rs | apps/desktop/e2e/ui/console.spec.ts, apps/desktop/e2e/ui/trigger.spec.ts, apps/desktop/src-tauri/tests/hil.rs, apps/desktop/src-tauri/tests/replug.rs, apps/desktop/src/lib/backend.trigger.test.ts, apps/desktop/src/lib/mockBackend.trigger.test.ts, apps/desktop/src/lib/trigger.legacy.svelte.test.ts, apps/desktop/src/lib/trigger.svelte.test.ts, crates/powerz-cli/tests/detect.rs, crates/powerz-cli/tests/trigger_legacy.rs, crates/powerz-protocol/tests/pdm_detect.rs, crates/powerz-protocol/tests/pdm_entered.rs, crates/powerz-protocol/tests/pdm_legacy.rs, crates/powerz-protocol/tests/pdm_pd.rs, crates/powerz-protocol/tests/session_stream.rs |
| AC-DGSQ6T.1..7 | apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/trigger.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/PdConsoleView.svelte, apps/desktop/src/lib/components/TriggerView.svelte, apps/desktop/src/lib/console.svelte.ts, apps/desktop/src/lib/mockBackend.ts, crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/console.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/pd_send.rs, crates/powerz-cli/src/commands/trigger.rs, crates/powerz-protocol/src/pdm/console.rs, crates/powerz-protocol/src/pdm/legacy.rs, crates/powerz-protocol/src/pdm/message.rs, crates/powerz-protocol/src/pdm/mod.rs, crates/powerz-protocol/src/pdm/pd.rs, crates/powerz-protocol/src/pdm/risk.rs, crates/powerz-protocol/src/pdm/session.rs, crates/powerz-sim/src/lib.rs, crates/powerz-sim/src/pdm.rs | apps/desktop/e2e/ui/console.spec.ts, apps/desktop/src/lib/console.svelte.test.ts, apps/desktop/src/lib/mockBackend.console.test.ts, crates/powerz-cli/tests/console.rs, crates/powerz-cli/tests/hil.rs, crates/powerz-cli/tests/pd_send.rs, crates/powerz-cli/tests/trigger_legacy.rs, crates/powerz-protocol/tests/pdm_entered.rs, crates/powerz-protocol/tests/pdm_message.rs, crates/powerz-sim/tests/pdm_message.rs |
| AC-E1ZJ2M.1..4 | rust-toolchain.toml, .github/workflows/ci.yml, apps/desktop/scripts/release/toolchain-pin.ts, README.md, docs/tutorials/getting-started.md | apps/desktop/scripts/release/toolchain-pin.test.ts |
| AC-E1ZJ2N.1..11 | .github/workflows/release.yml, apps/desktop/scripts/release/changelog.ts, apps/desktop/scripts/release/versions.ts, docs/how-to/install-a-release.md, README.md, docs/README.md | apps/desktop/scripts/release/changelog.test.ts, apps/desktop/scripts/release/versions.test.ts, apps/desktop/scripts/release/release-workflow.test.ts, apps/desktop/scripts/release/install-guide.test.ts |
| AC-4V3DHP.1..6 | .github/workflows/release.yml, apps/desktop/scripts/release/publish.ts | apps/desktop/scripts/release/publish.test.ts, apps/desktop/scripts/release/release-workflow.test.ts |
| AC-1HHT00.1..7 | apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/RecordChart.svelte, apps/desktop/src/lib/components/RecordsView.svelte, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/records.svelte.ts, crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/records.rs, crates/powerz-cli/src/trace.rs, crates/powerz-protocol/src/decode/mod.rs, crates/powerz-protocol/src/decode/records.rs, crates/powerz-protocol/src/decode/settings.rs, crates/powerz-protocol/src/frame.rs, crates/powerz-protocol/src/session/memory.rs, crates/powerz-protocol/src/session/mod.rs, crates/powerz-protocol/src/session/records.rs, crates/powerz-sim/src/device.rs, crates/powerz-sim/src/lib.rs, crates/powerz-sim/src/records.rs | apps/desktop/e2e/ui/firmware-update.spec.ts, apps/desktop/e2e/ui/meter-settings.spec.ts, apps/desktop/e2e/ui/records.spec.ts, apps/desktop/src-tauri/tests/meter_settings.rs, apps/desktop/src-tauri/tests/records.rs, apps/desktop/src/lib/backend.records.test.ts, apps/desktop/src/lib/mockBackend.meterSettings.test.ts, apps/desktop/src/lib/mockBackend.records.test.ts, apps/desktop/src/lib/records.svelte.test.ts, crates/powerz-cli/tests/commands.rs, crates/powerz-cli/tests/detect.rs, crates/powerz-cli/tests/e2e_pd.rs, crates/powerz-cli/tests/e2e_sim.rs, crates/powerz-cli/tests/firmware_flash.rs, crates/powerz-cli/tests/firmware_info.rs, crates/powerz-cli/tests/firmware_jump.rs, crates/powerz-cli/tests/firmware_probe.rs, crates/powerz-cli/tests/records.rs, crates/powerz-cli/tests/settings.rs, crates/powerz-cli/tests/snapshots/e2e_sim__records_list_sim.snap, crates/powerz-cli/tests/snapshots/e2e_sim__settings_sim.snap, crates/powerz-protocol/tests/protocol_doc.rs, crates/powerz-protocol/tests/records.proptest-regressions, crates/powerz-protocol/tests/records.rs, crates/powerz-protocol/tests/settings_fields.rs, crates/powerz-sim/tests/sim_records.rs |
| AC-1HHT01.1..6 | apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/MeterSettingsView.svelte, apps/desktop/src/lib/components/StatsPanel.svelte, apps/desktop/src/lib/meterSettings.svelte.ts, apps/desktop/src/lib/mockBackend.ts, crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/settings.rs, crates/powerz-protocol/src/decode/mod.rs, crates/powerz-protocol/src/decode/settings.rs, crates/powerz-protocol/src/session/mod.rs, crates/powerz-sim/src/device.rs | apps/desktop/e2e/ui/meter-settings.spec.ts, apps/desktop/src-tauri/tests/meter_settings.rs, apps/desktop/src/lib/backend.meterSettings.test.ts, apps/desktop/src/lib/meterSettings.svelte.test.ts, apps/desktop/src/lib/mockBackend.meterSettings.test.ts, crates/powerz-cli/tests/e2e_sim.rs, crates/powerz-cli/tests/settings.rs, crates/powerz-cli/tests/snapshots/e2e_sim__settings_sim.snap, crates/powerz-protocol/tests/protocol_doc.rs, crates/powerz-protocol/tests/settings_fields.rs |
| AC-1HHT02.1..6 | apps/desktop/src-tauri/src/devices.rs, apps/desktop/src-tauri/src/lib.rs, apps/desktop/src-tauri/src/ui_error.rs, apps/desktop/src-tauri/src/worker.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/TopBar.svelte, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/topBarFit.ts, crates/powerz-cli/src/app.rs, crates/powerz-cli/src/args.rs, crates/powerz-cli/src/commands/mod.rs, crates/powerz-cli/src/commands/read.rs, crates/powerz-cli/src/commands/stream.rs, crates/powerz-cli/src/commands/watch.rs, crates/powerz-cli/src/lib.rs, crates/powerz-cli/src/output.rs, crates/powerz-cli/src/trace.rs, crates/powerz-protocol/src/adcnew.rs, crates/powerz-protocol/src/lib.rs, crates/powerz-protocol/src/pdm/reply.rs, crates/powerz-protocol/src/pdm/session.rs, crates/powerz-protocol/src/session/adcnew.rs, crates/powerz-protocol/src/session/mod.rs, crates/powerz-protocol/src/session/stream.rs, crates/powerz-protocol/src/transport/cdc.rs, crates/powerz-protocol/src/transport/guard.rs, crates/powerz-protocol/src/transport/mod.rs, crates/powerz-sim/src/adcnew.rs, crates/powerz-sim/src/device.rs, crates/powerz-sim/src/lib.rs, crates/powerz-sim/src/queue.rs | apps/desktop/e2e/ui/cdc-source.spec.ts, apps/desktop/src-tauri/tests/cdc_source.rs, apps/desktop/src/lib/backend.cdcSource.test.ts, apps/desktop/src/lib/mockBackend.cdcSource.test.ts, crates/powerz-cli/tests/commands.rs, crates/powerz-cli/tests/e2e_sim.rs, crates/powerz-cli/tests/stream_cdc.rs, crates/powerz-cli/tests/watch_cdc.rs, crates/powerz-protocol/tests/adcnew.rs, crates/powerz-protocol/tests/adcnew_session.rs, crates/powerz-sim/tests/sim_adcnew.rs |
| AC-1HHT03.1..6 | apps/desktop/src-tauri/src/devices.rs, apps/desktop/src-tauri/src/lib.rs, apps/desktop/src/App.svelte, apps/desktop/src/lib/backend.ts, apps/desktop/src/lib/components/AddMeterDialog.svelte, apps/desktop/src/lib/components/Chart.svelte, apps/desktop/src/lib/components/MeterGrid.svelte, apps/desktop/src/lib/meterGrid.ts, apps/desktop/src/lib/meters.svelte.ts, apps/desktop/src/lib/mockBackend.ts, apps/desktop/src/lib/recording.svelte.ts, apps/desktop/src/lib/session.svelte.ts, crates/powerz-cli/src/app.rs, crates/powerz-sim/src/device.rs, crates/powerz-sim/src/lib.rs, crates/powerz-sim/src/memory.rs | apps/desktop/e2e/ui/multi-meter.spec.ts, apps/desktop/e2e/ui/replug.spec.ts, apps/desktop/src-tauri/tests/devices.rs, apps/desktop/src-tauri/tests/multi_meter.rs, apps/desktop/src/lib/backend.multi.test.ts, apps/desktop/src/lib/meterGrid.test.ts, apps/desktop/src/lib/meters.svelte.test.ts, apps/desktop/src/lib/mockBackend.multi.test.ts, apps/desktop/src/lib/mockBackend.test.ts, apps/desktop/src/lib/session.svelte.test.ts, crates/powerz-cli/tests/device_select.rs, crates/powerz-sim/tests/sim_serial.rs |
| AC-1HHT04.1..6 | hardware runbooks run on a KM003C (results outside the repo, summarized in the FR Notes) | — |
| AC-7V4B90.1..7 | README.md, apps/desktop/scripts/readme/readme-features.json, apps/desktop/scripts/readme/readme-features.ts, docs/images/desktop.png | apps/desktop/scripts/readme/readme-features.test.ts |

