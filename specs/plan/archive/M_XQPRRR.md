---
milestone: M_XQPRRR
id: fr_01M84XJY85807KH8RP21XQPRRR
status: archived
archived_at: 2026-11-23T22:36:40Z
kickoff_branch: fix/bl21-own-ids
frozen_at: null
migration: none
codename: Own Ids
---

# Implementation Plan

## M_XQPRRR — Own Ids {#M_XQPRRR}

**Goal:** The old 2.1 bootloader, which numbers its replies with its own counter, probes clean and can be flashed behind an explicit experimental flag. Ships as v0.14.0 (a new CLI flag and public API make it a minor release).
**Prerequisites:** M_E84WQE (v0.13.2).
**Status:** FRs written 2026-11-23 from the second field report (v0.13.2 `--trace firmware probe` on KM003C, app 1.9.5, bootloader 2.1). Reopened 2026-11-24 for NCH7DW after an independent review of PR #22.

**Scope:**
- Tie replies to requests by type on the old bootloader, with a longer first CONNECT and no pipe recovery; strict ids everywhere else.
- The probe names mismatched reply ids instead of reporting a timeout or a bare "accepted".
- A simulator mode that replays the field trace, and an experimental flash on the old bootloader driven end to end on it.
- Protocol doc, firmware how-to, CLI reference; the field note outside the repo.

**Out of scope:** flashing the old bootloader from the desktop app; jumping into or out of the old bootloader; any real-meter flash or jump in this milestone.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [XW4QQN](../../frs/archive/XW4QQN.md) | The old bootloader answers, but with its own reply ids, so we call it silent (bug) | n/a |
| [DJYNRP](../../frs/archive/DJYNRP.md) | Flash a meter on the old bootloader, experimentally | n/a |
| [NCH7DW](../../frs/archive/NCH7DW.md) | The experimental old-bootloader flash refuses a meter it just switched, and overstates what we know (bug) | n/a |

Build order: the table's order (DJYNRP reuses XW4QQN's reply-id policy and sim mode; NCH7DW fixes DJYNRP after review).

**Tasks:**
1. Session reply-id policy, legacy CONNECT timing, id-mismatch reporting in the probe (text and JSON), sim own-ids mode, NUL-cut strings, Linux busy hint URL, protocol doc — XW4QQN
   verify: `cargo test -p powerz-protocol -p powerz-sim -p powerz-cli -p open-powerz-desktop`.
2. Experimental old-bootloader flash: ack flag, download id policy, CLI flag and warning, refusal text (Rust and TS), how-to and CLI reference — DJYNRP
   verify: `cargo test -p powerz-protocol -p powerz-sim -p powerz-cli` and `bun run test`.
3. Review fixes: continue into the old-bootloader download after JUMP_DFU with the opt-in, accurate refusal without it, hedged safety wording, protocol-doc retags, warning only on the old path, HEAD stray overflow as a read error, NUL-cut JSON strings, sim-mode usage text — NCH7DW
   verify: `cargo test -p powerz-protocol -p powerz-sim -p powerz-cli -p open-powerz-desktop` and `bun run test`.

**Hardware:** none. The bootloader 2.5 path is unchanged and covered by the existing engine tests; nothing in this milestone is sent to a real meter.

**Gate:** the CLAUDE.md gating rule.
