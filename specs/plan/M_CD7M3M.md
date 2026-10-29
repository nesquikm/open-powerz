---
milestone: M_CD7M3M
id: fr_01M61PKCQKB552ZH6D0BCD7M3M
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Write Stuff
---

# Implementation Plan

## M_CD7M3M — Meter Writes {#M_CD7M3M}

**Goal:** Delete offline recordings and change the meter's display and rate settings, safely: narrow guard rules, backups and typed confirmations, read-back checks, and user-approved first runs on the real meter.
**Prerequisites:** M_1HHSZZ (record list/download, settings decode, v0.7.0).
**Status:** FRs written 2026-10-28; ready to implement. User decision 2026-10-28: delete recordings and change settings; startup logo stays in M7; never calibration.

**Scope:**
- Record delete via SET_DATA 0x48 att 0x28, backup first, typed confirm, re-read verify.
- Settings write via type 0x0A with recovered keys only, range-checked, read-back verify.
- Staged real-meter checks, each write approved by the user.

**Out of scope:** startup logo, calibration, factory reset, OTP, any other SET_DATA attribute.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [JKWT3V](../frs/JKWT3V.md) | Delete offline recordings | n/a |
| [JKWT3W](../frs/JKWT3W.md) | Change meter settings | n/a |
| [JKWT3X](../frs/JKWT3X.md) | Real-meter checks for meter writes | n/a |

Build order: JKWT3V, JKWT3W, then JKWT3X (each stage can run once its feature is done).

**Tasks:**
- [x] Delete offline recordings — JKWT3V
- [x] Change meter settings — JKWT3W
- [ ] Real-meter checks for meter writes — JKWT3X

**M7 follow-ups:**
- Delete with 22 or more remaining recordings (rewritten table over 1023 bytes): refused in M6b because the 10-bit `size` field cannot encode it; revisit if the meter's handling of larger tables is ever documented.
- Delete: refuse the 22+ remaining-entries case before the CSV backup is written, not after (CLI and desktop), with a test at each level.
- Delete: make the re-read mismatch message say when the count matches but the entries differ.
- Desktop: a delete-all on an empty list sends an empty-table write (the UI hides the button); return early like the CLI.
- Settings write: we now take ACCEPT, FINISHED or no reply to 0x0A, then let the read-back decide (the meter answered the record-table write with FINISHED on 2026-10-29). Once the JKWT3X settings run shows which reply the meter sends, make the simulator answer the same way and tag it `[device]` in docs/protocol.md.
- Settings write: the read-back compares the 11 decoded fields only; consider also comparing the undecoded bytes of the 180-byte block.

**Device safety:** writes only through the two new guarded paths. Forbidden: any other SET_DATA attribute, 0x4A, calibration, OTP, RESET 04, JUMP 08/09, 0x42, memory writes, triggering above 5 V. No write on the real meter before the user approves that stage.

**Gate:** same gating rule as CLAUDE.md.
