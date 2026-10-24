---
milestone: M_X4VEQ3
id: fr_01M5CYB12YQPNR2Y2TA0X4VEQ3
status: archived
archived_at: 2026-10-24T06:07:29Z
kickoff_branch: null
frozen_at: null
migration: none
codename: Trigger Happy
shipped_in: v0.4.0
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
| [2BF5G6](../frs/archive/2BF5G6.md) | Windows CI fails on every failing command and runs green | n/a |
| [PR41FT](../frs/archive/PR41FT.md) | PDM text session over the virtual serial port | n/a |
| [DGSQ6P](../frs/archive/DGSQ6P.md) | USB PD triggering | n/a |
| [DGSQ6Q](../frs/archive/DGSQ6Q.md) | Legacy and vendor protocol triggering | n/a |
| [DGSQ6R](../frs/archive/DGSQ6R.md) | Charger protocol detection | n/a |
| [DGSQ6S](../frs/archive/DGSQ6S.md) | Desktop trigger panel | n/a |
| [DGSQ6T](../frs/archive/DGSQ6T.md) | Custom PD messages and the raw console | n/a |

Build order: PR41FT, then DGSQ6P, DGSQ6R, DGSQ6Q, DGSQ6S, DGSQ6T. 2BF5G6 is implemented (merged with v0.3.0) and is archived with this milestone; record its CI run ids (39865959289, attempts 1 and 3) in its Notes at archive time.

**Gate:** same gating rule as CLAUDE.md.

**M7 follow-ups:**

- Cable info panel: one-click e-marker summary (vendor, current, voltage, speed, active/passive) built on the SOP' Discover Identity preset, with a passive fallback that reads the identity from the bulk PD capture (the charger's own SOP' exchange), since the cable answers only the VCONN source (DGSQ6T Notes).
- Console: clear the command field after a line is sent (found in the 2026-10-23 click-through).
- Trigger session survives a stream error that is not a device loss (decode/protocol): keep the PDM session open, suppress the automatic bulk reconnect while it is open (a new bulk CONNECT silences PDM [device]), and show that live VBUS is unavailable.
- CLI stream: after a queue backlog overflow (long pause at 50 SPS), the stop request 0x0F was rejected at exit [device 2026-10-24]; investigate and make the stop tolerant.
- Request result: after Back to 5 V the card mixes the old command and delivered values with the new request (seen as `pd req=2`, requested 5 V, delivered 8.47 V); show the return-to-5 V result on its own.
- Request result: a failure's message and hint are joined without a space ("…sent.Power-cycle…").
- Hardware coverage still open: QC 3.0 steps, FCP, AFC, SCP, VFCP, TFCP, SFCP and UFCS requests need a charger that speaks them (only PD, PPS, EPR AVS, QC 2.0 and QC 3.0 voltage were verified).
- Possible bug, not only missing hardware: on the EcoFlow the automatic list reported FCP, AFC and UFCS (63 W) as supported, yet `entry fcp`/`entry afc` requests got no reply or left VBUS unchanged and UFCS was not confirmed (DGSQ6Q and DGSQ6R Notes); investigate the legacy request path.
- Desktop detection does not show the D+/D- charging-mode hint that DGSQ6R's Requirement mentions (the CLI does); add it to the Detection card or narrow the wording.
- Console: lengthen the listen window after `entry` lines; the `pdo:` block arrives after it (meter, 2026-10-23).
- Mock backend: mirror the Stage B fixes (PD entry before built messages, cable match by VDM, swaps above 5 V, reserved control types) so demo mode matches the app.
- Risk classes: consider rating SOP'/SOP'' messages as at most 5 V (they cannot raise VBUS) to drop the confirmation on Discover Identity; today the classifier stays fail-closed.
- GoodCRC detection lives in both TypeScript and Rust; keep one source of truth, or add a parity test.
- `.claude/settings.json` lacks `Bash(claude:*)`, so the spawn-pattern probe keeps failing; decide whether to add it.
- `specs/technical-spec.md` still describes the PDM transport as planned; refresh it to match what shipped.
- Toolkit: the dev-process-toolkit evidence renderer and gate capture cannot parse cargo/Vitest/Playwright output (they ran `bun test`); report it upstream.
