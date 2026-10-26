---
title: Pinned Rust toolchain
milestone: M_WBBQ7F
status: active
archived_at: null
id: fr_01M5WNDAHHETCCVHSJK4E1ZJ2M
created_at: 2026-10-26T17:51:25.233Z
changelog_category: Changed
---

# Pinned Rust toolchain

## Summary

Every build of open-powerz uses one exact Rust version, named in a single file at the root of the repository.
Local builds, CI and release builds all pick it up.
A new Rust release on the CI runners can then no longer break a green branch with new lints.
Moving to a newer Rust version becomes a one-line change that goes through review like any other.

## Requirement

The repository pins one exact Rust toolchain, with rustfmt and clippy, in a root toolchain file that local builds, the CI workflow and the release workflow all use.

## Acceptance Criteria

- AC-E1ZJ2M.1: A rust-toolchain.toml at the repository root names one exact stable version (1.99.1 or newer, never a moving channel such as `stable`), the minimal profile, and the rustfmt and clippy components.
- AC-E1ZJ2M.2: The CI workflow and the release workflow install the toolchain from that file; no workflow names its own Rust version or channel. A check in the gate fails when a workflow does.
- AC-E1ZJ2M.3: With the pinned toolchain, `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo doc` for the protocol crate pass locally and in CI on all three OSes.
- AC-E1ZJ2M.4: The contributor docs say where the version is pinned and how to move it: edit the file, fix any new lints, open a PR.

## Technical Design

The toolchain file is the single source. In the workflows, a toolchain step reads it (or rustup installs from it on first cargo call) and adds the extra macOS target the universal build needs at install time, not in the file, so other hosts don't download it. The gate check is a small script with tests that scans the workflow files for a hard-coded Rust version or channel.

## Testing

Unit tests for the workflow scan (a workflow with a hard-coded version fails, one without passes). CI on the PR shows the pinned version in the toolchain step on all three OSes.

## Notes

Prompted by M3 and M5: CI moved to clippy 1.99 while local builds were on 1.96, and new lints failed green branches.
