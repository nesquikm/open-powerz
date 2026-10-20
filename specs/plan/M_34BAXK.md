---
milestone: M_34BAXK
id: fr_01M5CXT39J3QY24WCCV734BAXK
kind: scaffolding
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: null
---

# Implementation Plan

## M_34BAXK — Foundation / Scaffolding {#M_34BAXK}

**Goal:** A gate-check-ready workspace: Rust protocol library + CLI + Tauri 2 desktop app with a Svelte 5 UI, building on macOS, Linux and Windows.
**Prerequisites:** None

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| <scaffolding> | Bootstrap (cargo workspace, Bun workspace, Tauri 2 + Svelte 5 shell, CI) | n/a |

**Tasks:**
- [x] Cargo workspace with `crates/powerz-protocol`, `crates/powerz-cli`, `apps/desktop/src-tauri`
  verify: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [x] Svelte 5 + Vite 8 + TS web UI with Vitest, ESLint, Prettier, svelte-check
  verify: `bun run lint && bun run check && bun run test && bun run build:web`
- [x] GitHub Actions gate on ubuntu-24.04, macos-latest, windows-latest
  verify: `.github/workflows/ci.yml` runs the gating rule (green on first push)

**Tests:**
- `powerz-protocol`: VID constant; desktop backend: `app_info` command; UI: `formatVolts`

**Acceptance Criteria:**
- [x] Gating rule from CLAUDE.md passes locally on macOS
- [ ] CI is green on all three OSes after the first push

**Gate:** `bun install --frozen-lockfile && bun run lint && bun run check && bun run test && bun run build:web && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
