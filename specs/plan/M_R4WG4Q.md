---
milestone: M_R4WG4Q
id: fr_01M5CYB12PNSTXG28YNTR4WG4Q
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Live Wire
---

# Implementation Plan

## M_R4WG4Q — Desktop Meter {#M_R4WG4Q}

**Goal:** The Tauri desktop app becomes a polished live meter and recorder on macOS, Linux and Windows: a device picker with the demo device, a big live readout, a fast chart over the whole session, statistics, and CSV recording and export.
**Prerequisites:** M_34BAXK (protocol core and CLI, shipped in v0.1.0).

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [0P7HSJ](../frs/0P7HSJ.md) | Recording and CSV export | n/a |
| [VHY378](../frs/VHY378.md) | Desktop device backend: picker, session worker and live streaming | n/a |
| [AH2AEJ](../frs/AH2AEJ.md) | Session history, decimation and statistics | n/a |
| [AH2AEK](../frs/AH2AEK.md) | Desktop shell, live readout, themes and accessibility | n/a |
| [AH2AEM](../frs/AH2AEM.md) | Live chart | n/a |
| [AH2AEN](../frs/AH2AEN.md) | Desktop end-to-end tests | n/a |
| [272WSN](../frs/272WSN.md) | Whole-session view: session time, gaps and axes | n/a |
| [DDDYDJ](../frs/DDDYDJ.md) | Live and paused chart states | n/a |
| [FENCD9](../frs/FENCD9.md) | Timeline navigation | n/a |
| [VEDDFD](../frs/VEDDFD.md) | Resync the UI with a running session on load | n/a |

**Tasks:**
- [x] Extract `crates/powerz-record` from the CLI — 0P7HSJ (the `--include-demo` help text needed no fix)
  verify: `cargo test -p powerz-cli -p powerz-record --locked` passes with the CLI snapshots unchanged
- [x] Device manager, session worker, batcher and error mapping in the desktop crate — VHY378
  verify: worker integration tests against the simulator pass (batch count, ordering, fallback, disconnect, safe frames)
- [x] Session history with min/max pyramid and statistics — AH2AEJ
  verify: property tests and the release-mode 10M-sample timing test pass
- [x] Recording to file, session export, rate picker backend — 0P7HSJ
  verify: golden-file test matches `powerz stream` output for the same records
- [x] UI shell: layout, picker, readout, error banner, themes, shortcuts, About — AH2AEK
  verify: Vitest passes; visual-check screenshots reviewed (light/dark × ~900/~1600 px × demo streaming/no device)
- [x] Stats panel and recording controls in the UI — AH2AEJ, 0P7HSJ
  verify: Vitest passes; visual-check screenshots of the panel reviewed in both themes
- [x] uPlot chart with live window, zoom/pan via `history_range`, series toggles, hover readout — AH2AEM
  verify: Vitest passes; visual-check screenshots of live and zoomed views reviewed
- [x] WebdriverIO + tauri-driver real-app suite, Playwright + axe UI suite, CI `e2e` job — AH2AEN
  verify: `bun run e2e:ui` passes locally; CI `e2e` job green on its matrix
- [x] Desktop HIL backend test (`POWERZ_HIL=1`) against the KM003C — AH2AEN
  verify: `POWERZ_HIL=1 cargo test -p open-powerz-desktop --test hil` passes on the attached meter
- [x] Session-relative time, history gaps between runs, gap shading, axis width — 272WSN
  verify: `bun run test` and `cargo test --workspace --locked` pass; `bun run e2e:ui` passes `session.spec.ts`
- [x] Live/paused button, End to go live — DDDYDJ
  verify: `bun run test` passes; `bun run e2e:ui` passes `live.spec.ts`
- [x] Timeline model, gap collapse, overview strip, playhead, shuttle, zoom slider, presets — FENCD9
  verify: `bun run test` passes; `bun run e2e:ui` passes the timeline specs incl. the 10M-sample timings
- [ ] Resync with a running session on load: backend attach + swappable sink, UI rebuild, mock — VEDDFD
  verify: `cargo test --workspace --locked` and `bun run test` pass; `bun run e2e:ui` passes the reload specs
- [ ] Real-device run of the desktop app on macOS against the KM003C (safe command set only), incl. unplug/replug — AH2AEN, AH2AEK
  verify: real-device visual-check screenshots (light/dark × ~900/~1600 px, incl. 1000 SPS "—" temperature) and a recorded CSV listed in the report

**Tests:**
- Rust unit and property tests for batching, decimation, stats and recording; worker integration tests against `powerz-sim`; Vitest for UI logic; WebdriverIO real-app e2e (Linux, Windows); Playwright UI e2e with axe (Linux, macOS, Windows).

**Acceptance Criteria:**
- [x] Gating rule from CLAUDE.md passes locally on macOS
- [x] CI is green on all three OSes, including the `e2e` job
- [ ] Every AC of the ten FRs above is traced to a test by `/spec-review`
- [x] Blocking visual check passed for every UI FR: light and dark, ~900 px and ~1600 px wide, demo device streaming and no device; screenshot paths listed in the report
- [ ] The desktop app streams, charts and records from the real KM003C at 1000 SPS for 60 s with no host-side gaps

**Gate:** `bun install --frozen-lockfile && bun run lint && bun run check && bun run test && bun run build:web && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
