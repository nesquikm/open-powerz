---
milestone: M_SPYPSC
id: fr_01M731VQ6REMG9HCGYFTSPYPSC
status: active
archived_at: null
kickoff_branch: docs/v0.12.2
frozen_at: null
migration: none
codename: Fine Print
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
- The overview strip under the File tab's chart, shown in the README screenshot.

**Out of scope:** code changes beyond tests and the File tab strip; released changelog sections; any meter command.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [SPYPSD](../frs/archive/SPYPSD.md) | No test keeps the docs review's fixes true (bug) | n/a |
| [PFYFR6](../frs/archive/PFYFR6.md) | The README screenshot's stats disagree with its zoomed chart (bug) | n/a |
| [AMJB0K](../frs/archive/AMJB0K.md) | A few docs still lag behind v0.12.1 (bug) | n/a |
| [09H204](../frs/09H204.md) | The File tab has no overview strip | n/a |

Build order: the table's order.

**Tasks:**
1. Docs tests for the screenshot, links, transport rules and desktop-only claims; fix what they catch — SPYPSD
   verify: `bun run test` and `cargo test -p powerz-cli`.
2. Retake the README desktop screenshot with Stats on Shown range — PFYFR6
   verify: the README screenshot test passes; the image is checked by eye.
3. Read-through fixes — AMJB0K
   verify: `bun run test`.
4. The overview strip under the File tab's chart, then retake the README screenshot with it — 09H204
   verify: `bun run test` and the recording-file Playwright spec on WebKit.

**Gate:** the CLAUDE.md gating rule.
