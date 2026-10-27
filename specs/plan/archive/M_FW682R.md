---
milestone: M_FW682R
id: fr_01M5X67KQQR9RNV1E04KFW682R
status: archived
archived_at: 2026-10-27T00:56:11Z
kickoff_branch: null
frozen_at: null
migration: none
codename: Return to Sender
---

# Implementation Plan

## M_FW682R — Release Publish Fix {#M_FW682R}

**Goal:** The release workflow publishes a fresh tag in one run and finishes the v0.6.0 draft left by the first failed run.
**Prerequisites:** M_WBBQ7F (v0.6.0, release workflow).
**Status:** FR written 2026-10-27; ready to implement. Found by the first tag-triggered run (40413437391).

**Scope:**
- Publish by release id taken from the create response; tested publish script with a fake API.

**Out of scope:** everything else on the M7 list.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [4V3DHP](../frs/archive/4V3DHP.md) | Release publish finds its own draft | n/a |

**Tasks:**
- [x] Release publish finds its own draft — 4V3DHP

**M7 follow-ups:**
- (none yet)

**Gate:** same gating rule as CLAUDE.md.
