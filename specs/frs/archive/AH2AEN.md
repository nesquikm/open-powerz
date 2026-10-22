---
title: Desktop end-to-end tests
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5DDP0TWKNS6QSV917AH2AEN
created_at: 2026-10-20T20:07:53.038Z
changelog_category: Added
---

# Desktop end-to-end tests

## Summary

Automated tests drive the real desktop app against the demo device, from connecting to exporting.
They run on Linux and Windows, where the WebDriver tooling for Tauri is available.
A second suite runs the interface in a browser against a fake backend on all three systems.
That suite also checks keyboard use and accessibility in both themes.
With a meter attached, an opt-in run checks the same flow on real hardware.

## Requirement

Add two end-to-end suites for the desktop app — WebdriverIO with tauri-driver against the real app, and Playwright against the web UI with a mocked backend — and run both in CI.

## Acceptance Criteria

- AC-AH2AEN.1: The real-app suite launches the built app, selects "Demo device — load-5v-1a", connects, and finds VBUS within 5% of 5 V and IBUS within 5% of 1 A in the readout.
- AC-AH2AEN.2: In the same run the chart has drawn data for VBUS and IBUS (read through a test hook compiled only into the `e2e` build) and the stats elapsed time advances.
- AC-AH2AEN.3: The suite records 5 s at 50 SPS into a temporary directory (the `e2e` build skips the save dialog), stops, exports the session, and checks that both CSV files exist with the vendor header and row counts within 5% of rate × duration.
- AC-AH2AEN.4: The real-app suite runs in CI on ubuntu-24.04 (under xvfb, with the WebKitGTK WebDriver) and windows-latest (with the Edge WebDriver matching WebView2); macOS is skipped, and testing-spec records why.
- AC-AH2AEN.5: The UI suite runs the production web build in Chromium and WebKit against a scripted mock backend and covers the picker, connect, live values, the error banner with the Linux permission hint, the rate-locked notice, every shortcut and the theme toggle.
- AC-AH2AEN.6: The UI suite runs an axe accessibility scan on the main view disconnected and streaming, in both themes, at 900 px and 1600 px wide, and fails on any serious or critical violation.
- AC-AH2AEN.7: The UI suite runs in CI on ubuntu-24.04, macos-latest and windows-latest; both suites run in a separate CI job that must pass for a pull request to be green.
- AC-AH2AEN.8: A hardware-in-the-loop backend test, run only with `POWERZ_HIL=1` and never in CI, drives the same session worker the app uses (FR VHY378) against the attached KM003C over bulk: it connects, checks that the first batch is sane with nothing plugged into the meter (VBUS below 0.5 V; D+, D−, CC1 and CC2 between 0 and 5 V), then streams 30 s at 50 SPS and 30 s at 1000 SPS with zero gaps and a record count within 2% of rate × duration.
- AC-AH2AEN.9: In the same hardware run, a 10 s recording and a session export are written to a temporary directory; both start with the vendor header, and re-rendering their parsed records through the `powerz-record` writer that `powerz stream` uses reproduces each file byte for byte.
- AC-AH2AEN.10: Unplug and replug on the real meter are a manual check in the pull-request checklist: unplugging mid-stream shows `disconnected` within 2 s with the samples kept, and after replugging the meter is listed again within 2 s and reconnecting appends to the session with a gap event.
- AC-AH2AEN.11: On macOS, where tauri-driver is unavailable, the real app connected to the real KM003C is run by hand for the blocking visual-check screenshots (light and dark, ~900 and ~1600 px wide) next to the demo-device ones; testing-spec records that the real-device app e2e is manual on macOS.

## Technical Design

`apps/desktop/e2e/app/` holds the WebdriverIO config with the `tauri-driver` service and specs; the app is built with an `e2e` Cargo feature that exposes chart series lengths on `window` and reads a save directory from an environment variable. `apps/desktop/e2e/ui/` holds the Playwright config: its web server runs `vite preview` on a build made with `VITE_BACKEND=mock`, which swaps `backend.ts` for `mockBackend.ts`, a scripted backend driven by the simulator's scenario shapes. Scans use `@axe-core/playwright`. Workspace scripts: `e2e:app` and `e2e:ui`.

The same `window.__OPEN_POWERZ_E2E__` test hook (`apps/desktop/e2e/hook.ts`) is also installed in the mocked UI build (`VITE_BACKEND=mock`), so both suites read chart state the same way; a normal production build has none.

## Testing

The suites are the tests. CI runs them in a new `e2e` job matrix beside the existing gate job. The hardware test lives in the desktop crate's `tests/hil.rs`, gated like the CLI's.

## Notes

tauri-driver does not support macOS, so the real-device app run there is manual.

2026-10-21: QA shot the real app, light/dark, ~900/~1600 px (.11), kept outside the repo.

2026-10-22, build 55c7b88 (.10): unplug passed (recording closed within 1 s). Replug fell back to the demo device; the meter needed a manual reconnect. Gap band unverified; M3 bug.
