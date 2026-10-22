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
**Status:** requirements level only — FRs and ACs are written with `/spec-write` before `/implement`. Needs a device session with a PD charger and sink to confirm record and event formats.

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

PD analyzer FRs: to be written.

**Gate:** same gating rule as CLAUDE.md.
