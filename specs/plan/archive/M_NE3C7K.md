---
milestone: M_NE3C7K
id: fr_01M5CYB12TRA906QEWFZNE3C7K
status: archived
archived_at: 2026-10-22T20:51:07Z
kickoff_branch: null
frozen_at: null
migration: none
codename: Packet Sleuth
---

# Implementation Plan

## M_NE3C7K — PD Analyzer {#M_NE3C7K}

**Goal:** Capture, decode and inspect USB Power Delivery traffic through the meter.
**Prerequisites:** M_34BAXK (session, auth level 1 for sniffer start/stop); M_R4WG4Q (desktop shell and charts).
**Status:** FRs written 2026-10-22; ready to implement.

**Scope (requirements level):**
- PD sniffer start/stop (0x10/0x11, auth level ≥ 1) and PD_PACKET polling at ~5 ms.
- Record and event parsing (attach/detach, hard reset, cable reset, receive errors, big messages), timestamps.
- Full USB PD r3.2 message decode: control/data/extended messages, Source/Sink capabilities, PDO/RDO (fixed, battery, variable, PPS, AVS, EPR), VDM, with the spec's field widths.
- Packet table with GoodCRC filter and details tree, timeline linked to the power chart.
- Export (and import) of captures; vendor `.sqlite` compatibility as a stretch goal.
- UFCS capture (att 0x040) noted for later.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [KQF5GM](../../frs/archive/KQF5GM.md) | CI green on all 8 jobs | n/a |
| [TGE839](../../frs/archive/TGE839.md) | Replug reconnects to the meter | n/a |
| [TGE83A](../../frs/archive/TGE83A.md) | PD capture core and the powerz pd command | n/a |
| [TGE83B](../../frs/archive/TGE83B.md) | USB PD 3.2 message decoder | n/a |
| [TGE83C](../../frs/archive/TGE83C.md) | Desktop PD analyzer view | n/a |
| [TGE83D](../../frs/archive/TGE83D.md) | PD timeline linked to the power chart | n/a |
| [TGE83E](../../frs/archive/TGE83E.md) | PD capture save and load | n/a |

Build order: TGE839, then TGE83A → TGE83B → TGE83C → TGE83D → TGE83E. The UFCS analyzer moves to a later milestone.

**Tasks:**
- [x] CI green on all 8 jobs — KQF5GM (implemented and merged in v0.2.0)
- [x] Replug reconnects to the meter — TGE839
- [x] PD capture core and the powerz pd command — TGE83A
- [x] USB PD 3.2 message decoder — TGE83B
- [x] Desktop PD analyzer view — TGE83C
- [x] PD timeline linked to the power chart — TGE83D
- [x] PD capture save and load — TGE83E

**M7 follow-ups:**
- Replug (TGE839): a reconnect whose open keeps failing retries every 500 ms poll; add a back-off.
- CLI: a closed output pipe (`powerz watch | head`) prints `write failed: Broken pipe`; end quietly instead (watch and pd).
- powerz-pd API polish before the web edition: `#[non_exhaustive]` public enums, `Display` for `Issue`, one visibility rule for sub-decoders.
- CLAUDE.md Architecture tree lists only powerz-protocol and powerz-cli; add powerz-pd, powerz-sim and powerz-record (needs the user's OK to edit).
- PD analyzer: cap the backend record store and the UI column store for long captures (or re-decode on demand); make the type/SOP option lists and filtering incremental; make the page behind the Clear dialog inert.
- PD timeline: drop PD status samples that were in flight at a Clear; add an e2e check that a badge splits on zoom.
- Demo device: the PD scenario negotiates 9 V while the meter stream stays at 5 V, so the chart and the PD table disagree. Link the simulator's PD and measurement streams.
- PD files: count skipped vendor rows in the import notice; open a .pdcap damaged on its first entry as an empty capture with the damage notice; give `pd decode` a file-error exit code instead of 5; share `record_from` between pdcap and the vendor import.
- PD files: an automatic reconnect after a replug does not release an opened capture's memory (only the Connect command does).
- Replug HIL: a first real-meter run (2026-10-23) missed an unplug the user made inside its 120 s window (it logged nothing, so the cause is unknown; a hung transfer is the main suspect). Three instrumented runs that day all passed (unplug seen within 2 ms of the OS removal, reconnect 136–140 ms); keep re-running it and fix the miss if it recurs.
- Gate probe `spawn_pattern_allowlist` fails on `main` too: `.claude/settings.json` lacks `Bash(claude:*)`. Widening the allow-list needs the user's OK.

**Device checks:** pass-through PD capture (charger → meter → phone or laptop), passive only; replug at 1000 SPS. The user approved a short slot for each.

**Gate:** same gating rule as CLAUDE.md.
