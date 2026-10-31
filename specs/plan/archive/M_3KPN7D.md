---
milestone: M_3KPN7D
id: fr_01M65KVDYBFQHCXFN1BG3KPN7D
status: archived
archived_at: 2026-10-31T13:10:58Z
kickoff_branch: null
frozen_at: null
migration: none
codename: Loose Ends
shipped_in: v0.8.1
---

# Implementation Plan

## M_3KPN7D — Polish: Fixes {#M_3KPN7D}

**Goal:** Close the bugs and rough edges collected as M7 follow-ups in M3–M6 and the release pipeline, with no new features.
**Prerequisites:** M_CD7M3M (v0.8.0).
**Status:** FRs written 2026-10-30; ready to implement. User decision 2026-10-30: M7 split into M7a Fixes, M7b UX features, M7c startup logo; no Chinese UI for now; config edits approved (settings.json Bash(claude:*), CLAUDE.md).

**Scope:**
- Trigger panel, console and CLI stream fixes.
- Firmware update fixes (cancel, reload state, target-only disconnect, version policy).
- PD analyzer, replug back-off, CLI pipes, powerz-pd API polish, demo PD/power link.
- Multi-meter controls, recording start, tile order, app.log, record delete and settings edge cases.
- Release pipeline fixes (tag name, Latest badge, release id, MSRV job).
- Housekeeping (settings.json, CLAUDE.md, technical spec).

**Out of scope (M7b):** don't-ask-again for triggers, cable e-marker panel, hide-serial, recorder and chart extras, memory trim. **(M7c):** startup logo. **Open research needing hardware:** rotation bits and bit 7 of 0x60, which total the record screen shows, untested charger protocols (QC3 steps, FCP, AFC, SCP, VFCP, TFCP, SFCP, UFCS) and the EcoFlow FCP/AFC/UFCS no-reply question.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [4PJRVH](../frs/archive/4PJRVH.md) | Trigger panel and console fixes | n/a |
| [4PJRVJ](../frs/archive/4PJRVJ.md) | Firmware update fixes | n/a |
| [4PJRVK](../frs/archive/4PJRVK.md) | PD analyzer, replug and CLI fixes | n/a |
| [4PJRVM](../frs/archive/4PJRVM.md) | Several meters, records and settings fixes | n/a |
| [4PJRVN](../frs/archive/4PJRVN.md) | Release pipeline fixes | n/a |
| [4PJRVP](../frs/archive/4PJRVP.md) | Housekeeping | n/a |

Build order: 4PJRVP, 4PJRVN, 4PJRVK, 4PJRVH, 4PJRVJ, 4PJRVM.

**Tasks:**
- [x] Trigger panel and console fixes — 4PJRVH
- [x] Firmware update fixes — 4PJRVJ
- [x] PD analyzer, replug and CLI fixes — 4PJRVK
- [x] Several meters, records and settings fixes — 4PJRVM
- [x] Release pipeline fixes — 4PJRVN
- [x] Housekeeping — 4PJRVP

**Follow-ups for later:**
- (none yet)

**Device safety:** no new device writes. Read-only checks only; no triggering above 5 V, no flash, delete or settings writes on the real meter in this milestone.

**Gate:** same gating rule as CLAUDE.md.
