---
title: The library docs build fails on links to feature-gated logo methods
milestone: M_PVFHM6
status: active
archived_at: null
id: fr_01M6RVY8F3435N7N8PWY1DHPPW
created_at: 2026-11-06T12:00:00.000Z
changelog_category: Fixed
kind: bug
---

# The library docs build fails on links to feature-gated logo methods

## Summary

The API docs for the protocol library no longer build with warnings treated as errors.
The built-in logo module's docs link to two methods that only exist when image file loading is turned on.
Without that feature the links point nowhere, so the documentation step in CI fails.
The docs now build cleanly both with the feature and without it.

## Requirement

### Steps to reproduce
1. On branch feat/m7c-logo at d52c275, run `RUSTDOCFLAGS="-D warnings" cargo doc -p powerz-protocol --no-deps --locked`.

### Expected
The docs build with no warnings, as CI's "Rust docs" step requires.

### Actual
rustdoc reports two unresolved intra-doc links in `crates/powerz-protocol/src/logo/builtin.rs` line 5: `BuiltinLogo::load` and `BuiltinLogo::encode`, which exist only behind the `image-files` feature (FR 70X5YX). The build fails.

## Acceptance Criteria

- AC-1DHPPW.1: Regression: `RUSTDOCFLAGS="-D warnings" cargo doc -p powerz-protocol --no-deps --locked` (default features, CI's exact command) succeeds.
- AC-1DHPPW.2: `RUSTDOCFLAGS="-D warnings" cargo doc -p powerz-protocol --no-deps --locked --features image-files` succeeds, and the built-in logo module docs still link to `BuiltinLogo::load` and `BuiltinLogo::encode` in that build.
- AC-1DHPPW.3: The full gating chain in CLAUDE.md passes.

## Technical Design

### Root cause
The module doc comment uses intra-doc links to methods defined in an `#[cfg(feature = "image-files")]` impl block, so they do not resolve in a default-feature doc build.

### Fix
Keep the module prose feature-neutral (plain code spans) and attach the links through `#[cfg_attr(feature = "image-files", doc = ...)]` so they appear only where the methods exist.

## Testing

The doc build itself is the regression test: both doc commands are run by hand and in the gate; CI already runs the default-feature command on every push.

## Notes

Found by the M7c orchestrator, 2026-11-06. Docs only; no behaviour change.
