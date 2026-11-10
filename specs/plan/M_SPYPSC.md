---
milestone: M_SPYPSC
id: fr_01M731VQ6REMG9HCGYFTSPYPSC
status: active
archived_at: null
kickoff_branch: docs/v0.12.2
frozen_at: null
migration: none
codename: Fine Print
shipped_in: v0.12.2
---

# Implementation Plan

## M_SPYPSC — Fine Print {#M_SPYPSC}

**Goal:** Pin the docs review's fixes with tests, retake the README screenshot with shown-range stats, and fix what else is stale after v0.12.1, for v0.12.2.
**Prerequisites:** M_NA20AD (v0.12.1).
**Status:** FRs written 2026-11-10 from qa-runs/web/docs13-brief.md.

**Scope:**
- Regression tests for the docs review: README screenshot, links and anchors, CLI transport rules, desktop-only claims.
- A new README desktop screenshot whose stats match the zoomed chart.
- A read-through of the README and docs for claims still stale after v0.12.1.

**Out of scope:** code changes beyond tests; released changelog sections; any meter command.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [SPYPSD](../frs/archive/SPYPSD.md) | No test keeps the docs review's fixes true (bug) | n/a |
| [PFYFR6](../frs/archive/PFYFR6.md) | The README screenshot's stats disagree with its zoomed chart (bug) | n/a |
| [AMJB0K](../frs/archive/AMJB0K.md) | A few docs still lag behind v0.12.1 (bug) | n/a |

Build order: the table's order.

**Tasks:**
1. Docs tests for the screenshot, links, transport rules and desktop-only claims; fix what they catch — SPYPSD
   verify: `bun run test` and `cargo test -p powerz-cli`.
2. Retake the README desktop screenshot with Stats on Shown range — PFYFR6
   verify: the README screenshot test passes; the image is checked by eye.
3. Read-through fixes — AMJB0K
   verify: `bun run test`.

**Gate:** the CLAUDE.md gating rule.
