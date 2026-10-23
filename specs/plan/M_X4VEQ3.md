---
milestone: M_X4VEQ3
id: fr_01M5CYB12YQPNR2Y2TA0X4VEQ3
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Trigger Happy
---

# Implementation Plan

## M_X4VEQ3 — Protocol Triggers {#M_X4VEQ3}

**Goal:** Protocol triggering and detection over the virtual serial port, in the CLI and the desktop app, behind a confirmation for every request above 5 V.
**Prerequisites:** M_34BAXK (CDC transport); M_R4WG4Q (desktop shell).
**Status:** FRs written 2026-10-23; ready to implement. Re-scoped to triggers only (user decision 2026-10-23); offline records, the settings view, multi-device and the ADCNEW stream move to M6.

**Scope:**
- PDM text session, reply and capability-block parsing, and a risk classifier for every command.
- PD triggering (fixed, PPS, AVS, EPR, hard reset, e-marker and sink emulation).
- QC 2.0/3.0, FCP, SCP, AFC, VFCP, SFCP, TFCP and UFCS triggering.
- Detection (BC1.2, Apple, the automatic list) and a D+/D- hint table.
- Desktop trigger panel; custom PD messages and a raw console.
- A confirmation for every request above 5 V (CLI: an allow-high-voltage flag). Hardware checks run with nothing on the output.

**Moved to M6:** offline records, settings view, multi-device grid, ADCNEW stream.

**Out of scope here:** firmware update (M5) and signed release packaging.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [2BF5G6](../frs/2BF5G6.md) | Windows CI fails on every failing command and runs green | n/a |
| [PR41FT](../frs/PR41FT.md) | PDM text session over the virtual serial port | n/a |
| [DGSQ6P](../frs/DGSQ6P.md) | USB PD triggering | n/a |
| [DGSQ6Q](../frs/DGSQ6Q.md) | Legacy and vendor protocol triggering | n/a |
| [DGSQ6R](../frs/DGSQ6R.md) | Charger protocol detection | n/a |
| [DGSQ6S](../frs/DGSQ6S.md) | Desktop trigger panel | n/a |
| [DGSQ6T](../frs/DGSQ6T.md) | Custom PD messages and the raw console | n/a |

Build order: PR41FT, then DGSQ6P, DGSQ6R, DGSQ6Q, DGSQ6S, DGSQ6T. 2BF5G6 is implemented (merged with v0.3.0) and is archived with this milestone; record its CI run ids (39865959289, attempts 1 and 3) in its Notes at archive time.

**Gate:** same gating rule as CLAUDE.md.

**M7 follow-ups:**

- Cable info panel: one-click e-marker summary (vendor, current, voltage, speed, active/passive) built on the SOP' Discover Identity preset, with a passive fallback that reads the identity from the bulk PD capture (the charger's own SOP' exchange), since the cable answers only the VCONN source (DGSQ6T Notes).
