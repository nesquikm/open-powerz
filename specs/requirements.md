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
**Later** = PD sniffer, protocol trigger panel (CDC), offline recordings, multi-device,
UFCS analyzer, vendor `.db`/`.sqlite` import/export. Each lands as its own FR in
`specs/frs/`.

Roadmap (one plan per milestone in `specs/plan/`): M_34BAXK Protocol Core and CLI →
M_R4WG4Q Desktop Meter → M_NE3C7K PD Analyzer → M_X4VEQ3 Triggers and Extras. Firmware
update and signed release packaging are not planned.

## 2. Functional Requirements (cross-cutting only)

- **Platforms:** every user-facing feature works on macOS (Apple silicon + Intel), Linux
  (x86_64; udev rule documented) and Windows 10/11 with no vendor driver installed.
- **Device identification:** devices are matched by USB VID `0x5FC9`; interfaces are chosen by
  class (vendor bulk 0xFF, HID 0x03, CDC 0x02/0x0A), never by interface number.
- **Transport fallback:** vendor bulk is the primary pipe; CDC (binary protocol on the virtual
  serial port) is the second pipe; HID is only a read-only fallback for single-report readings,
  because multi-report HID replies hang and reboot the meter (device session 1, FW 2.0.6).
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
- At 1000 SPS a reply holds at most 63 queue records: poll at least every ~30–60 ms.
- On CDC, frame types 0x01/0x02/0x03 start or stop the ADCNEW push stream, so CONNECT must
  never be sent there.
- Linux `powerz` hwmon kernel driver bound to the bulk interface: detach it or fall back to HID.
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
