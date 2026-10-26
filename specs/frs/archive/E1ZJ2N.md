---
title: Release builds on a version tag
milestone: M_WBBQ7F
status: archived
archived_at: 2026-10-26T21:32:10Z
id: fr_01M5WNDAHHETCCVHSJK4E1ZJ2N
created_at: 2026-10-26T17:51:25.233Z
changelog_category: Added
---

# Release builds on a version tag

## Summary

Pushing a version tag builds open-powerz for macOS, Windows and Linux and publishes a GitHub Release.
The release has the desktop installers, the command line tool for each system and a checksum file.
Its notes are the changelog section for that version.
Builds are unsigned for now, so an install guide explains the system warnings users will see.
A pull request that touches the release setup runs the same builds as a dry run that publishes nothing.

## Requirement

A GitHub Actions workflow turns a pushed version tag into a published GitHub Release with unsigned desktop installers and command line binaries for all three operating systems, and its notes come from the changelog.

## Acceptance Criteria

- AC-E1ZJ2N.1: Pushing a tag of the form `vX.Y.Z` (optionally with a pre-release suffix such as `-rc.1`) starts the release workflow. A manual run takes an existing tag as input and builds that tag, so a release can be redone or created for a tag that predates the workflow.
- AC-E1ZJ2N.2: Before any build, a check job fails the run with a clear message when the tag version differs from the workspace Cargo version, the desktop package.json or tauri.conf.json, or when CHANGELOG.md has no section for that version.
- AC-E1ZJ2N.3: The desktop app is built with tauri-action as a macOS universal .dmg (Apple silicon and Intel), a Windows .msi and NSIS .exe, and a Linux AppImage and .deb. Nothing is signed or notarized, and no signing secret is needed.
- AC-E1ZJ2N.4: The `powerz` CLI is built in release mode for macOS (universal), Windows x86_64 and Linux x86_64, and packed as .tar.gz (macOS, Linux) or .zip (Windows) with the LICENSE and README.
- AC-E1ZJ2N.5: The release is titled with the version and codename, and its notes are that version's CHANGELOG section, extracted by a tested script. A tag with a pre-release suffix gives a pre-release.
- AC-E1ZJ2N.6: A SHA256SUMS.txt covering every attached file is attached as well.
- AC-E1ZJ2N.7: The release appears only when every build succeeded: it is created as a draft, all files are attached, then it is published. A failed build leaves no published release. Re-running for the same tag replaces the files instead of failing or duplicating them.
- AC-E1ZJ2N.8: A pull request that changes the release workflow, its scripts or the Tauri bundle config runs the workflow as a dry run: the same builds, files uploaded as workflow artifacts, no release and no tag.
- AC-E1ZJ2N.9: The workflow has read-only permissions by default; only the publish job gets write access to contents. Every third-party action is pinned to a commit SHA, and the only token used is the built-in GITHUB_TOKEN.
- AC-E1ZJ2N.10: An install guide in docs/how-to covers downloading each file, opening the unsigned app on macOS (Gatekeeper) and Windows (SmartScreen), the Linux AppImage and .deb with the udev rule from the Linux docs, and checking SHA256SUMS. The README links to the latest release and the guide.
- AC-E1ZJ2N.11: The dry run on this milestone's PR is green on all three OSes, and its macOS .dmg and CLI are smoke-tested on this Mac: the app opens and shows its version, and `powerz --version` prints it. The first tagged release after merge is checked the same way from the published files.

## Technical Design

One workflow file, release.yml, with jobs: check (version and changelog), desktop (matrix of three OSes via tauri-action), cli (matrix, lipo for the macOS universal binary), publish (draft, attach, checksums, publish). Linux builds run on the oldest supported Ubuntu runner for wider glibc reach. Small scripts with unit tests extract the changelog section and check versions. The publish job is kept separate so the web edition can later add a GitHub Pages deploy job beside it.

## Testing

Unit tests for the changelog extraction (normal, pre-release, missing section, last section in file) and the version check (match and each mismatch). The dry run on the PR is the integration test; AC 11 covers the manual smoke test.

## Notes

User decision 2026-10-26: unsigned for now (no Apple Developer account or Windows certificate); signing comes later.

## Implementation notes

- publish step edits, uploads and deletes assets by tag rather than by release id — round 2; current gh resolves drafts by tag; M7 follow-up to switch to gh api by id
- changelog heading parser takes a malformed date as an unquoted codename — round 2; cosmetic and unlikely; M7 follow-up
