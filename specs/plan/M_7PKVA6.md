---
milestone: M_7PKVA6
id: fr_01M69QVAS60M21YPGYA47PKVA6
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Fine Print
---

# Implementation Plan

## M_7PKVA6 — Polish: UX Features {#M_7PKVA6}

**Goal:** Close the M7a partial acceptance criteria and follow-ups, then add the UX features the vendor app has and we lack.
**Prerequisites:** M_3KPN7D (v0.8.1).
**Status:** FRs written 2026-10-31. M7 split per user decision 2026-10-30; don't-ask-again per user decision 2026-10-26.

**Scope:**
- Fixes first: firmware update consent, app log, MSRV job, settings double read and unplug detection, notices, review leftovers.
- Features: trigger don't-ask-again, cable panel, hide serial, recorder extras, recording import, chart extras, memory trim.

**Out of scope:** startup logo (M7c), Chinese UI, web edition. `CLAUDE.md:11` stays as it is until the user answers.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [AXHK2F](../frs/AXHK2F.md) | A firmware update never ends a recording unasked | n/a |
| [AXHK2G](../frs/AXHK2G.md) | The app log records errors and firmware updates | n/a |
| [AXHK2H](../frs/AXHK2H.md) | The MSRV job covers the desktop crate | n/a |
| [AXHK2J](../frs/AXHK2J.md) | Settings writes and unplugs are detected reliably | n/a |
| [AXHK2K](../frs/AXHK2K.md) | Notices and opened captures survive other meters' actions | n/a |
| [AXHK2M](../frs/AXHK2M.md) | Firmware, trigger and release review leftovers | n/a |
| [AXHK2N](../frs/AXHK2N.md) | Trigger panel "don't ask again" | n/a |
| [AXHK2P](../frs/AXHK2P.md) | One-click cable e-marker panel | n/a |
| [AXHK2Q](../frs/AXHK2Q.md) | Hide serial numbers | n/a |
| [AXHK2R](../frs/AXHK2R.md) | Recorder pause, auto-stop and limits | n/a |
| [AXHK2S](../frs/AXHK2S.md) | Import recordings and vendor database files | n/a |
| [AXHK2T](../frs/AXHK2T.md) | Chart ruler, segment marks and energy share | n/a |
| [AXHK2V](../frs/AXHK2V.md) | Chart appearance and image export | n/a |
| [AXHK2W](../frs/AXHK2W.md) | Memory trim at 1000 SPS | n/a |

Build order: the table's order. AXHK2W goes last so it measures the finished chart.

**Tasks:**
- [ ] A firmware update never ends a recording unasked — AXHK2F
  verify: targeted `devices.rs` tests, Vitest, Playwright prompt path
- [ ] The app log records errors and firmware updates — AXHK2G
  verify: temporary-log unit tests, including the serial scan
- [ ] The MSRV job covers the desktop crate — AXHK2H
  verify: `release-workflow.test.ts`, then the job's run on the PR
- [ ] Settings writes and unplugs are detected reliably — AXHK2J
  verify: session and transport unit tests, then a tester runbook
- [ ] Notices and opened captures survive other meters' actions — AXHK2K
  verify: Vitest and Playwright tile flows
- [ ] Firmware, trigger and release review leftovers — AXHK2M
  verify: per-item unit tests
- [ ] Trigger panel "don't ask again" — AXHK2N
  verify: disarm-path unit tests, Playwright, then a tester runbook
- [ ] One-click cable e-marker panel — AXHK2P
  verify: simulator cable tests, Playwright, then a tester runbook
- [ ] Hide serial numbers — AXHK2Q
  verify: Playwright DOM scan and CLI tests
- [ ] Recorder pause, auto-stop and limits — AXHK2R
  verify: hand-fed record tests and Playwright
- [ ] Import recordings and vendor database files — AXHK2S
  verify: generated-file tests and a round trip
- [ ] Chart ruler, segment marks and energy share — AXHK2T
  verify: pure-module tests, `range_stats` tests, visual check
- [ ] Chart appearance and image export — AXHK2V
  verify: SVG and PDF parse tests, visual check
- [ ] Memory trim at 1000 SPS — AXHK2W
  verify: element-count tests and a macOS demo measurement

**Follow-ups for later (triaged from M7a, each kept out on purpose):**
- Needs hardware we lack: rotation bits and bit 7 of 0x60, which total the record screen shows, QC 3.0 steps, FCP, AFC, SCP, VFCP, TFCP, SFCP and UFCS coverage, the EcoFlow no-reply question, the Apple battery curve, Windows `\\?\` paths, the root cause of the rejected stop after a queue overflow.
- Needs a vendor sample we lack: proof that our `.db` opens in the vendor app.
- Stays refused: delete with 22 or more remaining recordings.
- Low value, no user effect: mock and simulator divergences, floor test gaps, duplicate asserts, `released` visibility, `limit()` being test-only, FirstSeen filter lists, the hard-coded AppImage name, release-note blank lines, plain string equality in checks, `[doc]` tags, stale wording in two comments, `claim_part` naming, the dead `startRecording` null branch, the `undecoded_diff` skipped words, `is_recording_in` accepting a subdirectory, `entry bc`/`apple` wait time.
- Outside this repo: the toolkit's evidence renderer report, the syspolicyd reminder for the user.
- Unchanged by rule: `a_sink_that_blocks_200_ms_per_event_loses_no_record_at_1000_sps` keeps its timing.

**Device safety:** the double read adds SETTINGS reads only. AXHK2N and AXHK2P drive the bus and run only through the tester's runbooks, at or below 20 V in the wall-charger setup. No flash, delete or logo write on the real meter is planned.

**Gate:** same gating rule as CLAUDE.md.
