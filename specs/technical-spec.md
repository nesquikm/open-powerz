# Technical Specification

## 1. Architecture

### System Overview

A Rust cargo workspace with three crates and one web UI:

- `powerz-protocol` (library): everything that knows the device. Transports (vendor bulk via
  `nusb`, HID via `hidapi`, CDC via `serialport`) behind a transport trait; frame building and
  parsing (main header, extended section headers); decoders for ADC snapshots, ADC-queue
  records, PD/UFCS capture records, SETTINGS and offline-record tables; AES-128-ECB auth and
  memory read; the PDM text-command layer and the CDC binary/text demux. Pure decode
  functions are separated from I/O so they are testable with byte vectors.
- `powerz-cli` (`powerz` binary): device listing, live readout, CSV streaming, raw frame
  dumps for protocol work and device tests.
- `open-powerz-desktop` (Tauri 2 backend, `apps/desktop/src-tauri`): owns device sessions on a
  worker thread, exposes Tauri commands, and streams batched measurement frames to the UI
  over `tauri::ipc::Channel`.
- Web UI (`apps/desktop/src`): Svelte 5 + TypeScript, uPlot charts, built by Vite and embedded
  into the Tauri binary.

The protocol reference will live in the repo's `docs/` tree as `protocol.md` (planned): rewritten in our own words from the
research notes (outside the repo), keeping `[device]` / `[static]` / `[inferred]` tags.

### Directory Structure

```
Cargo.toml                       # workspace: resolver 3, edition 2024, MSRV 1.85
package.json / bun.lock          # Bun workspace root (apps/desktop)
crates/powerz-protocol/src/      # lib.rs (+ transport/, frame/, decode/, auth/ as they land)
crates/powerz-cli/src/main.rs    # `powerz` binary
apps/desktop/src/                # App.svelte, main.ts, lib/*.ts (+ co-located *.test.ts)
apps/desktop/src-tauri/          # Tauri crate: lib.rs (commands), tauri.conf.json, capabilities, icons
docs/                            # user docs + protocol.md
.github/workflows/ci.yml         # gate matrix: ubuntu-24.04, macos-latest, windows-latest
specs/                           # SDD specs (this file)
```

### Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Build new vs. fork | Build new; laPower, km003c-rs and other prior art are references only | Full control and a strict clean room |
| GUI stack | Tauri 2 + Svelte 5 + TS + uPlot | Rich tables/trees, i18n, CJK text and a larger contributor pool; shares the Rust core with the CLI |
| USB stack | `nusb` (bulk, primary), `serialport` (CDC, second pipe), `hidapi` (HID, read-only ADC fallback) | Pure Rust bulk with hotplug and no libusb DLL; HID limited because multi-report replies reboot the meter (device session 1) |
| MSRV | Current stable (1.96) | New project; an older MSRV only holds dependencies back |
| AES keys | Shipped in the repository | User decision: authenticated features work out of the box |
| JS tooling | Bun (package manager + script runner), Vite 8, Vitest 5 | Fast installs; Vite is Tauri's reference bundler |
| License | MIT | User decision |

## 2. Data Model

To be defined per FR. Core types (planned): `Frame`/`MainHeader`/`SectionHeader`,
`AdcSnapshot`, `QueueRecord`, `PdRecord`/`PdEvent`, `Settings`, `DeviceInfo`; all in SI units
(`f64` volts/amps or fixed-point integers with explicit scale) with signed current.

## 3. API / Interface Design

- Library: a `Device` handle (open by transport, `request(&Request) -> Result<Reply>`), plus
  pure `parse_*` / `build_*` functions. Public API documented with rustdoc (packages docs).
- Tauri commands: `app_info` (today); device list/open/close and stream start/stop as FRs land.
- CLI: `powerz <subcommand>` with `clap` once the first command lands.

## 4. Key Patterns

### State Management
Device sessions live in the Rust backend (one worker per device, one request in flight). The
UI holds view state in Svelte 5 runes and receives measurement batches over a Channel.

### Error Handling
`thiserror` error enums in `powerz-protocol` (transport, timeout, framing, auth, rejected);
`anyhow` in binaries. Device data never causes a panic.

### Testing Strategy
See `specs/testing-spec.md`. Decoders are tested with byte vectors; transports with fakes.

## 5. Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| tauri / tauri-build | 2.12.4 / 2.7.3 | Desktop shell |
| serde / serde_json | 1.x | Tauri IPC payloads |
| nusb | 0.2.x (planned) | Vendor bulk transport, hotplug |
| hidapi | 2.6.x (planned) | HID transport |
| serialport | 4.10.x (planned) | CDC transport |
| aes, crc32fast | 0.9.x, 1.5.x (planned) | Auth / memory read crypto, CRC-32 |
| thiserror / clap | 2.x / 4.6.x (planned) | Errors / CLI args |
| csv, ctrlc | latest (planned) | CLI stream export, Ctrl-C handling |
| proptest, assert_cmd | latest (planned, dev) | No-panic parser properties, CLI tests |
| svelte | ^5.57.2 | UI framework |
| vite / @sveltejs/vite-plugin-svelte | ^8.3.4 / ^7.3.1 | Bundler |
| @tauri-apps/api / @tauri-apps/cli | ^2.12.4 / ^2.12.3 | Tauri JS bindings / CLI |
| uplot | ^1.6.32 | Charts |
| typescript | ~6.0.3 | Types (svelte-check and typescript-eslint do not support TS 7 yet) |
| vitest | ^5.0.5 | TS unit tests |
| eslint / typescript-eslint / eslint-plugin-svelte / prettier | ^10.13 / ^8.72 / ^3.23 / ^3.9 | Lint + format |

## 6. Risks & Considerations

- **Linux:** the mainline `powerz` hwmon driver binds the bulk interface; we must detach it and
  ship a udev rule (`uaccess`, `ID_MM_DEVICE_IGNORE=1`).
- **macOS HID:** only through IOHIDManager (`hidapi`); libusb/nusb cannot claim it.
- **Firmware drift:** most protocol facts come from V2.5.2 binaries and FW 1.9.9 prior art; our
  device runs FW 2.0.6 (device session 1 confirmed the core protocol on it). Every unconfirmed
  fact stays `[inferred]` until a device test confirms it; KM002C is untested.
- **Device safety:** a wrong command can hang, reboot or brick the meter. A single guard in the
  transport layer refuses the never-send set before any byte is written, and HID only carries
  single-report GET_DATA.
- **WebKitGTK** on Linux is slower; chart decimation must happen in Rust.
- **Tauri build order:** `apps/desktop/dist` must exist before the Tauri crate compiles.

## Architecture Decision Records

| Decision | Options Considered | Choice | Rationale |
|----------|-------------------|--------|-----------|
| GUI toolkit | egui/eframe, Iced, Tauri 2 + web UI, PySide6, Qt6 C++, Electron | Tauri 2 + Svelte 5 | User decision; better tables/trees/i18n than egui, small bundles vs. Electron |
| Prior-art reuse | Depend on km003c-lib, fork laPower, build new | Build new | User decision; prior art is a test oracle and reference only |
| Key policy | Ship keys, user-supplied key file, drop auth features | Ship keys | User decision |
