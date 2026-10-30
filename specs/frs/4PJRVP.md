---
title: Housekeeping
milestone: M_3KPN7D
status: active
archived_at: null
id: fr_01M65KVDYC49QF1V7N1Y4PJRVP
created_at: 2026-10-30T05:17:20.204Z
changelog_category: Changed
---

# Housekeeping

## Summary

A few project files have fallen behind what shipped.
The project guide lists only two of the six crates. The technical spec still calls the serial trigger path "planned". One toolkit check has failed since the start for lack of a settings entry.
The user approved fixing both config files on 2026-10-30.

## Requirement

The project guide, the technical spec and the repository settings match what has shipped, and the toolkit's spawn check passes.

## Acceptance Criteria

- AC-4PJRVP.1: `.claude/settings.json` allows `Bash(claude:*)`, and the gate's spawn-pattern probe passes.
- AC-4PJRVP.2: The CLAUDE.md Architecture tree lists every crate and app in the workspace, and Tech Stack names the pinned Rust version beside the MSRV.
- AC-4PJRVP.3: specs/technical-spec.md describes the PDM transport, the ADCNEW stream and the two meter-write paths as shipped.

## Technical Design

Text edits only; the probe run is the check.

## Testing

The gate's probe run and a link check.

## Notes

User approval for the settings.json and CLAUDE.md edits: 2026-10-30.
