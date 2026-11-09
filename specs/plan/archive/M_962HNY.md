---
milestone: M_962HNY
id: fr_01M6ZH26N15NXC726QXN962HNY
status: archived
archived_at: 2026-11-09T07:19:31Z
kickoff_branch: fix/web-polish
frozen_at: null
migration: none
codename: Fine Print
shipped_in: v0.11.1
---

# Implementation Plan

## M_962HNY — Web Polish {#M_962HNY}

**Goal:** Fix the two wording and hint issues the web HIL round 4 found, for v0.11.1.
**Prerequisites:** M_QFTCWW (v0.11.0, the web edition).
**Status:** FRs written 2026-11-09 from qa-runs/WEB.hil4-20261109-0946 (W9, W10 minors).

**Scope:**
- The disabled rate picker's tooltip over HID.
- The connect hint in a browser without WebUSB and WebHID.

**Out of scope:** any device command change; the desktop app's behaviour.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [09R2FK](../frs/archive/09R2FK.md) | The rate tooltip over HID blames authentication (bug) | n/a |
| [09R2FM](../frs/archive/09R2FM.md) | The connect hint shows in a browser that cannot connect (bug) | n/a |

Build order: the table's order (independent).

**Tasks:**
1. HID label in the rate picker and a shared HID wording helper — 09R2FK
   verify: `bun run test` (recording and driver suites green).
2. Hide the connect hint without WebUSB and WebHID — 09R2FM
   verify: `bun run test` plus the WebKit Pages spec (`--workers=1`).
