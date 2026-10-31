# Changelog

All notable changes to open-powerz are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [0.8.1] — 2026-10-31 — "Loose Ends"

### Fixed
- Trigger panel and console: the panel shows the D+/D- charging hint, a console `entry` line waits long enough to catch its `pdo:` block, a stream error that is not a device loss no longer ends the session while a trigger session opens or closes, and `powerz stream` exits 0 with a note when the meter rejects the stop after gaps (4PJRVH)
- Firmware update: an update can be cancelled until the erase begins, a reloaded page follows the running update instead of offering a second one, a recording on the updating meter asks before it ends, another meter's recording keeps going, and `powerz info` keeps what it read when one block fails (4PJRVJ)
- PD analyzer: a capture keeps its latest 100,000 records and says when it drops the oldest, `pdClear` drops late status samples, a file damaged on its first entry opens empty and names the line, an opened file is released on the next connect, and `pd decode` exits 2 on bad input (4PJRVK)
- Several meters and records: each grid tile has its own connect, rate, Record and Export controls, Record starts at once and asks where to keep the file at Stop, a parked meter keeps its PD capture, a record delete that cannot go ahead is refused before any backup is written, and the app writes `app.log` with no serial or UID (4PJRVM)
- Real-meter fixes: the settings panel reads a block the meter saved before its last reboot, and an unplug is logged as unplugged instead of an error (4PJRVM)
- Release pipeline: the release keeps its tag name and id, the Latest badge stays on the newest final release, release notes carry the AppImage commands, and CI checks the minimum supported Rust (4PJRVN)

### Changed
- Project guide and specs match the code again: the architecture tree lists every crate and the Rust 1.99.1 pin is named (4PJRVP)

Total test count at release: 3848 tests, 0 failures, 0 errors.

## [0.8.0] — 2026-10-30 — "Write Stuff"

### Added
- Delete offline records: `powerz records delete N|--all --i-understand` and the desktop Records tab's Delete and Delete all back the records up as CSV first, ask you to type the record name (or `all`), then rewrite the meter's record list and read it again; a failed backup deletes nothing, and a re-read that does not match names the backup folder and is not retried (JKWT3V)
- Change meter settings: `powerz settings set field=value… --i-understand` and the desktop Meter settings panel's Edit and Apply change the language, brightness, dim timeout, theme, rotation and sample rate (never calibration, auto-rotate or the startup logo) with one write over the meter's serial port, read the settings back and say that the meter's screen shows the change once that setting is opened in its menu (JKWT3W)
- Real-meter checks: deleting records and changing settings were checked on a KM003C, each write approved before it was sent; the checks found that the meter answers a record delete with FINISHED, takes the settings write only on its serial port, and pairs rotation and sample rate the other way round from the vendor tool (JKWT3X)

Total test count at release: 3519 tests, 0 failures, 0 errors.

## [0.7.1] — 2026-10-28 — "Read Me Maybe"

### Changed
- The README tells the real story: the current status, every shipped feature on one line with a link to its guide, what comes next, and a screenshot of the desktop app on the demo device; a gate test fails when a release adds a feature the README does not list or when a README link points nowhere (7V4B90)

Total test count at release: 3334 tests, 0 failures, 0 errors.

## [0.7.0] — 2026-10-28 — "Memory Lane"

### Added
- Offline records: `powerz records list` and `powerz records get N|--all` download the recordings the meter makes on its own as CSV files in the `stream` layout, and the desktop app's Records tab lists them, downloads one or all with progress and opens a downloaded record in the history chart with its statistics; each download reads only that record's memory window, 256 bytes at a time (1HHT00)
- Meter settings, read-only: `powerz settings` (`--json`, `--raw`) and a Meter settings panel show language, brightness, dim timeout, startup logo, theme, auto-rotate, rotation, sample rate and the calibration values, each tagged confirmed or inferred; nothing is ever written (1HHT01)
- ADCNEW stream over the virtual serial port: `powerz stream --source cdc` and `powerz watch --source cdc` use the stream the meter pushes on its CDC port at 50 or 1000 SPS with no sign-in, report records, gaps, dropped frames and CRC errors, and start again when the meter echoes the start after a bulk session; the desktop app offers it as the Serial stream source (1HHT02)
- Several meters at once: the desktop app connects more than one meter, shows them in a grid of tiles with Add meter, Expand, All meters and per-tile Disconnect, and keeps recording, replug and every action on its own meter; a reload picks up every running meter (1HHT03)
- Real-meter checks: settings, offline records, the ADCNEW stream and two meters side by side were checked on a KM003C with read-only runbooks (1HHT04)

Total test count at release: 3294 tests, 0 failures, 0 errors.

## [0.6.1] — 2026-10-27 — "Return to Sender"

### Fixed
- Release publishing finishes in one run: the publish job keeps the id of the draft it creates instead of looking it up again in a release list that can lag behind, checks every release id before using it, and finishes a release whose earlier run stopped at the draft, such as v0.6.0 (4V3DHP)

Total test count at release: 2957 tests, 0 failures, 0 errors.

## [0.6.0] — 2026-10-27 — "Special Delivery"

### Added
- Release builds on a version tag: a `v*` tag, or a manual run for an existing tag, checks the versions and the CHANGELOG, builds unsigned desktop installers (macOS universal .dmg, Windows .msi and setup .exe, Linux AppImage and .deb) and the `powerz` CLI for all three systems, and publishes a GitHub Release with a SHA256SUMS.txt; pull requests that touch the release setup get a dry run, and an install guide explains the unsigned builds (E1ZJ2N)

### Changed
- One pinned Rust toolchain (1.99.1) in `rust-toolchain.toml` for local, CI and release builds; the gate fails when a workflow names its own Rust version (E1ZJ2M)

Total test count at release: 2879 tests, 0 failures, 0 errors.

## [0.5.0] — 2026-10-26 — "Flash Gordon"

### Added
- Device mode and firmware version: application or bootloader mode from the USB interface string, both versions from the info blocks, the old-firmware check, and warnings in `powerz info` and the desktop app; a meter in bootloader mode is listed as such (V9SK52)
- Firmware file inspection: `powerz firmware info` and the desktop app read a ChargerLAB firmware container's model, version, date, CRC and size without decrypting it, and refuse broken or oversized files (V9SK53)
- Firmware flashing engine: `powerz firmware flash` signs in, switches the meter to its bootloader, erases, writes and verifies the image, and brings it back to its application; the bootloader is never written, nothing is retried on its own, and every failure says how to recover (V9SK54)
- Desktop firmware update behind an Advanced firmware switch: file details, a typed version to confirm, each stage with its progress, replug prompts, and recovery steps after a failure (V9SK55)
- Staged checks on a real KM003C, each approved by the user: `powerz firmware probe` and `powerz firmware jump`, and a 2.0.6 → 2.0.7 update confirmed on the meter's About screen (V9SK56)

Total test count at release: 2756 tests, 0 failures, 0 errors.

## [0.4.0] — 2026-10-24 — "Trigger Happy"

### Added
- PDM text session over the meter's virtual serial port: a safe command set, reply and capability-block parsing, a risk classifier, and a single-use confirmation for anything above 5 V; replies that belong to another command stop the session with a power-cycle hint (PR41FT)
- USB PD triggering from the command line: list the charger's offers, request fixed, PPS and EPR AVS voltages, hard reset, and always end back at 5 V (DGSQ6P)
- Legacy and vendor protocol triggering for QC 2.0/3.0, FCP, AFC, SFCP, SCP, VFCP, TFCP and UFCS, each request entering its own protocol first (DGSQ6Q)
- Charger protocol detection: BC1.2 and Apple checks, the meter's automatic list with a sync check after a cut list, and both sources named when they disagree (DGSQ6R)
- Desktop trigger panel: detection, per-protocol request controls, a confirmation dialog above 5 V, failed requests with their reason, Back to 5 V, and a measurement stream that survives the list's pause (DGSQ6S)
- Custom PD messages and a raw console: build control and data messages for SOP, SOP' and SOP'', a Discover Identity preset for e-marked cables, GoodCRC refused, `powerz pd send` and `powerz console` (DGSQ6T)

### Fixed
- Windows CI fails on every failing command and runs green (2BF5G6)

Total test count at release: 2185 tests, 0 failures, 0 errors.

## [0.3.0] — 2026-10-23 — "Packet Sleuth"

### Added
- PD capture core: the meter's PD sniffer starts and stops safely, records are split and timed, and `powerz pd` streams them as text or JSON Lines, with the decoded name and summary of each message (TGE83A)
- USB PD 3.2 message decoder in a new dependency-free `powerz-pd` crate: headers, power data objects, requests, VDMs, extended messages and chunk reassembly, each with a one-line summary and a field tree (TGE83B)
- Desktop PD analyzer: start and stop a capture, a fast table with GoodCRC, type and SOP filters, a details pane with highlighted raw bytes, follow mode and Clear (TGE83C)
- PD messages as coloured markers on the power chart, linked both ways with the packet table, plus the PD status VBUS, IBUS, CC1 and CC2 values (TGE83D)
- Save and open PD captures in the documented `.pdcap` format, `powerz pd --save` and `powerz pd decode`, and a read-only import of the vendor's PD log database (TGE83E)

### Fixed
- After an unplug the app keeps the meter selected, waits, and reconnects to it on its own with the gap marked, instead of falling back to the demo device (TGE839)
- CI is green on all eight jobs: real stalls fixed and timing races made deterministic (KQF5GM)

Total test count at release: 1562 tests, 0 failures, 0 errors.

## [0.2.0] — 2026-10-22 — "Live Wire"

### Added
- Desktop app device backend: a device picker with the demo device, a session worker per meter, and live streaming to the UI in batches (VHY378)
- Desktop shell: a big live readout, light, dark and system themes, keyboard access and screen-reader announcements (AH2AEK)
- Live chart of VBUS, IBUS, power, D+, D−, CC1 and CC2 with per-series toggles (AH2AEM)
- Session history with min/max decimation at every zoom, and statistics: min, max, average, charge and energy (AH2AEJ)
- Recording and CSV export in the vendor-compatible layout, shared with `powerz stream` (0P7HSJ)
- Whole-session view on session time, with shaded "No data" bands for gaps and readable axes (272WSN)
- Live and paused chart states with a "Go live" button (DDDYDJ)
- Timeline navigation: overview strip, wheel and trackpad zoom and pan, playhead, J/K/L, zoom slider and presets (FENCD9)
- The UI picks up a running session after a page reload instead of starting disconnected (VEDDFD)
- Notices after a recording or export with Reveal in Finder / Show in folder and Copy path, deep zoom to two samples, and empty-chart states (9CFMD3)
- Desktop end-to-end tests: mocked-UI Playwright with axe on all three OSes, real-app WebdriverIO on Linux and Windows (AH2AEN)

### Changed
- A new app icon: a USB-C port drawn as the meter's display (5MTE4V)

### Fixed
- The live chart no longer leaks memory in the macOS web view (4NSVC9)
- Clear chart history clears what the chart, Whole session and Export show (5X154V)
- Chart and layout polish: the remembered preset, notices that never move the chart, single values in the hover readout (AKNVND)
- Honest gaps on the overview strip, minimum axis spans against idle noise, and no stale temperatures in CSVs (CWWKJ6)
- Rate keys locked during a recording, Power drawn while paused, and throttled range announcements (JFD9XR)
- Keyboard and overlay behaviour: Esc, the ⋯ menu and focus return (M46CTX)
- Real-meter fixes: no holes at 1000 SPS behind a save sheet, correct gap counts on stop, matching Unix times, temperature at 1000 SPS, and recovery after quitting mid-stream (X2SPDW)
- Final polish: a top bar that stays put during an export, notices that never cover Stats, precise short-range announcements, continuous strip lines and a poll thread at user-interactive priority on macOS (SX3EDV)

Total test count at release: 1082 tests, 0 failures, 0 errors.

## [0.1.0] — 2026-10-20 — "Wire Whisperer"

### Added
- Protocol reference document: `docs/protocol.md`, the KM003C / KM002C USB protocol in our own words, with confidence tags (ZVJ3EB)
- Frame codec: message headers, PUT_DATA section walker and sequence counter (BXYTJX)
- Measurement and settings decoders: ADC snapshot, ADC queue records, sample rates, settings and device-info blocks (E88WR5)
- Device discovery and transports: vendor bulk (`nusb`), CDC (`serialport`) and a guarded, ADC-only HID fallback (`hidapi`) (SJGK7E)
- Device session: CONNECT, memory read, AES authentication with the shipped keys, device info and ADC-queue streaming at 2/10/50/1000 SPS (N6EHQC)
- `powerz` command-line tool: `list`, `info`, `read`, `watch` and `stream` (vendor-compatible CSV), `--json`, `--trace` and stable exit codes (STQEA1)
- Device simulator (the demo device, `--transport sim`) with named scenarios, and end-to-end CLI tests against it (G0W08Z)

Total test count at release: 370 tests, 0 failures, 0 errors.
