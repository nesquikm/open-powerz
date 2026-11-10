---
milestone: M_NA20AD
id: fr_01M72933WSKAVJGFANJRNA20AD
status: active
archived_at: null
kickoff_branch: fix/v0.12.1
frozen_at: null
migration: none
codename: Loose Ends
---

# Implementation Plan

## M_NA20AD — Loose Ends {#M_NA20AD}

**Goal:** Make the three flaky e2e tests deterministic and fix the v0.12.0 known issues and advisories, for v0.12.1.
**Prerequisites:** M_ZFZQE7 (v0.12.0).
**Status:** FRs written 2026-11-10 from qa-runs/web/fixes12-brief.md and CI runs 42773524151, 42769569733, 42683371695.

**Scope:**
- The reload, timeline and PD analyzer e2e flakes (the last one is a real WebKit follow bug).
- Cancel during a large Open recording parse in the browser.
- TEMP in the first rows of a session: match the desktop and document it.
- The CLI watch over CDC while the browser streams over WebUSB.
- The recording rows type, and the unasked PD offer list.
- An independent docs review: README and docs match the app; new README screenshot.
- Docs review follow-ups (PR #18): stats scope under a zoomed File chart, the live chart buffer wording, one name for the web edition.

**Out of scope:** the Import vendor log known issue (no real vendor file exists; stays in Known issues); any meter command outside the CLI CDC read path; meter runs (a HIL run follows).

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [3B70S3](../frs/3B70S3.md) | The live windows test waits on wall time for the demo stream (bug) | n/a |
| [3B70S4](../frs/3B70S4.md) | The zoom readout test compares against a stale span (bug) | n/a |
| [3B70S5](../frs/3B70S5.md) | A wheel scroll up in a live PD capture can snap back to the bottom (bug) | n/a |
| [3B70S6](../frs/3B70S6.md) | Cancel cannot interrupt a large Open recording parse in the browser (bug) | n/a |
| [3B70S7](../frs/3B70S7.md) | The first rows of a session have no TEMP (bug) | n/a |
| [3B70S8](../frs/3B70S8.md) | The CLI watch over CDC exits while the browser streams (bug) | n/a |
| [3B70S9](../frs/3B70S9.md) | The recording rows type lacks the TEMP field (bug) | n/a |
| [3B70SA](../frs/3B70SA.md) | An unasked PD offer list does not refresh the ready offers (bug) | n/a |
| [SHTZHQ](../frs/SHTZHQ.md) | The user docs lag behind the app (bug) | n/a |
| [PBDEQE](../frs/PBDEQE.md) | The File tab stats ignore the zoomed range (bug) | n/a |
| [2VPF6P](../frs/2VPF6P.md) | The recorder's chart memory line reads like the session history (bug) | n/a |
| [Z9PC5A](../frs/Z9PC5A.md) | The README says browser edition where the rest says web edition (bug) | n/a |

Build order: the table's order (independent).

**Tasks:**
1. Hold and step the page clock in the reload live-window test — 3B70S3
   verify: reload spec `--repeat-each=20 --workers=1` on WebKit and Chromium.
2. Read the zoom readout and the slider value together — 3B70S4
   verify: the timeline test `--repeat-each=20 --workers=1` on both browsers.
3. An upward wheel pauses following — 3B70S5
   verify: `bun run test` plus the PD analyzer spec `--repeat-each=20 --workers=1`.
4. Stepped CSV reader in wasm and a sliced, cancellable web read — 3B70S6
   verify: `cargo test -p powerz-wasm` and `bun run test`.
5. Pin the TEMP rule in both editions, add the debug line, document it — 3B70S7
   verify: `bun run test` and `cargo test -p open-powerz-desktop`.
6. Demux every CDC read; watch over CDC falls back to snapshots — 3B70S8
   verify: `cargo test -p powerz-protocol -p powerz-cli`.
7. Type the recording rows' TEMP — 3B70S9
   verify: `bun run check` and `bun run test`.
8. Refresh the ready PD offers from an unasked block — 3B70SA
   verify: `cargo test -p powerz-protocol -p powerz-trigger`.
9. Bring the README and docs in line with the app, new screenshot — SHTZHQ
   verify: `bun run test`, `cargo test --workspace`, the link check.
10. Range stats gain min and max; File tab scope label and toggle; live panel label — PBDEQE
   verify: `cargo test -p open-powerz-desktop`, `bun run test`, the recording-file Playwright spec on WebKit.
11. Name the live chart buffer in the recorder options and the how-tos — 2VPF6P
   verify: `bun run test`.
12. Web edition everywhere in the README and docs — Z9PC5A
   verify: `bun run test`.
