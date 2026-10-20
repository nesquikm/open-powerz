---
title: Desktop end-to-end tests
milestone: M_R4WG4Q
status: active
archived_at: null
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

## Technical Design

`apps/desktop/e2e/app/` holds the WebdriverIO config with the `tauri-driver` service and specs; the app is built with an `e2e` Cargo feature that exposes chart series lengths on `window` and reads a save directory from an environment variable. `apps/desktop/e2e/ui/` holds the Playwright config: its web server runs `vite preview` on a build made with `VITE_BACKEND=mock`, which swaps `backend.ts` for `mockBackend.ts`, a scripted backend driven by the simulator's scenario shapes. Scans use `@axe-core/playwright`. Workspace scripts: `e2e:app` and `e2e:ui`.

## Testing

The suites are the tests. CI runs them in a new `e2e` job matrix beside the existing gate job.

## Notes

tauri-driver does not support macOS, so the real app is not driven there; the mocked UI suite still runs on macOS.
