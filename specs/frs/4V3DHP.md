---
title: Release publish finds its own draft
milestone: M_FW682R
status: active
archived_at: null
id: fr_01M5X67KQRK0874KKS9X4V3DHP
created_at: 2026-10-26T22:45:23.832Z
changelog_category: Fixed
kind: bug
---

# Release publish finds its own draft

## Summary

The first real release, version 0.6.0, built and uploaded every file but stopped before publishing.
It left a draft behind instead of a published release.
Right after creating the draft, the workflow looked it up again in the release list.
That list did not show the new draft yet.
The workflow must keep the identity of the draft it created and use it for every later step.
Then a fresh tag ends in a published release.

## Requirement

The release workflow's publish job publishes a new release for a fresh tag in one run, and finishes a release whose earlier run stopped at the draft.

### Steps to reproduce
1. Push a fresh `v*` tag so `.github/workflows/release.yml` runs (first seen with v0.6.0, run 40413437391).
2. Let the build jobs finish and the publish job create the draft and upload the files.

### Expected
The publish job finds the draft it just created, checks its assets and publishes it.

### Actual
The publish job fails with `gh: Not Found (HTTP 404)` after the uploads; the release stays a draft.

## Acceptance Criteria

- AC-4V3DHP.1: When the publish job creates the draft, it takes the release id from the create response and uses that id for the asset check, the asset cleanup and the final publish; it never looks the new draft up again.
- AC-4V3DHP.2: When a release for the tag already exists (for example the v0.6.0 draft left by the failed run), the job finds it by tag in the release list, takes its id the same way, and finishes it.
- AC-4V3DHP.3: An empty or malformed release id stops the job with a clear message before any API call uses it; no request goes to a URL with an empty id.
- AC-4V3DHP.4: The publish logic lives in a script with tests that run against a fake GitHub API: a fresh tag, an existing draft, a list that lags behind a create, two releases for one tag, and an upload that leaves a file missing.
- AC-4V3DHP.5: After the merge, a manual run for v0.6.0 publishes that release from its draft, and pushing the next tag publishes in one run. Both are checked from the published files: the macOS app opens and shows its version, and the CLI prints it.
- AC-4V3DHP.6: Regression: a publish-script test replays the v0.6.0 failure (the release list lags behind the create); it fails against the old lookup and passes after the fix.

## Technical Design

### Root cause
The job runs the release list query right after it creates the draft. The listing did not include the new draft yet, so the id came back empty and the assets request went to a URL with no id (HTTP 404). Fix: create the draft through the releases API and read its id from the response, then address the release by id. Move the step into a tested script beside the other release scripts.

## Testing

Unit tests with a fake API cover the cases in AC 4. The dry run on the PR still skips publishing; AC 5 is checked by the orchestrator after the merge.

## Notes

Failed run: 40413437391 (tag v0.6.0, 2026-10-27). All nine files are on the v0.6.0 draft; it was left unpublished on purpose so the fixed workflow can finish it.
