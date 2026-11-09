---
title: Release builds fail when the bundler tooling download times out
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHXE
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Fixed
kind: bug
---

# Release builds fail when the bundler tooling download times out

## Summary

The desktop release build downloads its packaging tools while it runs.
Three times in one day that download got a gateway timeout and broke the Linux and Windows builds.
The release workflow now keeps those tools in the build cache.
It also retries the packaging step once, so a slow download no longer breaks a release.

## Requirement

### Steps to reproduce
1. Push a release tag while the tool host is slow.

### Expected
The Linux AppImage and Windows installer builds finish.

### Actual
The bundler fails with http status 504 while downloading its tooling (2026-11-09, three runs).

## Acceptance Criteria

- AC-W8SHXE.1: Regression: the release workflow restores and saves the bundler's tool folder on Linux and Windows, keyed on the runner OS and the Tauri CLI version from the lockfile.
- AC-W8SHXE.2: When the packaging step fails, it runs once more before the job fails; a second failure still fails the job.
- AC-W8SHXE.3: The workflow still pins every action by commit hash, and the workflow lint test passes.

## Technical Design

### Root cause
Tauri's bundler fetches linuxdeploy, the AppImage runtime, WiX and NSIS on every run into its cache folder, which the job never caches.

### Fix
An actions cache step on that folder before the build, and a retry step after a failed first build.

## Testing

The existing workflow Vitest checks extended for the cache and retry steps.

## Notes

Only the release workflow changes.
