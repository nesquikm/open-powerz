---
milestone: M_8M9B7H
id: fr_01M86886H2K8AEJW9ZVR8M9B7H
status: archived
archived_at: 2026-11-24T08:06:20Z
kickoff_branch: fix/old-bootloader-wording
frozen_at: null
shipped_in: v0.14.1
migration: none
codename: Straight Talk
---

# Implementation Plan

## M_8M9B7H — Straight Talk {#M_8M9B7H}

**Goal:** What we say about the old 2.1 bootloader matches what v0.14.0 does, and the workspace API docs build. Ships as v0.14.1.
**Prerequisites:** M_XQPRRR (v0.14.0).
**Status:** FRs written 2026-11-24.

**Scope:**
- Every user-facing "not supported yet" about the old bootloader, in the CLI, the desktop app and the docs.
- Private intra-doc links, and CI documenting the whole workspace.

**Out of scope:** any protocol or behaviour change; the flash path; any real meter.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [RXGEC1](../../frs/archive/RXGEC1.md) | Texts about the old bootloader still say it is not supported yet (bug) | n/a |
| [8VDNQ8](../../frs/archive/8VDNQ8.md) | Workspace API docs do not build because of links to private items (bug) | n/a |

Build order: the table's order.

**Tasks:**
1. New wording in Rust, TypeScript and docs, with the pinning tests — RXGEC1
   verify: `cargo test --workspace --locked` and `bun run test`.
2. Code spans instead of private links; CI documents the workspace — 8VDNQ8
   verify: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`.

**Hardware:** none.

**Gate:** the CLAUDE.md gating rule.
