# Testing Specification

## 1. Test Framework
- **Runner:** Rust built-in harness via `cargo test --workspace --locked`; Vitest 5 (`bun run test`) for TypeScript
- **Mocking:** hand-written fake transports implementing the protocol crate's transport trait; no mocking crate. Vitest `vi.fn()` / `vi.mock()` for the Tauri `invoke` boundary in UI logic tests
- **Coverage:** `cargo llvm-cov` for Rust (when coverage is wired into CI); Vitest v8 coverage for TS

## 2. Test Structure

**Layout policy: `src/`-co-located** — tests live next to the source they exercise (toolkit default).

- **Rust:** unit tests in a `#[cfg(test)] mod tests` at the bottom of each module; cross-module
  tests in `crates/<crate>/tests/*.rs`; byte-vector fixtures in `crates/powerz-protocol/tests/vectors/`.
- **TypeScript:** each `apps/desktop/src/lib/<module>.ts` has a sibling `<module>.test.ts`.

```
crates/powerz-protocol/src/
└── lib.rs              # + #[cfg(test)] mod tests (one per module as modules land)
apps/desktop/src/lib/
├── backend.ts, mockBackend.ts, session.svelte.ts, format.ts, theme.ts, shortcuts.ts, … (+ *.test.ts)
└── components/          # Svelte components (covered by the Playwright UI suite)
```

Planned: a `tests` directory inside the protocol crate holding integration tests and a
`vectors` folder of our own hex fixture files rewritten from device logs.

## 3. Conventions

### Naming
- Rust: test fns named for the behavior (`parses_adc_section_of_44_bytes`); files `*.rs` under `tests/`
- TS: `*.test.ts`; `describe` per function, `it` states the expected behavior

### What to Test
- Every decoder and frame builder against byte vectors from the vendor docs' worked examples and our own device captures (tag each vector with its source: `[doc]`, `[device]`)
- Malformed input: truncated sections, unknown attributes, wrong lengths (must return errors, never panic)
- Session logic (one request in flight, timeouts, reconnect) with fake transports
- Unit conversion and formatting in the UI layer

### What NOT to Test
- Tauri/WebView internals, third-party crates, generated files under `src-tauri/gen/`
- Real hardware in CI. Device tests are manual, run through the CLI, and their logs become fixtures

### End-to-end and hardware-in-the-loop
- **Simulator e2e (in the gate):** the `powerz` binary is run with `assert_cmd` against the
  `powerz-sim` demo device on Linux, macOS and Windows; human-readable output is
  snapshot-tested with `insta`.
- **HIL (opt-in):** the same flows against a real meter run only with `POWERZ_HIL=1`, using
  only the safe command set in `docs/protocol.md`; CI never sets it.
- **Desktop real-app e2e:** WebdriverIO + `tauri-driver` drives the built app (with the `e2e`
  Cargo feature: test hooks, no save dialog) against the demo device — connect, live values,
  chart, record, export, CSV checks. Runs in CI in the `e2e-app` job on ubuntu-24.04 (xvfb,
  WebKitGTK WebDriver) and windows-latest (Edge WebDriver matching the installed WebView2).
  **Not on macOS:** `tauri-driver` has no macOS support, because WKWebView offers no
  WebDriver.
- **`e2e` Cargo feature (in the gate):** the `gate` job on all three OSes also runs
  `cargo clippy -p open-powerz-desktop --all-targets --features e2e --locked -- -D warnings` and
  `cargo test -p open-powerz-desktop --features e2e --locked --lib --test e2e_save`, so the
  e2e-only code is linted and its save-path tests run even where `e2e-app` does not (macOS).
  The e2e app build writes its web UI to `apps/desktop/dist-e2e-app/` (never `dist/`); on a
  failure the `e2e-app` job uploads the WebdriverIO and `tauri-driver` logs
  (`apps/desktop/e2e-app-logs/`).
- **Desktop UI e2e:** Playwright runs the web build (`VITE_BACKEND=mock`, a scripted backend)
  in Chromium and WebKit on all three OSes: UI flows, shortcuts, focus order, and an axe scan
  (no serious or critical violations) in both themes at 900 and 1600 px. Runs in CI in the
  `e2e-ui` job on ubuntu-24.04, macos-latest and windows-latest with Playwright's bundled
  browsers (`bunx playwright install [--with-deps] chromium webkit`, never `channel: chrome`);
  the HTML report and traces are uploaded as an artifact when it fails.
- **Required checks:** `e2e-app` and `e2e-ui` are separate from the `gate` job and have no
  `continue-on-error`, so a failure turns the pull request red. Branch protection on `main`
  must list every `gate (*)`, `e2e-app (*)` and `e2e-ui (*)` check as required; that is a repo
  setting, not something the workflow file can enforce.
- **Desktop HIL:** with `POWERZ_HIL=1`, a backend test drives the app's session worker against
  a real meter (connect, sane values, 50 and 1000 SPS with zero gaps, record, export, CSV byte
  parity with `powerz stream`); never in CI. Unplug/replug on hardware is a manual checklist
  item in `.github/pull_request_template.md` ("Manual hardware checks").
- **macOS real-device app e2e (manual):** with no `tauri-driver` on macOS, the real app is run
  by hand (`bun run dev`, or the debug build from `bun run --cwd apps/desktop tauri build --debug --no-bundle`)
  against an attached KM003C, safe command set only: connect, live values, 1000 SPS with
  temperature "—" and its tooltip, record and export, light and dark at ~900 and ~1600 px.
  The screenshots are listed in the PR / milestone report next to the demo-device ones.
- **Visual checks (blocking):** `/dev-process-toolkit:visual-check` screenshots for every UI
  FR — light and dark, ~900 and ~1600 px wide, demo device streaming and no device — read
  and fixed before the FR is done.
- **Future (Linux CI):** expose the simulator as a real USB device (dummy_hcd + FunctionFS
  gadget, or USB/IP) so the nusb bulk path is tested end to end.

## 4. Coverage Targets
| Layer   | Target | Minimum |
|---------|--------|---------|
| `powerz-protocol` decode/framing | >=90%  | 80%     |
| Desktop backend (batch, history, stats, recording) | >=85% | 75% |
| Overall | >=75%  | 65%     |

## 5. Test Data
Byte-vector fixtures written in our own files (hex text plus a comment naming the source and
firmware version). Serial numbers and UIDs in device captures are redacted before they are committed.
