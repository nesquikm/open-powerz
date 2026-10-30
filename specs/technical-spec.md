# Technical Specification

## 1. Architecture

### System Overview

A Rust cargo workspace with six crates and one web UI:

- `powerz-protocol` (library): everything that knows the device. Transports (vendor bulk via
  `nusb`, HID via `hidapi`, CDC via `serialport`) behind a transport trait; frame building and
  parsing (main header, extended section headers); decoders for ADC snapshots, ADC-queue
  records, PD/UFCS capture records, SETTINGS and offline-record tables; AES-128-ECB auth and
  memory read; the PDM text-command layer (protocol triggering, detection, custom PD
  messages, raw console) and the CDC binary/text demux; the ADCNEW push stream on the CDC
  port (start, poll, keep-alive, stop; CRC-8 frames, resync after a bad one); the two meter
  writes (offline-record delete, settings write) and firmware update. Pure decode
  functions are separated from I/O so they are testable with byte vectors.
- `powerz-cli` (`powerz` binary): device listing, info, live readout (`watch`), CSV
  streaming (`stream`, over the ADC queue or the ADCNEW stream), settings read and write,
  offline records (list, get, delete), PD capture, protocol triggering and detection over
  PDM, the raw PDM console, firmware update, and raw frame dumps for protocol work.
- `open-powerz-desktop` (Tauri 2 backend, `apps/desktop/src-tauri`): a device manager (listing
  plus hotplug polling, real meters and demo-device scenarios) and one session worker thread
  per connected meter. The worker owns the `Session`, interleaves queue polls with ADC
  snapshots (the queue carries no temperature), keeps the per-session history with a min/max
  decimation pyramid and running statistics, feeds the CSV recorder, and streams batched
  events (20–60 Hz) to the UI over `tauri::ipc::Channel`.
- `powerz-record` (library): the vendor-compatible CSV recorder shared by the CLI and the
  desktop app (header, plain-decimal formatting, time origin, signed charge/energy
  integration).
- `powerz-pd` (library): the USB PD r3.2 message decoder (headers, PDOs/RDOs, VDMs,
  extended messages and chunk reassembly, a one-line summary and a field tree per
  message). It has no dependencies and does no I/O, so it also builds for wasm32 (checked
  in CI) for the later web edition; `powerz-protocol` re-exports it as `powerz_protocol::pd`.
- `powerz-sim` (library): a stateful simulated KM003C behind the same `Transport` trait, with
  bulk, CDC and HID personas, named scenarios and fault injection. It is both the e2e test
  device and a shipped "demo device" (`powerz --transport sim`, and the desktop device
  picker).
- Web UI (`apps/desktop/src`): Svelte 5 + TypeScript, uPlot charts, built by Vite and embedded
  into the Tauri binary.

The protocol reference lives in `docs/protocol.md`: rewritten in our own words from the
research notes (outside the repo), keeping `[device]` / `[static]` / `[inferred]` tags.

### Directory Structure

```
Cargo.toml                       # workspace: resolver 3, edition 2024, MSRV 1.96
package.json / bun.lock          # Bun workspace root (apps/desktop)
crates/powerz-protocol/src/      # transport/, frame, decode/, session/, crypto/, pdm/, adcnew, firmware/, pdcap
crates/powerz-cli/src/main.rs    # `powerz` binary
crates/powerz-record/src/        # shared CSV recorder + charge/energy integrator
crates/powerz-pd/src/            # USB PD message decoder (no deps, no I/O, wasm32-clean)
crates/powerz-sim/src/           # simulated KM003C (demo device, e2e double)
apps/desktop/src/                # App.svelte, main.ts, lib/*.ts (+ co-located *.test.ts)
apps/desktop/src-tauri/          # Tauri crate: commands, devices, worker, batch, history, stats, recording
apps/desktop/e2e/app/            # WebdriverIO + tauri-driver suite (real app, Linux + Windows)
apps/desktop/e2e/ui/             # Playwright + axe suite (web build, mocked backend, all OSes)
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
| MSRV | 1.96 (`rust-version`) | New project; an older MSRV only holds dependencies back |
| Build toolchain | One exact version in `rust-toolchain.toml` (1.99.1), used locally, in CI and in release builds; workflows never name their own | A new Rust on the runners can't break a green branch with new lints; moving it is a reviewed one-line change |
| AES keys | Shipped in the repository | User decision: authenticated features work out of the box |
| JS tooling | Bun (package manager + script runner), Vite 8, Vitest 5 | Fast installs; Vite is Tauri's reference bundler |
| License | MIT | User decision |

## 2. Data Model

Defined per FR. Core types: `Frame`/`MainHeader`/`SectionHeader`,
`AdcSnapshot`, `QueueRecord`, `PdRecord`/`PdEvent`, `Settings`, `DeviceInfo`; all in SI units
(`f64` volts/amps or fixed-point integers with explicit scale) with signed current.

## 3. API / Interface Design

- Library: a `Device` handle (open by transport, `request(&Request) -> Result<Reply>`), plus
  pure `parse_*` / `build_*` functions. Public API documented with rustdoc (packages docs).
- Tauri commands: `app_info`, `list_devices`, `watch_devices`, `connect`, `attach` (a reloaded page
  takes over the running session), `disconnect`, `set_rate`,
  `history_range`, `history_gaps`, `stats`, `reset_stats`, `clear_history`, `start_recording`,
  `stop_recording`, `export_session`, `cancel_wait` (stop waiting for an unplugged meter),
  `running_meters` (the running sessions' device ids, for a reload), `meter_settings`,
  `records_list` and `record_download` (read-only meter data), `records_delete` and
  `meter_settings_write` (the two meter writes, each confirmed in the UI first), and
  for the PD analyzer `pd_start`, `pd_stop`, `pd_details` (a record's raw bytes and field tree,
  on demand, from the live capture or, with `opened`, from an opened file), `pd_save` / `pd_open`
  (`.pdcap` files) and `pd_import` (read-only vendor PD log) and `pd_clear` (answers the next record index; indices never restart, so rows sent
  before a clear are recognised and dropped); backend events `Batch`, `Gap`, `Status`
  (including `waiting`), `Error`, `Recording` (started / progress / stopped / failed),
  `PdState`, `PdRows` (batched, at most 60 a second, decoded in the backend; each row carries
  `session_ms` on the measurement samples' time base) and `PdStatus` (the PD status blocks'
  VBUS / IBUS / CC1 / CC2, batched the same way) over one session channel, with export progress on its own channel. The UI reaches them only through `apps/desktop/src/lib/backend.ts`, the
  seam the Playwright mock replaces. Every command that acts on a meter takes an optional
  `device_id` (several meters at once; without it, the most recently connected running meter),
  and `connect` takes a `source`: `queue` (ADC queue over bulk) or `cdc` (the ADCNEW stream).
- CLI: `powerz <subcommand>` with `clap` (derive).

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
| serde / serde_json | 1.x | Tauri IPC payloads; the `.pdcap` capture file format (`powerz-protocol`) |
| rusqlite | 0.40.x, `bundled` | Read-only import of the vendor PD log database (desktop) |
| nusb | 0.2.x | Vendor bulk transport, hotplug |
| hidapi | 2.6.x (`hid` feature, default on) | HID transport |
| serialport | 4.10.x | CDC transport: the binary protocol, the ADCNEW stream and the PDM text session |
| aes, crc32fast | 0.8.x, 1.5.x | Auth / memory read crypto, CRC-32 |
| thiserror / clap | 2.x / 4.6.x | Errors / CLI args |
| ctrlc / csv | 3.5.x / 1.4.x (dev) | CLI Ctrl-C handling / CSV checks in CLI tests |
| proptest, assert_cmd | 1.12.x, 2.2.x (dev) | No-panic parser properties, CLI tests |
| svelte | ^5.57.2 | UI framework |
| vite / @sveltejs/vite-plugin-svelte | ^8.3.4 / ^7.3.1 | Bundler |
| @tauri-apps/api / @tauri-apps/cli | ^2.12.4 / ^2.12.3 | Tauri JS bindings / CLI |
| uplot | ^1.6.32 | Charts |
| tauri-plugin-dialog | 2.8.x | Save dialogs (recording, export, PD capture) and open dialogs (PD capture, vendor PD log) |
| @wdio/cli + tauri-driver | 10.1.0 (dev) | Real-app e2e (Linux, Windows) |
| @playwright/test + @axe-core/playwright | 1.64.1 / 4.14.0 (dev) | UI e2e and accessibility scan |
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
  the ADC snapshot (GET_DATA mask 0x001). The only writes past it are two narrow paths, each
  checked frame by frame: the record table (SET_DATA 0x48, attribute 0x28) and the settings
  write (type 0x0A, over CDC only).
- **Meter writes:** deleting offline records rewrites the record table with one SET_DATA
  0x48 (attribute 0x28) carrying the kept entries in order, then re-reads the list; a
  mismatch is an error and nothing is retried. The desktop app first backs the affected
  records up as CSV (`open-powerz-backups/records-<time>` under Downloads) and needs the
  typed record name (or "all"). The settings write reads SETTINGS, sends one type 0x0A
  write with the changed display / rate fields over the values just read (calibration,
  startup logo and auto-rotate are never written), then reads SETTINGS back; a changed
  field without its new value, or any other field that moved, is an error. [device]
  (single-record delete and settings write; delete-all is checked on the simulator only)
- **WebKitGTK** on Linux is slower; chart decimation must happen in Rust.
- **Tauri build order:** `apps/desktop/dist` must exist before the Tauri crate compiles.

## Architecture Decision Records

| Decision | Options Considered | Choice | Rationale |
|----------|-------------------|--------|-----------|
| GUI toolkit | egui/eframe, Iced, Tauri 2 + web UI, PySide6, Qt6 C++, Electron | Tauri 2 + Svelte 5 | User decision; better tables/trees/i18n than egui, small bundles vs. Electron |
| Prior-art reuse | Depend on km003c-lib, fork laPower, build new | Build new | User decision; prior art is a test oracle and reference only |
| Key policy | Ship keys, user-supplied key file, drop auth features | Ship keys | User decision |
