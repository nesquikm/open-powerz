---
title: Gate typechecks the UI before the wasm module exists
milestone: M_QFTCWW
status: archived
archived_at: 2026-11-08T21:34:13Z
id: fr_01M6XHFRZZAZAARM7NVCY6JFM3
created_at: 2026-11-08T12:17:47.519Z
changelog_category: Fixed
kind: bug
---

# Gate typechecks the UI before the wasm module exists

## Summary

The gate checks the UI's types before it builds the wasm module the web backend imports.
On a fresh checkout that module does not exist yet, so the type check fails on every OS.
It only passed locally because an older build of the module was still on disk.
The fix builds the wasm module first, in CI and in the documented gate chain.

## Requirement

### Steps to reproduce
1. Start from a fresh checkout, where `apps/desktop/src/wasm/` is absent (it is git-ignored).
2. Run the CI `gate` job, or the CLAUDE.md gating chain from the repo root.

### Expected
The gate passes: the UI typecheck sees the generated wasm module.

### Actual
`bun run check` (svelte-check) fails with `Cannot find module '../../wasm/powerz_wasm.js' or its corresponding type declarations` on ubuntu-24.04, macos-latest and windows-latest (CI run 42477535385, PR #15), because `bun run build:wasm` runs after `bun run check`.

## Acceptance Criteria

- AC-Y6JFM3.1: The CI `gate` job's JS step runs `bun run build:wasm` before `bun run check` and before `bun run test`.
- AC-Y6JFM3.2: The CLAUDE.md Gating rule chain lists `bun run build:wasm` before `bun run check`.
- AC-Y6JFM3.3: Regression: `apps/desktop/src/lib/web/wasmBuild.test.ts › gate order` fails when `bun run check` precedes `bun run build:wasm` in the ci.yml gate step or in the CLAUDE.md gating chain; it fails before the fix and passes after it.

## Technical Design

### Root cause
`.github/workflows/ci.yml` (gate job, "JS lint, typecheck, test, build") and the CLAUDE.md Gating rule run `check` before `build:wasm`. `apps/desktop/src/lib/web/webBackend.ts` imports `../../wasm/powerz_wasm.js`, which only `build:wasm` generates, so svelte-check cannot resolve it.

Move `bun run build:wasm` ahead of `bun run check` in both places. Extend the existing gate-order test to read both files and compare the two steps' positions.

## Testing

Vitest, line-based reads of `.github/workflows/ci.yml` and `CLAUDE.md`, in the style of the existing gate-order test.

## Notes

Found on the first CI run of PR #15. The local gate hid it because a stale generated module was on disk.
