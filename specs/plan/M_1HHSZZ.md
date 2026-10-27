---
milestone: M_1HHSZZ
id: fr_01M5XK5N83KRGEZMRZRV1HHSZZ
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Memory Lane
---

# Implementation Plan

## M_1HHSZZ — Meter Data (read side) {#M_1HHSZZ}

**Goal:** Read what the meter keeps by itself (offline recordings and settings), add the ADCNEW push stream as a second live source, and let the desktop app run several meters at once. Nothing on the meter is written.
**Prerequisites:** M_MJ7KE9 (memory reads, auth, device mode); M_X4VEQ3 (PDM text session on CDC); M_FW682R (v0.6.1).
**Status:** FRs written 2026-10-27; ready to implement. Orchestrator decision 2026-10-27: M6 is split. This milestone (M6a) is read-only; record delete and settings writes (both need a user-approved safety exception) follow in M6b.

**Scope:**
- Offline records: list, download one or all, decode the 16-byte samples, CSV export, history chart; safe-read window per record.
- Settings block decode, read-only CLI and desktop panel, checked against the meter's screen.
- ADCNEW stream on CDC with a demultiplexer shared with the PDM text session; CLI and desktop source option.
- Multi-device desktop grid with per-meter sessions.
- Real-meter checks from runbooks run by fresh test agents.

**Out of scope (M6b):** record delete (SET_DATA 0x48 att 0x28), settings writes (type 0x0A), startup logo.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [1HHT00](../frs/1HHT00.md) | Offline records: list and download | n/a |
| [1HHT01](../frs/1HHT01.md) | Meter settings, read-only | n/a |
| [1HHT02](../frs/1HHT02.md) | ADCNEW stream over the virtual serial port | n/a |
| [1HHT03](../frs/1HHT03.md) | Several meters at once | n/a |
| [1HHT04](../frs/1HHT04.md) | Real-meter checks for meter data | n/a |

Build order: 1HHT01, 1HHT00, 1HHT02, 1HHT03, then 1HHT04 (its stages can run as soon as their feature is done).

**Tasks:**
- [ ] Offline records: list and download — 1HHT00
- [ ] Meter settings, read-only — 1HHT01
- [ ] ADCNEW stream over the virtual serial port — 1HHT02
- [ ] Several meters at once — 1HHT03
- [ ] Real-meter checks for meter data — 1HHT04

**M7 follow-ups:**
- Release publish sends no tag_name in its updates; the v0.6.0 draft was published as "untagged-…" and repaired by hand. Send tag_name and check it after publishing (found after M_FW682R).
- A manual release re-run of an older tag takes the Latest badge (make_latest true).
- Settings rotation: bits 0–1 of u32@0x60 read 90° while the screen shows 0° with auto-rotate on. Re-check with auto-rotate off before trusting the mapping (found in 1HHT04 settings stage).

**Device safety:** read-only. Forbidden: SET_DATA 0x48 (any att), 0x4A, settings writes 0x0A, RESET 04, JUMP 08/09, 0x42, memory reads outside the safe list plus the per-record window, triggering above 5 V.

**Gate:** same gating rule as CLAUDE.md.
