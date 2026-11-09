---
milestone: M_ZFZQE7
id: fr_01M6ZX9Y7XV14TCPQWZ6ZFZQE7
status: active
archived_at: null
kickoff_branch: feat/web-parity
frozen_at: null
migration: none
codename: Full Tab
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
| [W8SHX8](../frs/W8SHX8.md) | Meter settings and offline records in the browser | n/a |
| [W8SHXD](../frs/W8SHXD.md) | Rate keys over HID try to change the rate (bug) | n/a |
| [W8SHXE](../frs/W8SHXE.md) | Release builds fail when the bundler tooling download times out (bug) | n/a |
| [W8SHX9](../frs/W8SHX9.md) | Change meter settings in the browser | n/a |
| [W8SHXA](../frs/W8SHXA.md) | Delete offline records in the browser | n/a |
| [W8SHXB](../frs/W8SHXB.md) | Startup logo upload in the browser | n/a |
| [W8SHXC](../frs/W8SHXC.md) | Protocol triggering in the browser | n/a |
| [W8SHXF](../frs/W8SHXF.md) | Web edition docs for the new browser features | n/a |

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

**Gate:** CLAUDE.md gating rule, with `bun run build:wasm` before `bun run test`.

**Device rules:** the developer does not touch the meter. Writes (settings, record delete, logo) and triggering above 5 V run on the real meter only in the HIL tester's tab, each first run with the user's OK; triggering above 5 V only in the wall-charger setup. Firmware update stays out of the browser.
