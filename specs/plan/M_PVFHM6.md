---
milestone: M_PVFHM6
id: fr_01M6P7JAK35X1G53WNATPVFHM6
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: First Impression
---

# Implementation Plan

## M_PVFHM6 — Polish: Startup Logo Upload {#M_PVFHM6}

**Goal:** Upload, remove and read the meter's startup logo, built and proven on the simulator first, with the real-meter stages run later by test agents with the user's OK.
**Prerequisites:** M_7PKVA6 (v0.9.0).
**Status:** FRs written 2026-11-05 from the design proposal the user approved that day (M7c). Stage 0 = this repo's code, simulator and tests; stages 1–4 are hardware runbooks outside the repo.

**Scope:**
- Pure logo encoder and decoder, three guarded device paths (0x4A logo chunks, SET_DATA 0x1C, 0x44 reads in 0x98300000–0x9831FFFF), the simulator model, the session flows, `powerz logo`, the desktop view.
- Carried from M7b: the deferred hardware checks and the @perf release check (below).

**Out of scope:** any other 0x4A or 0x48 use, logo upload on the KM002C (refused until one is tested), alpha blending beyond alpha 0/255 (open until stage 2), Chinese UI, web edition.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [PVFHM7](../frs/PVFHM7.md) | Logo image encoder and decoder | n/a |
| [R1ST8Z](../frs/R1ST8Z.md) | Guarded logo write, flag and read paths | n/a |
| [A2XTES](../frs/A2XTES.md) | Simulated logo flash | n/a |
| [TZK5K4](../frs/TZK5K4.md) | Logo upload, remove and read-back in the session | n/a |
| [YCRQRQ](../frs/YCRQRQ.md) | Command line logo commands | n/a |
| [MEHQC0](../frs/MEHQC0.md) | Desktop startup logo view | n/a |
| [Z39H24](../frs/Z39H24.md) | Logo writes change the startup duration and leave the settings check stale (bug) | n/a |
| [QG6RNY](../frs/QG6RNY.md) | Desktop logo view guides the menu save after a logo write (bug) | n/a |

Build order: the table's order. The two bug FRs come from the stage 2 and 3 device runs (2026-11-06): SET_DATA 0x1C is the startup duration and leaves the settings CRC A stale. The last three come after stage 4 (2026-11-06): the user picked the electron-surf mascot as the project's art, and the stage 4 run found session actions enabled during a write. The guards need the encoder's header check; the session flows are tested on the simulator; the CLI and the desktop wrap the session.

**Tasks:**
- [x] Logo image encoder and decoder — PVFHM7
  verify: byte-vector unit tests on the worked examples, wasm32 check without the file feature
- [x] Guarded logo write, flag and read paths — R1ST8Z
  verify: accept/refuse table tests in guard and memory modules
- [x] Simulated logo flash — A2XTES
  verify: sim unit tests per erase policy and fault
- [x] Logo upload, remove and read-back in the session — TZK5K4
  verify: mock-transport tests, then sim end-to-end and fault cases
- [x] Command line logo commands — YCRQRQ
  verify: CLI integration tests against the simulator
- [x] Desktop startup logo view — MEHQC0
  verify: Rust command tests, Vitest, Playwright on Chromium and WebKit with E2E_WIDE_FONT=1
- [x] Startup duration and stale settings check in the library, simulator and CLI — Z39H24
  verify: sim tests for the stale block and menu save, CLI tests for the wait and exit codes
- [x] Desktop menu-save panel and settings guidance — QG6RNY
  verify: Rust command tests, Vitest, Playwright on Chromium and WebKit with E2E_WIDE_FONT=1
- [x] open-powerz brand art, app icon and README banner — 492Z4N
  verify: Vitest brand checks plus the existing icon tests
- [ ] Built-in open-powerz startup logos — 70X5YX
  verify: library and CLI tests on the simulator, Rust command tests, Vitest, Playwright on Chromium and WebKit with E2E_WIDE_FONT=1
- [ ] Session actions stay enabled during a logo write — 70X5YY
  verify: Vitest, Playwright on Chromium and WebKit with E2E_WIDE_FONT=1
- [x] Hardware runbooks for stages 1–4 and the M7b carry-overs, in `../qa-runs/m7c/runbooks/` (outside the repo)
  verify: each runbook has its approval gate, stop rules, photos, read-back and settings diffs, and the recovery ladder

**Hardware stages (separate test agents, each write stage with the user's explicit OK in that agent's tab):**
- Stage 1, read-only: SETTINGS, 16 B and 256 B at 0x98300000, 256 B at 0x9831D400, startup-screen photo.
- Stage 2, tiny logo (OK #1): 48×48 opaque solid, read-back, SETTINGS diff around the flag, erase granularity, boot photo.
- Stage 3, Remove (OK #2): read-back 0xFF, erase extent, built-in logo at boot.
- Stage 4, real image (OK #3, then OK #4 for alpha 0/255): full read-back, neighbourhood reads before and after, boot photo.

**Deferred (hardware checks carried from M7b to this sitting, wall-charger setup, nothing on the output):**
- AXHK2J AC.9: one settings write from the desktop app on the real meter, read back.
- AXHK2N AC.9: unplug the charger under an accepted contract; the armed state ends within 1 s.
- AXHK2N AC.10: the same unplug forgets the offers and says the charger went away; a same-charger replug sends nothing.

**Release checklist:**
- Run the @perf specs on a developer Mac in both browsers before the release: `bun run e2e:ui --grep @perf --workers=1`.

**Device safety:** stage 0 never opens the real meter. The only new device paths are the three guarded ones in R1ST8Z; the shared guard keeps refusing every other 0x4A and 0x48 frame. Upload is KM003C-only.

**Gate:** same gating rule as CLAUDE.md, plus Playwright on Chromium and WebKit (and `E2E_WIDE_FONT=1`), one run at a time.
