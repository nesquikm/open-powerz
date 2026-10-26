---
milestone: M_WBBQ7F
id: fr_01M5WNDAHGTBR6V716Q5WBBQ7F
status: archived
archived_at: 2026-10-26T21:32:10Z
kickoff_branch: null
frozen_at: null
migration: none
codename: Special Delivery
shipped_in: v0.6.0
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
| [E1ZJ2M](../frs/archive/E1ZJ2M.md) | Pinned Rust toolchain | n/a |
| [E1ZJ2N](../frs/archive/E1ZJ2N.md) | Release builds on a version tag | n/a |

Build order: E1ZJ2M, then E1ZJ2N.

**Tasks:**
- [x] Pinned Rust toolchain — E1ZJ2M
- [x] Release builds on a version tag — E1ZJ2N (AC-E1ZJ2N.11 dry run + smoke test verified on the PR)

**M7 follow-ups:**
- Toolchain-pin scan: an unpaired apostrophe in a run line keeps a trailing comment, so the line can be flagged falsely (fails safe; from E1ZJ2M review).
- CLAUDE.md Tech Stack names only the MSRV (1.96); add the rust-toolchain.toml pin there (needs the user's OK to edit CLAUDE.md).
- Release publish step: switch the edit, upload and delete-asset calls from the tag to the release id (`gh api`), so draft handling never depends on the gh version (from E1ZJ2N review).
- Changelog parser: an unquoted codename branch also accepts a malformed date as a codename; tighten the heading pattern (cosmetic; from E1ZJ2N review).
- Release AppImage loses its executable bit through the workflow artifact; the install guide says `chmod +x` (consider packing it, or noting it in the release notes).

**Gate:** same gating rule as CLAUDE.md.
