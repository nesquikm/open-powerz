---
milestone: M_PVFHM6
id: fr_01M6P7JAK35X1G53WNATPVFHM6
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: First Impression
shipped_in: v0.10.0
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
| [PVFHM7](../frs/archive/PVFHM7.md) | Logo image encoder and decoder | n/a |
| [R1ST8Z](../frs/archive/R1ST8Z.md) | Guarded logo write, flag and read paths | n/a |
| [A2XTES](../frs/archive/A2XTES.md) | Simulated logo flash | n/a |
| [TZK5K4](../frs/archive/TZK5K4.md) | Logo upload, remove and read-back in the session | n/a |
| [YCRQRQ](../frs/archive/YCRQRQ.md) | Command line logo commands | n/a |
| [MEHQC0](../frs/archive/MEHQC0.md) | Desktop startup logo view | n/a |
| [Z39H24](../frs/archive/Z39H24.md) | Logo writes change the startup duration and leave the settings check stale (bug) | n/a |
| [QG6RNY](../frs/archive/QG6RNY.md) | Desktop logo view guides the menu save after a logo write (bug) | n/a |
| [CCF47K](../frs/archive/CCF47K.md) | Hidden serials read as the model, and *** where a value must show | n/a |
| [JV54SN](../frs/archive/JV54SN.md) | The chart stops drawing after a meter settings write (bug) | n/a |
| [JV54SP](../frs/archive/JV54SP.md) | The settings-write hint says when the meter's screen really changes (bug) | n/a |
| [JV54SQ](../frs/archive/JV54SQ.md) | The trigger view after the charger went away (bug) | n/a |
| [1DHPPW](../frs/archive/1DHPPW.md) | The library docs build fails on links to feature-gated logo methods (bug) | n/a |
| [ZRJ8W6](../frs/archive/ZRJ8W6.md) | The trigger session close is lost when the charger is attached (bug) | n/a |
| [TJNB54](../frs/archive/TJNB54.md) | The workspace docs build fails on a private link in the desktop crate (bug) | n/a |

Build order: the table's order. The two bug FRs come from the stage 2 and 3 device runs (2026-11-06): SET_DATA 0x1C is the startup duration and leaves the settings CRC A stale. The three bug FRs JV54SN, JV54SP and JV54SQ come from the M7c carry-over device run (2026-11-06, parts S and U). The last three before them come after stage 4 (2026-11-06): the user picked the electron-surf mascot as the project's art, and the stage 4 run found session actions enabled during a write. The guards need the encoder's header check; the session flows are tested on the simulator; the CLI and the desktop wrap the session.

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
- [x] Built-in open-powerz startup logos — 70X5YX
  verify: library and CLI tests on the simulator, Rust command tests, Vitest, Playwright on Chromium and WebKit with E2E_WIDE_FONT=1
- [x] Session actions stay enabled during a logo write — 70X5YY
  verify: Vitest, Playwright on Chromium and WebKit with E2E_WIDE_FONT=1
- [x] Hidden serials read as the model, and *** where a value must show — CCF47K
  verify: Vitest helpers, CLI hide-serial tests, Playwright hide-serial on Chromium and WebKit with E2E_WIDE_FONT=1
- [x] The chart stops drawing after a meter settings write — JV54SN
  verify: sim settings-write rate test, protocol interval tests, worker restart and log tests, Playwright chart before and after Apply
- [x] The settings-write hint says when the meter's screen really changes — JV54SP
  verify: Vitest, Playwright and CLI tests pin the new note
- [x] The trigger view after the charger went away — JV54SQ
  verify: Rust trigger tests on the simulated charger, Vitest, Playwright on the mock
- [x] The library docs build fails on links to feature-gated logo methods — 1DHPPW
  verify: `cargo doc -p powerz-protocol` with -D warnings, with and without image-files
- [x] The trigger session close is lost when the charger is attached — ZRJ8W6
  verify: Rust trigger and device-manager tests on a port whose Hard Reset offers take 3 s
- [x] The workspace docs build fails on a private link in the desktop crate — TJNB54
  verify: `cargo doc --workspace` and `cargo doc -p powerz-protocol` with -D warnings
- [x] Hardware runbooks for stages 1–4 and the M7b carry-overs, in `../qa-runs/m7c/runbooks/` (outside the repo)
  verify: each runbook has its approval gate, stop rules, photos, read-back and settings diffs, and the recovery ladder

**Hardware stages (separate test agents, each write stage with the user's explicit OK in that agent's tab):**
- [x] Stage 1, read-only (PASS, qa-runs/M7C.stage1-20261106-0829): SETTINGS, 16 B and 256 B at 0x98300000, 256 B at 0x9831D400, startup-screen photo.
- [x] Stage 2, tiny logo (written and verified, shows at boot; found Z39H24; qa-runs/M7C.stage2-20261106-0920) (OK #1): 48×48 opaque solid, read-back, SETTINGS diff around the flag, erase granularity, boot photo.
- [x] Stage 3, Remove (PASS, qa-runs/M7C.stage3-20261106-0953) (OK #2): read-back 0xFF, erase extent, built-in logo at boot.
- [x] Stage 4, real image (PASS for both parts, Remove skipped by the user; found 70X5YY; qa-runs/M7C.stage4-20261106-1248) (OK #3, then OK #4 for alpha 0/255): full read-back, neighbourhood reads before and after, boot photo.
- [x] Final: the built-in mascot logo on the real meter (PASS, qa-runs/M7C.final-20261106-1633).

**Deferred (hardware checks carried from M7b to this sitting, wall-charger setup, nothing on the output):**
- [x] AXHK2J AC.9 (qa-runs/M7C.carryover-20261106-1703, part S): one settings write from the desktop app on the real meter, read back.
- [x] AXHK2N AC.9 (qa-runs/M7C.verify-20261106-2044, part A): unplug the charger under an accepted contract; the armed state ends within 1 s.
- [x] AXHK2N AC.10 (qa-runs/M7C.carryover-20261106-1703, part U): the same unplug forgets the offers and says the charger went away; a same-charger replug sends nothing.

**Release checklist:**
- [x] Run the @perf specs on a developer Mac in both browsers before the release: `bun run e2e:ui --grep @perf --workers=1`. Done on a developer Mac at b4bd624: 6 of 6 passed (qa-runs/m7c/perf-release-check-b4bd624.log).

**Device safety:** stage 0 never opens the real meter. The only new device paths are the three guarded ones in R1ST8Z; the shared guard keeps refusing every other 0x4A and 0x48 frame. Upload is KM003C-only.

**Gate:** same gating rule as CLAUDE.md, plus Playwright on Chromium and WebKit (and `E2E_WIDE_FONT=1`), one run at a time.
