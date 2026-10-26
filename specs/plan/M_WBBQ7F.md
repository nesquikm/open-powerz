---
milestone: M_WBBQ7F
id: fr_01M5WNDAHGTBR6V716Q5WBBQ7F
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Special Delivery
---

# Implementation Plan

## M_WBBQ7F — Release Pipeline {#M_WBBQ7F}

**Goal:** A pushed version tag builds unsigned desktop installers and CLI binaries for macOS, Windows and Linux and publishes a GitHub Release with the changelog notes; every build uses one pinned Rust toolchain.
**Prerequisites:** M_MJ7KE9 (v0.5.0); the existing CI workflow.
**Status:** FRs written 2026-10-26; ready to implement. User decision 2026-10-26: build this right after M5 and before M6, unsigned for now.

**Scope:**
- rust-toolchain.toml pin used by local builds, CI and the release workflow.
- Tag-triggered release workflow: version and changelog check, tauri-action desktop bundles (macOS universal .dmg, Windows .msi/.exe, Linux AppImage/.deb), CLI archives, SHA256SUMS, draft-then-publish GitHub Release, dry run on PRs, manual run for an existing tag.
- Install guide for unsigned builds.

**Out of scope:** code signing and notarization, auto-update, Linux arm64 and Windows arm64 builds, package-manager feeds (Homebrew, winget), the GitHub Pages deploy (web edition milestone).

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [E1ZJ2M](../frs/E1ZJ2M.md) | Pinned Rust toolchain | n/a |
| [E1ZJ2N](../frs/E1ZJ2N.md) | Release builds on a version tag | n/a |

Build order: E1ZJ2M, then E1ZJ2N.

**Tasks:**
- [ ] Pinned Rust toolchain — E1ZJ2M
- [ ] Release builds on a version tag — E1ZJ2N

**M7 follow-ups:**
- (none yet)

**Gate:** same gating rule as CLAUDE.md.
