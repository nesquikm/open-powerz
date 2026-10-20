---
milestone: M_R4WG4Q
id: fr_01M5CYB12PNSTXG28YNTR4WG4Q
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Live Wire
---

# Implementation Plan

## M_R4WG4Q — Desktop Meter {#M_R4WG4Q}

**Goal:** The Tauri desktop app becomes a usable live meter and recorder on macOS, Linux and Windows.
**Prerequisites:** M_34BAXK (protocol core and CLI).
**Status:** requirements level only — FRs and ACs are written with `/spec-write` before `/implement`.

**Scope (requirements level):**
- Live readout of VBUS, IBUS (signed, direction shown), power, temperature, D+/D−, CC1/CC2.
- High-rate charts with uPlot; min/max decimation in Rust so long histories (up to 10 million points) stay interactive; zoom, pan, crosshair, dual axis.
- Charge (Ah) and energy (Wh) accumulation, statistics (min/max/avg, duration).
- Start/stop recording at 2/10/50/1000 SPS; CSV export in the vendor-compatible layout (reusing the CLI writer); save while recording.
- Device connect/disconnect handling with hotplug; recording survives an unplug with data kept.
- Batched IPC from the Rust backend to the UI (20–60 Hz frames), never one message per sample.
- Modern UI with light and dark themes; does not copy the vendor look.

**FR list:** to be written.

**Gate:** same gating rule as CLAUDE.md.
