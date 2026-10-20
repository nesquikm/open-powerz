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

## M_X4VEQ3 — Triggers and Extras {#M_X4VEQ3}

**Goal:** Protocol triggering over the virtual serial port, offline records, a settings view and multi-device support.
**Prerequisites:** M_34BAXK (CDC transport); M_R4WG4Q (desktop shell).
**Status:** requirements level only — FRs and ACs are written with `/spec-write` before `/implement`. Needs a device session with a PD/QC charger to confirm text replies.

**Scope (requirements level):**
- CDC PDM text commands: session (`pdm open` / `close`), protocol entry (PD, UFCS, QC, FCP, SCP, AFC, VFCP, SFCP), requests (`pd req=`, `qc3 volt=`, …), e-marker and sink emulation; binary/text demux on one stream.
- Safety confirmation with a clear warning for any request above 5 V (VBUS can reach 48 V); the CLI requires an explicit flag.
- ADCNEW push stream on CDC (CRC-8 poly 0x29) as an optional low-overhead stream.
- Offline-record list, download and delete (needs the 16-byte sample format confirmed on the device).
- Settings view (read-only first; writes only once each field is confirmed safe).
- Multi-device grid in the desktop app.

**Out of scope here:** firmware update (risk of bricking; only if the user asks) and signed release packaging.

**FR list:** to be written.

**Gate:** same gating rule as CLAUDE.md.
