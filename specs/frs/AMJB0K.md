---
title: A few docs still lag behind v0.12.1
milestone: M_SPYPSC
status: active
archived_at: null
id: fr_01M731VQ73CVNCX463XRAMJB0K
created_at: 2026-11-10T15:40:16.932Z
changelog_category: Fixed
kind: bug
---

# A few docs still lag behind v0.12.1

## Summary

Version 0.12.1 changed the File tab stats, the stall notice, the serial-port watch and the web device menu.
A read-through after the release looks for claims in the README and the pages under docs that still describe the old behaviour.
Each stale claim found is fixed in place.
Released changelog sections stay as they are.

## Requirement

### Steps to reproduce
1. Read the README and the pages under docs after v0.12.1.
2. Compare each claim with the app, the CLI and the code.

### Expected
Every claim matches v0.12.1.

### Actual
Some claims may still describe the behaviour before v0.12.1.

## Acceptance Criteria

- AC-AMJB0K.1: Every stale or wrong claim the read-through finds in the README or a page under docs is corrected, and the Notes list each one.
- AC-AMJB0K.2: Released changelog sections are unchanged.

## Technical Design

### Root cause
Some pages were not touched when the behaviour they describe changed.

### Fix
Read every page against the code, fix the wording in place. No code changes.

## Testing

The existing docs tests and the new ones from the sibling FR pass.

## Notes

Fixed: the web device menu order and its HID label; CLI CSV TEMP; the CLI archive folder in install; which desktop-only controls hide in the backend doc; the docs index name of the web edition; Cancel keeps the open file; the desktop-only list counted firmware twice.
