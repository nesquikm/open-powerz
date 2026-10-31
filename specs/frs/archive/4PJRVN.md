---
title: Release pipeline fixes
milestone: M_3KPN7D
status: archived
archived_at: 2026-10-31T13:10:58Z
id: fr_01M65KVDYC49QF1V7N1Y4PJRVN
created_at: 2026-10-30T05:17:20.204Z
changelog_category: Fixed
---

# Release pipeline fixes

## Summary

The release workflow publishes every tag, but its first runs showed a few weak spots.
A re-published draft can lose its tag name. Re-running an old release takes the Latest badge from a newer one.
This fixes those and a few smaller checks, and makes sure the oldest supported Rust version still builds.

## Requirement

The release workflow addresses releases by id, keeps tag names and the Latest badge correct on re-runs, and CI checks the declared minimum Rust version.

## Acceptance Criteria

- AC-4PJRVN.1: Every release update sends the tag name, and after publishing the workflow checks that the release's tag equals the tag it built.
- AC-4PJRVN.2: A manual run for a tag older than the current Latest does not take the Latest badge.
- AC-4PJRVN.3: Edit, upload and delete-asset calls address the release by id, not by tag.
- AC-4PJRVN.4: The changelog heading parser rejects a malformed date in place of a codename; the toolchain-pin scan handles an unpaired apostrophe without a false flag.
- AC-4PJRVN.5: The Linux AppImage keeps its executable bit in the release, or the release notes say how to set it.
- AC-4PJRVN.6: A CI job builds and tests with the MSRV declared in `rust-version`, or `rust-version` moves up to a version CI builds.

## Technical Design

Changes in the publish script and its tests, the changelog and toolchain scripts, the release workflow's packaging step, and one CI job.

## Testing

Script unit tests against the fake GitHub API for tag name, Latest and id addressing; the PR dry run; the next real tag is checked by the orchestrator.

## Notes

Source: the M_WBBQ7F M7 follow-ups and the v0.6.0 publish repair.

## Implementation notes

- Version compare ignores -rc suffixes — left for M7b; outside this FR's ACs
- AppImage name is hard-coded in versions.ts — left for M7b; outside this FR's ACs
- Release notes blank lines need a look — left for M7b; outside this FR's ACs
- The msrv job has not run yet — it runs first in CI on this PR
- A tag mismatch leaves the release public — left for M7b; outside this FR's ACs
