---
milestone: M_4RN3SF
id: fr_01M73H7QMYN42VHAZ3W14RN3SF
status: archived
archived_at: 2026-11-10T20:51:44Z
kickoff_branch: fix/v0.13.1
frozen_at: null
migration: none
codename: Home Wave
shipped_in: v0.13.1
---

# Implementation Plan

## M_4RN3SF — Home Wave {#M_4RN3SF}

**Goal:** After a replug the meter comes back instead of the demo, and the built-in logos sit on the meter without a box, for v0.13.1.
**Prerequisites:** M_SPYPSC (v0.13.0).
**Status:** FRs written 2026-11-11 from qa-runs/web/fix131-brief.md.

**Scope:**
- No silent demo fallback after a replug; a not-connected meter stays picked, with Refresh.
- The demo is named as the demo in tile headers and next to every write.
- Both built-in logos end their sea inside the 200 by 200 frame.

**Out of scope:** any meter command; a larger logo; protocol changes.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [NWGY72](../../frs/archive/NWGY72.md) | After a meter replug, Connect silently uses the demo device (bug) | n/a |
| [BZJGJ5](../../frs/archive/BZJGJ5.md) | The built-in startup logos are cut by their frame (bug) | n/a |

Build order: the table's order.

**Tasks:**
1. The session keeps the chosen meter (not-connected stand-in, Refresh), names the demo, and names each write's target — NWGY72
   verify: `bun run test` and the device-picker Playwright spec on WebKit.
2. Borderless built-in logos, re-rendered with previews — BZJGJ5
   verify: `cargo test -p powerz-protocol --test logo_builtin --features image-files`.

**Gate:** the CLAUDE.md gating rule.
