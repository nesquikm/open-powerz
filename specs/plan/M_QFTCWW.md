---
milestone: M_QFTCWW
id: fr_01M6VHDG2BG62PZDW7K4QFTCWW
status: active
archived_at: null
kickoff_branch: feat/web
frozen_at: null
migration: none
codename: Open Tab
---

# Implementation Plan

## M_QFTCWW — Web Edition {#M_QFTCWW}

**Goal:** A browser edition of open-powerz on GitHub Pages (Chrome and Edge): the protocol core as WebAssembly, WebUSB / WebHID transports, the existing Svelte UI over a web backend, and the demo meter, so the page is useful with no meter. Read and stream only.
**Prerequisites:** M_PVFHM6 (v0.10.0, desktop feature-complete).
**Status:** FRs written 2026-11-07; ready to implement on `feat/web`.

**Scope:**
- Protocol core, PD decoder and simulator build for wasm32-unknown-unknown; native transports behind cargo features; CI builds the target.
- `powerz-wasm` binding (wasm-bindgen): sans-IO request builders, reply decoders and the demo meter.
- TypeScript WebUSB (vendor bulk by class) and WebHID transports plus a session driver with a closed, read-only command set.
- Web backend behind the `backend.ts` seam: device picker, live readings and chart, stats/history, passive PD analyzer, CSV recording download, demo meter.
- Static Pages build under `/open-powerz/`, favicon, not-supported page, Pages deploy job on `v*` tags and a manual dispatch for main.
- README section and a browser-support docs page.

**Out of scope here (desktop app only in the web UI):** firmware update, logo upload, settings writes, record delete, protocol triggering, Web Serial (CDC ADCNEW stream).

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [QFTCWX](../frs/QFTCWX.md) | Protocol core builds for the browser | n/a |
| [QFTCWY](../frs/QFTCWY.md) | WebAssembly binding for the protocol core | n/a |
| [QFTCWZ](../frs/QFTCWZ.md) | Browser transports and session driver | n/a |
| [QFTCX0](../frs/QFTCX0.md) | Web backend for the desktop UI | n/a |
| [QFTCX1](../frs/QFTCX1.md) | Static site and GitHub Pages deploy | n/a |
| [QFTCX2](../frs/QFTCX2.md) | Web edition docs | n/a |

Build order: QFTCWX → QFTCWY → QFTCWZ → QFTCX0 → QFTCX1 → QFTCX2.

**Tasks:**
1. Feature-gate native transports; sim manual clock; toolchain target; CI wasm step.
   verify: `cargo build -p powerz-protocol --no-default-features --target wasm32-unknown-unknown --locked` and workspace tests green.
2. `powerz-wasm` crate and `build:wasm` script.
   verify: parity tests green; Vitest smoke over the built module.
3. Transports and driver in `apps/desktop/src/lib/web/`.
   verify: Vitest driver suite over the demo transport and fake devices.
4. Web backend and desktop-only gating.
   verify: Vitest web-backend suite; mock and Tauri suites unchanged.
5. Pages build, not-supported view, Playwright on the built site, workflows.
   verify: `bun run build:pages` + Playwright (`--workers=1`).
6. Docs.
   verify: docs Vitest checks.

**Gate:** CLAUDE.md gating rule, with `bun run build:wasm` before `bun run test`; plus the wasm32 builds in CI.

**Device rules:** read and stream only (CONNECT, auth, GET_DATA, queue start/stop, PD capture, safe memory reads). No writes. The real meter is checked later by the HIL tester in the user's Chrome.

**Needs enabling on GitHub (not done by this milestone):** Settings → Pages → Source: GitHub Actions; the `github-pages` environment allows `v*` tags and main.
