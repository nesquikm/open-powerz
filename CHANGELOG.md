# Changelog

All notable changes to open-powerz are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

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
