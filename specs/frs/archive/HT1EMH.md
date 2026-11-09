---
title: Serial lint misses its allowlist on Windows paths
milestone: M_QFTCWW
status: archived
archived_at: 2026-11-08T21:34:13Z
id: fr_01M6XKFBKRCVDYSTRQDRHT1EMH
created_at: 2026-11-08T12:52:30.969Z
changelog_category: Fixed
kind: bug
---

# Serial lint misses its allowlist on Windows paths

## Summary

A test checks that only known UI modules read a meter serial.
It compares file names against a list written with forward slashes.
On Windows the directory walk returns backslashes, so the web session module is flagged.
The fix turns every walked name into forward-slash form before the comparison.

## Requirement

### Steps to reproduce
1. On windows-latest, run `bun run test` (CI run 42480023059, PR #15, gate job).

### Expected
`hideSerial.lint.test.ts` passes: `web/session.ts` is on its known list.

### Actual
`components read serials through the helper (AC-AXHK2Q.2) › no other module of the UI reads a serial` fails with `expected [ 'web\session.ts' ] to deeply equal []`.

## Acceptance Criteria

- AC-HT1EMH.1: The serial-read lint compares module names in forward-slash form on every OS, so a known module in a subfolder matches its list entry.
- AC-HT1EMH.2: Regression: `apps/desktop/src/lib/hideSerial.lint.test.ts › module names` shows that a Windows-style name such as `web\session.ts` normalizes to `web/session.ts`; it fails before the fix and passes after it.

## Technical Design

### Root cause
`apps/desktop/src/lib/hideSerial.lint.test.ts` lists modules with `readdirSync(..., { recursive: true })`, which yields platform separators. The `known` set holds `web/session.ts`, so on Windows the backslash name never matches.

Normalize each walked name with a small helper that replaces backslashes with forward slashes, and use it in both walks of that file.

## Testing

Vitest: a unit check of the helper with a backslash name, plus the existing walk on the host OS.

## Notes

Introduced with the web backend; hidden until the Windows gate reached Vitest after the wasm build-order fix.
