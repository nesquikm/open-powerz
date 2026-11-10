# Changelog

All notable changes to open-powerz are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [0.12.2] — 2026-11-10 — "Fine Print"

### Fixed
- The README desktop screenshot shows stats for the zoomed range, matching its chart (PFYFR6)
- Docs: the web device menu order and its "(HID)" label, the CLI CSV TEMP column, the CLI archive folder in Install a release, which desktop-only controls the web edition hides, and Cancel keeping the open recording (AMJB0K)
- Tests pin the docs review: the README screenshot, every relative link and anchor in the README and docs, the CLI transport rules, and what stays desktop-only (SPYPSD)

Total test count at release: 7792 tests, 0 failures, 0 errors.

## [0.12.1] — 2026-11-10 — "Loose Ends"

### Fixed
- `powerz watch` over the serial port keeps running while the browser streams: stray stream frames are skipped, `--source cdc` falls back to ADC snapshots, and a missed snapshot skips one refresh instead of ending the watch (3B70S8, NVN6M7)
- Both apps say when the meter stops sending samples because another program polls it over the serial port, and the notice stays put until samples keep coming again (NVN6M8, EHY1JW)
- Web edition: the device menu lists a meter granted over both WebUSB and WebHID once; a HID-only meter says "(HID)" (EHY1JX)
- Web edition: Cancel interrupts a large Open recording parse and keeps the file that was open before (3B70S6)
- PD analyzer: a wheel scroll up in a live capture pauses following instead of snapping back to the bottom (3B70S5)
- PD triggering: an unasked offer list refreshes the ready offers (3B70SA)
- The first rows of a session have no TEMP until the first valid reading, the same in both editions, and the docs say so (3B70S7, 3B70S9)
- File tab: stats follow the zoomed range, with min and max, and the scope toggle shows which side is on (PBDEQE, EWEV1Z)
- The recorder's chart memory line names the live chart buffer, not the session history (2VPF6P)
- README and docs match the app, with a new README screenshot and one name for the web edition (SHTZHQ, Z9PC5A)
- Tests: the live windows and zoom readout e2e tests no longer depend on wall time (3B70S3, 3B70S4)

### Known issues
- Import vendor log is tested on synthetic files only; it has not run against a real vendor PD log yet
- Settings Apply is refused while another program, such as `powerz watch` over the serial port, holds the meter's serial port

Total test count at release: 7765 tests, 0 failures, 0 errors.

## [0.12.0] — 2026-11-10 — "Full Tab"

### Added
- Web edition: meter settings and offline records over WebUSB: read the settings, then list, download and open the meter's own recordings (W8SHX8)
- Web edition: change the meter's settings over Web Serial, with two matching reads, one write and a read-back check (W8SHX9)
- Web edition: delete offline records after a CSV backup to a folder you pick and a typed confirmation (W8SHXA)
- Web edition: startup logo preview, upload, remove and read, each write checked by reading it back (W8SHXB)
- Web edition: protocol triggering over Web Serial with the desktop's own trigger engine, a confirmation above 5 V and "Don't ask again" up to 20 V (W8SHXC)
- Web edition: Open recording reads a CSV or `.db` recording in the browser, no meter needed (AHTZW3)
- Web edition: open and save PD capture files (`.pdcap`) and import a vendor PD log in the PD analyzer (EAKFEC)
- Web edition: an (i) beside the device menu explains WebUSB, HID and Web Serial and names the current connection (60QB2S)

### Changed
- Docs: the browser-support page lists every feature with the interface it needs, and the README names what stays desktop-only (W8SHXF)

### Fixed
- Web edition: Export .db downloads a `.db` of the session, and both session exports fill the TEMP column (AHTZW2, JFRPME)
- Web edition settings changes: another program on the serial port is caught before the write, and a change that landed is no longer reported as failed (CJ3QC9, CJ3QCA, X3H3JY)
- Web edition trigger: a confirmed request reaches the engine, the first request after Detect no longer times out, and a request after a failed PD entry waits for fresh offers (K4DWCF, AHTZW1, AHTZW4)
- Web edition: Refresh lists a record made after connect, the logo tab over HID gives the HID reason and clears on reconnect, the stored-logo read shows progress, and the rate picker and record totals match the meter (CJ3QCB, CJ3QCC, CJ3QCD)
- Web edition: Read is disabled on the logo tab over HID, TEMP is judged when a batch arrives, and a new file pick replaces one that never answered (Z99166)
- Rate keys over HID no longer try to change the rate (W8SHXD)
- Glossary popovers for VFCP, TFCP and SFCP and the other abbreviations that had none (62VEDM)
- Release builds cache the bundler's tools and retry packaging once when the download times out (W8SHXE)

### Known issues
- Cancel can't interrupt a large Open recording parse in the browser
- The first ~20 rows (0.4 s) of a web session after a HID↔USB reconnect have an empty TEMP
- `powerz --transport cdc watch` exits by itself while the browser streams over WebUSB
- Import vendor log is tested on synthetic files only; it has not run against a real vendor PD log yet

Total test count at release: 7488 tests, 0 failures, 0 errors.

## [0.11.1] — 2026-11-09 — "Fine Print"

### Fixed
- Web edition over HID: the disabled rate picker says the rate is not available over HID instead of blaming authentication (09R2FK)
- Web edition: no "Pick “Connect a meter…”" hint in a browser without WebUSB and WebHID, such as Safari, where the not-supported box explains it (09R2FM)

Total test count at release: 6531 tests, 0 failures, 0 errors.

## [0.11.0] — 2026-11-09 — "Open Tab"

### Added
- Web edition: open-powerz in Chrome or Edge at nesquikm.github.io/open-powerz, with live readings, charts, stats, passive PD capture, CSV recording and the demo meter over WebUSB, and a WebHID fallback for basic readings (QFTCX0)
- The protocol core, PD decoder and simulator build for WebAssembly, with the native transports behind cargo features (QFTCWX)
- `powerz-wasm`: a WebAssembly binding for the request builders, reply decoders and demo meter (QFTCWY)
- Browser transports and a read-only session driver: WebUSB by interface class, WebHID for snapshots, and an opt-in frame log (QFTCWZ)
- GitHub Pages site under /open-powerz/ with a not-supported view for browsers without WebUSB, deployed on `v*` tags or by hand (QFTCX1)

### Changed
- Web edition: "Connect a meter…" and "Connect over HID…" live in the device menu, with a hint while only the demo is listed (AYR4W6)
- Docs: a web edition guide with browser support, the device picker and permissions, and the WebHID fallback (QFTCX2)

### Fixed
- Web edition sign-in to a real KM003C: stale data drained at open, a silent first connect recovered, a split memory read reassembled, and a recovery that cancels its own read no longer ends the session (VG37JA, 7T1YGJ)
- Web edition stream: the meter's empty queue reply means "nothing yet" instead of a broken frame (F3M587)
- Web edition connect: a failed connect releases the meter, the meter you pair is the one selected, and a replugged meter replaces its dead menu entry (G30K4R, XPT7PZ, R13Y5V)
- Web edition: no "Reveal in Finder" on a saved recording, and the demo's statistics clear when a real meter connects (R13Y5W)
- Drop-downs draw their own arrow with room before the edge, and the device picker fits its longest label again (K1GZTR, 350H0S)
- Build and tests: build:wasm picks a Rust with the wasm target, the gate typechecks before the wasm module exists, the serial lint matches Windows paths, and two timing-sensitive tests hold (G30K4S, Y6JFM3, HT1EMH, FFQVBQ, B67EYP)

Total test count at release: 6513 tests, 0 failures, 0 errors.

## [0.10.0] — 2026-11-07 — "First Impression"

### Added
- Startup logo preview: see any image as the KM003C's startup logo, opaque or with transparent pixels, before anything is written (PVFHM7)
- Guarded logo writes: only the logo's own flash window, its on/off flag and reads inside that window get through; every other flash write stays refused (R1ST8Z)
- Demo device: a simulated logo flash, so upload, remove and read-back can be tried without a meter (A2XTES)
- Logo upload, remove and read-back in the session, each write checked by reading it back (TZK5K4)
- CLI logo commands: `powerz logo preview`, `logo read`, `logo write` and `logo remove`, with a typed confirmation (YCRQRQ)
- Desktop logo view: preview, write, read back or remove the startup logo from the app (MEHQC0)
- Built-in open-powerz logos: the mascot, or the mascot with "open-powerz" lettering, with no image to make first (70X5YX)
- Brand art: the electron-surf mascot as the app icon and the README banner (492Z4N)

### Changed
- Hide serial numbers: a hidden meter reads as its model, numbered when two of the same model are listed, and *** where a value must show (CCF47K)

### Fixed
- Logo writes: the tool says up front that Upload sets a 3 s startup screen and Remove turns it off, then guides the menu save the meter needs afterwards, in the CLI and the desktop app (Z39H24, QG6RNY)
- Session actions such as Record and Export stay dimmed while a logo write runs (70X5YY)
- Settings writes: the chart keeps drawing after a write, and the hint says the meter's screen shows the change once the value is changed in its menu (JV54SN, JV54SP)
- Trigger panel: Back to 5 V hides while the charger is gone, and leaving the view or quitting always sends the session close, with the charger attached or not (JV54SQ, ZRJ8W6)
- Docs builds: the library and workspace API docs build with warnings denied again (1DHPPW, TJNB54)

Total test count at release: 6011 tests, 0 failures, 0 errors.

## [0.9.0] — 2026-11-05 — "Fine Print"

### Added
- Trigger panel "don't ask again": the confirmation dialog can skip the question for later requests up to 20 V until you go back to 5 V, hard reset, disconnect or quit; EPR requests above 20 V and the raw console always ask (AXHK2N)
- Cable card: one Read cable press asks the cable's e-marker for its vendor, current, voltage, speed and whether it is active, says plainly why when no answer comes, and can show an identity already seen in a PD capture without sending anything (AXHK2P)
- Hide serial numbers: one option hides serial numbers everywhere in the desktop app, tiles switch to position labels, and the CLI gains a matching flag (AXHK2Q)
- Recorder: pause and resume, an automatic stop when current stays low, a row limit, a file name prefix, a default folder, and how long memory can hold the current stream (AXHK2R)
- Open recordings: CSV recordings in our layout or the vendor's open for charts and statistics, and the vendor's SQLite database format can be read and written (AXHK2S)
- Chart ruler, percentage marks and energy share: a two-cursor ruler shows time, charge, energy and averages, marks show when charge or energy reached each share, plus an energy share series, a second chart and a reversed current axis (AXHK2T)
- Chart appearance and image export: axis range presets, a title, per-series colour, width and transparency, and PNG, SVG or PDF export at a chosen size with no serial number (AXHK2V)
- Info popovers: every feature and abbreviation has an (i) that says what it does and, where it matters, its risk (CW66T1)
- Firmware update: the progress section keeps the device list's labels for the updated meter, so the bootloader switch can be read after the update ends (XCCQAK)

### Changed
- Memory at 1000 SPS: PD status data is capped, the chart allocates less, long live sessions keep older samples at a coarser resolution, and live batches go to the page 20 times a second instead of 60, so the macOS web view stays within its budget (AXHK2W, HR2JAG)

### Fixed
- Firmware update: the backend never ends a session you did not agree to release, cancel and reload work as expected, the device list line keeps its last label, and the Writing stage explains the 16 bytes the file stores past its image (AXHK2F, AXHK2M, PGY704, VQCKC6)
- App log: errors that leave a session running and firmware updates are logged, the file size is limited, and it still holds no serial or UID (AXHK2G)
- Settings writes read the settings block twice and refuse when the reads differ, an unplug that surfaces as a timeout is caught, and a replugged meter is matched more carefully (AXHK2J)
- Several meters: notices for every tile show, and connecting a second meter keeps an opened PD file (AXHK2K)
- Trigger panel: a charger unplugged with no contract is noticed and the page says so, and the PD analyzer says why Detect stopped its capture (TFA5N7, WFSNZD, JQ971R)
- Charts: a hidden chart no longer hangs the window, dashed series move with their line, a click on the overview strip right after connect counts, and a filling live window keeps its start on the first sample instead of wobbling before it (C6VR9K, SX1VTC, B5XBTA, 9QTPNP)
- Window and layout: a window left on a removed display opens on screen, the tabs, chart options and settings fit at 375, 900 and 1280 px, and the top bar no longer flashes a sideways scrollbar for a frame when its contents change (EWQ6FQ, QBSMHA)
- Tests and CI: the MSRV job covers the desktop crate, the Device select tooltip is tested under Hide serial numbers, and a timing-sensitive worker test holds at a fixed poll (AXHK2H, NKAD6S, DS89PS)

Total test count at release: 5447 tests, 0 failures, 0 errors.

## [0.8.1] — 2026-10-31 — "Loose Ends"

### Fixed
- Trigger panel and console: the panel shows the D+/D- charging hint, a console `entry` line waits long enough to catch its `pdo:` block, a stream error that is not a device loss no longer ends the session while a trigger session opens or closes, and `powerz stream` exits 0 with a note when the meter rejects the stop after gaps (4PJRVH)
- Firmware update: an update can be cancelled until the erase begins, a reloaded page follows the running update instead of offering a second one, a recording on the updating meter asks before it ends, another meter's recording keeps going, and `powerz info` keeps what it read when one block fails (4PJRVJ)
- PD analyzer: a capture keeps its latest 100,000 records and says when it drops the oldest, `pdClear` drops late status samples, a file damaged on its first entry opens empty and names the line, an opened file is released on the next connect, and `pd decode` exits 2 on bad input (4PJRVK)
- Several meters and records: each grid tile has its own rate, Record, Export and Disconnect controls, Record starts at once and asks where to keep the file at Stop, a parked meter keeps its PD capture, a record delete that cannot go ahead is refused before any backup is written, and the app writes `app.log` with no serial or UID (4PJRVM)
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
