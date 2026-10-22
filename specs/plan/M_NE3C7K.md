---
milestone: M_NE3C7K
id: fr_01M5CYB12TRA906QEWFZNE3C7K
status: active
archived_at: null
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
| [KQF5GM](../frs/KQF5GM.md) | CI green on all 8 jobs | n/a |
| [TGE839](../frs/TGE839.md) | Replug reconnects to the meter | n/a |
| [TGE83A](../frs/TGE83A.md) | PD capture core and the powerz pd command | n/a |
| [TGE83B](../frs/TGE83B.md) | USB PD 3.2 message decoder | n/a |
| [TGE83C](../frs/TGE83C.md) | Desktop PD analyzer view | n/a |
| [TGE83D](../frs/TGE83D.md) | PD timeline linked to the power chart | n/a |
| [TGE83E](../frs/TGE83E.md) | PD capture save and load | n/a |

Build order: TGE839, then TGE83A → TGE83B → TGE83C → TGE83D → TGE83E. The UFCS analyzer moves to a later milestone.

**Tasks:**
- [x] CI green on all 8 jobs — KQF5GM (implemented and merged in v0.2.0)
- [ ] Replug reconnects to the meter — TGE839
- [ ] PD capture core and the powerz pd command — TGE83A
- [ ] USB PD 3.2 message decoder — TGE83B
- [ ] Desktop PD analyzer view — TGE83C
- [ ] PD timeline linked to the power chart — TGE83D
- [ ] PD capture save and load — TGE83E

**M7 follow-ups:**
- Replug (TGE839): a reconnect whose open keeps failing retries every 500 ms poll; add a back-off.
- CLI: a closed output pipe (`powerz watch | head`) prints `write failed: Broken pipe`; end quietly instead (watch and pd).
- Gate probe `spawn_pattern_allowlist` fails on `main` too: `.claude/settings.json` lacks `Bash(claude:*)`. Widening the allow-list needs the user's OK.

**Device checks:** pass-through PD capture (charger → meter → phone or laptop), passive only; replug at 1000 SPS. The user approved a short slot for each.

**Gate:** same gating rule as CLAUDE.md.
