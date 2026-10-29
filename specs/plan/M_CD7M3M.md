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
- [x] Real-meter checks for meter writes — JKWT3X

**M7 follow-ups:**
- Delete with 22 or more remaining recordings (rewritten table over 1023 bytes): refused in M6b because the 10-bit `size` field cannot encode it; revisit if the meter's handling of larger tables is ever documented.
- Delete: refuse the 22+ remaining-entries case before the CSV backup is written, not after (CLI and desktop), with a test at each level.
- Delete: make the re-read mismatch message say when the count matches but the entries differ.
- Desktop: a delete-all on an empty list sends an empty-table write (the UI hides the button); return early like the CLI.
- Settings write: it now goes over the CDC port only (over bulk the meter ignored it on 2026-10-29); we take ACCEPT, FINISHED or no reply, then let the read-back decide. Once the JKWT3X settings run on CDC shows whether the meter applies it and which reply it sends, make the simulator's CDC persona answer the same way and tag both `[device]` in docs/protocol.md.
- Settings write on the desktop with a CDC-only meter session: the write uses that session's port; with a bulk session it opens the CDC port beside it. If another program holds the serial port, the error should say so in plain words.
- Records: work out which charge/energy total the meter's record screen shows. It shows either the last sample or the stored list entry, and on 2026-10-29 the same recording (A03.d) showed the last sample first and the list entry an hour later (docs/protocol.md, Offline records, open question). Until then, real-meter checks compare the screen by name, points and duration only.
- Settings write: the read-back compares the 11 decoded fields only; consider also comparing the undecoded bytes of the 180-byte block.
- Settings: what bits 0..1 of 0x60 (key 0x14) really mean. They read 90° while the screen was upright with auto-rotate on; find the meaning on the meter before offering rotation as an editable field in the UI.
- Settings: what bit 7 of 0x60 means. It went from 0 to 1 while the user changed brightness in the meter's menu (2026-10-29); writes leave it as it is.

**Device safety:** writes only through the two new guarded paths. Forbidden: any other SET_DATA attribute, 0x4A, calibration, OTP, RESET 04, JUMP 08/09, 0x42, memory writes, triggering above 5 V. No write on the real meter before the user approves that stage.

**Gate:** same gating rule as CLAUDE.md.
