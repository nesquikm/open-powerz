---
milestone: M_ZFZQE7
id: fr_01M6ZX9Y7XV14TCPQWZ6ZFZQE7
status: archived
archived_at: 2026-11-10T07:07:16Z
kickoff_branch: feat/web-parity
frozen_at: null
migration: none
codename: Full Tab
shipped_in: v0.12.0
---

# Implementation Plan

## M_ZFZQE7 — Web Parity {#M_ZFZQE7}

**Goal:** Bring the web edition's "Desktop app only" features to the browser, reusing the protocol core through WebAssembly: settings and records (read), then settings change, record delete, startup logo and protocol triggering, with the desktop app's safeguards.
**Prerequisites:** M_962HNY (v0.11.1).
**Status:** FRs written 2026-11-09 from the web parity brief.

**Scope:**
- Phase A (read only, WebUSB): settings panel, records list, record download.
- Phase B (meter writes): settings change over Web Serial, record delete, startup logo over WebUSB, protocol triggering over Web Serial.
- The HID rate-key bug and the release workflow's bundler download failures.
- Docs.

**Out of scope (stays desktop only):** firmware update (an interrupted update in a browser tab could brick the meter) and showing a file in the file manager. WebHID sessions keep their limits (ADC snapshots only).

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [W8SHX8](../../frs/archive/W8SHX8.md) | Meter settings and offline records in the browser | n/a |
| [W8SHXD](../../frs/archive/W8SHXD.md) | Rate keys over HID try to change the rate (bug) | n/a |
| [W8SHXE](../../frs/archive/W8SHXE.md) | Release builds fail when the bundler tooling download times out (bug) | n/a |
| [W8SHX9](../../frs/archive/W8SHX9.md) | Change meter settings in the browser | n/a |
| [W8SHXA](../../frs/archive/W8SHXA.md) | Delete offline records in the browser | n/a |
| [W8SHXB](../../frs/archive/W8SHXB.md) | Startup logo upload in the browser | n/a |
| [W8SHXC](../../frs/archive/W8SHXC.md) | Protocol triggering in the browser | n/a |
| [W8SHXF](../../frs/archive/W8SHXF.md) | Web edition docs for the new browser features | n/a |
| [60QB2S](../../frs/archive/60QB2S.md) | An (i) beside the web device menu explains the connections | n/a |
| [K4DWCF](../../frs/archive/K4DWCF.md) | A confirmed request in the browser never reaches the trigger engine (bug) | n/a |
| [CJ3QC9](../../frs/archive/CJ3QC9.md) | A settings change in the browser misses another program on the serial port (bug) | n/a |
| [CJ3QCA](../../frs/archive/CJ3QCA.md) | A settings change in the browser reports a lost reply although it landed (bug) | n/a |
| [CJ3QCB](../../frs/archive/CJ3QCB.md) | Refreshing the record list in the browser misses a new record (bug) | n/a |
| [CJ3QCC](../../frs/archive/CJ3QCC.md) | The startup logo tab over HID gives the wrong reason (bug) | n/a |
| [CJ3QCD](../../frs/archive/CJ3QCD.md) | Small stale and mismatched readings in the browser's meter views (bug) | n/a |
| [AHTZW1](../../frs/archive/AHTZW1.md) | The first PD request after Detect sometimes times out in the browser (bug) | n/a |
| [AHTZW4](../../frs/archive/AHTZW4.md) | A request after a failed PD entry goes out against old offers (bug) | n/a |
| [AHTZW2](../../frs/archive/AHTZW2.md) | Export .db does nothing in the browser (bug) | n/a |
| [AHTZW3](../../frs/archive/AHTZW3.md) | Open a saved recording in the browser | n/a |
| [62VEDM](../../frs/archive/62VEDM.md) | Some abbreviations have no glossary popover (bug) | n/a |
| [JFRPME](../../frs/archive/JFRPME.md) | The browser's session history forgets the temperature (bug) | n/a |
| [EAKFEC](../../frs/archive/EAKFEC.md) | Open a PD capture and import a vendor PD log in the browser | n/a |
| [X3H3JY](../../frs/archive/X3H3JY.md) | A repeated settings change in the browser fails although it landed (bug) | n/a |
| [Z99166](../../frs/archive/Z99166.md) | Three small web edition snags from the parity HIL (bug) | n/a |

Build order: the table's order. Triggering goes last of the features: it moves the desktop trigger engine into a shared crate and needs a cross-origin-isolated page.

**Tasks:**
1. Settings and record read exports in powerz-wasm; driver and session commands; features on — W8SHX8
   verify: `cargo test -p powerz-wasm` parity tests; `bun run test` (web suites).
2. HID flag in the rate-change decision — W8SHXD
   verify: `bun run test` (recording suite).
3. Bundler tool cache and retry in the release workflow — W8SHXE
   verify: `bun run test` (workflow checks).
4. Web Serial transport and settings write — W8SHX9
   verify: wasm parity tests; Vitest over a fake serial port.
5. Record delete — W8SHXA
   verify: wasm parity tests; Vitest over the demo meter.
6. Startup logo — W8SHXB
   verify: wasm parity tests; Vitest over the demo meter.
7. Trigger engine crate, wasm worker, serial bridge, isolation service worker — W8SHXC
   verify: `cargo test --workspace`; Vitest bridge and backend suites; Pages e2e (`--workers=1`).
8. Docs — W8SHXF
   verify: docs Vitest checks.
9. Connection info button beside the web device menu — 60QB2S
   verify: `bun run test` (device picker suites); Pages e2e connection-info check (`--workers=1`).
10. Plain copies of the trigger RPC arguments — K4DWCF
   verify: `bun run test` (trigger bridge and web trigger suites).
11. Foreign-reply check on the settings pipe — CJ3QC9
   verify: `bun run test` (settings pipe, driver and backend settings suites).
12. Skip earlier replies on the settings pipe, read-back decides — CJ3QCA
   verify: `bun run test` (driver settings suite).
13. Fresh sign-in before every record list — CJ3QCB
   verify: `bun run test` (driver and backend record suites).
14. HID reason, model token and session reset on the logo tab — CJ3QCC
   verify: `bun run test` (logo panel suites).
15. Logo read cleared and with progress, rate picker resync, record totals from the list — CJ3QCD
   verify: `bun run test` (logo, records and rate picker suites).
16. PD re-entry retried once before a request; trigger log to the page console — AHTZW1
   verify: `cargo test -p powerz-trigger`; `bun run test` (trigger worker and backend suites).
17. PD ready only after fresh offers; requests checked against the latest negotiation — AHTZW4
   verify: `cargo test -p powerz-protocol -p powerz-trigger`.
18. Shared .db builders in powerz-record; browser .db export through sql.js — AHTZW2
   verify: `cargo test -p powerz-record -p open-powerz-desktop`; `bun run test` (session .db export suite).
19. Shared file-reading rules; Open recording in the browser (CSV via wasm, .db via sql.js) — AHTZW3
   verify: `cargo test -p powerz-record -p open-powerz-desktop`; `bun run test` (recording file suites).
20. Temperature log beside the web history; session exports fill TEMP — JFRPME
   verify: `bun run test` (session export suites).
21. Shared PD capture and vendor log rules in powerz-protocol; PD Open capture and Import vendor log in the browser (pdcap via wasm, .sqlite via sql.js); desktop-only sweep — EAKFEC
   verify: `cargo test -p powerz-protocol -p powerz-wasm -p open-powerz-desktop`; `bun run test` (PD open suites).
22. Settings pipe reads the serial port through the CDC demux; SETTINGS re-read on a bad block; own-open port reopened — X3H3JY
   verify: `cargo test -p powerz-wasm`; `bun run test` (settings pipe, serial and driver settings suites).
23. HID logo Read disabled; TEMP judged on arrival; a new pick supersedes a pending one — Z99166
   verify: `bun run test` (logo, session export and PD file suites).

**Gate:** CLAUDE.md gating rule, with `bun run build:wasm` before `bun run test`.

**Device rules:** the developer does not touch the meter. Writes (settings, record delete, logo) and triggering above 5 V run on the real meter only in the HIL tester's tab, each first run with the user's OK; triggering above 5 V only in the wall-charger setup. Firmware update stays out of the browser.

**Known follow-ups (not fixed in this milestone):**
- AHTZW3: Cancel can't interrupt a big parse in Open recording.
- AHTZW4: an unasked `pdo:` block isn't refreshed after a failed PD entry (safe: a request is still checked against the latest offers).
- The `RecordingRows` type lacks `temp_c`.
- Up to ~20 empty TEMP rows in the first 0.4 s after a HID↔USB reconnect (qa-runs/WEB.parity-hil4-20261110-1037, P14).
- The CLI `powerz --transport cdc watch` exits by itself while the browser streams over WebUSB (poll source: `unexpected reply to GET_DATA`; `--source cdc`: after ≈25–40 s).
- Not verified on hardware: Import vendor log (AC-EAKFEC.3), no vendor PD .sqlite at hand.
- The desktop README screenshot (`docs/images/desktop.png`) waits for a new capture.
