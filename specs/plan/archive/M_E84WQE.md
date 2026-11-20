---
milestone: M_E84WQE
id: fr_01M7W218R6ABPQAFJ5AAE84WQE
status: archived
archived_at: 2026-11-20T12:37:28Z
kickoff_branch: fix/v0.13.2
frozen_at: null
migration: none
codename: Old Boot
---

# Implementation Plan

## M_E84WQE — Old Boot {#M_E84WQE}

**Goal:** Meters on the old 2.1 bootloader are named, refused at once instead of after a 120 s timeout, and diagnosable in one probe run; the Linux permission message works for AppImage users. Ships as v0.13.2.
**Prerequisites:** M_4RN3SF (v0.13.1).
**Status:** FRs written 2026-11-20 from a Linux field report (KM003C, app 1.9.5, bootloader 2.1) and a static read of the vendor app's mode and flash logic.

**Scope:**
- Recognise the 2.1 bootloader shape as an old bootloader; keep the 2.5 bootloader and application mode exactly as today.
- Refuse flash and jump on it at once, and end the switch wait as soon as it appears, with an actionable message.
- A read-only probe that prints endpoints, strings 1 to 4, the timeout direction, and read-only CONNECT/HEAD variants.
- A Linux permission message that needs no source checkout; the .deb installs the rule.

**Out of scope:** flashing the 2.1 bootloader (needs the user's endpoint dump; a later milestone); any new write command; Chinese UI.

**FR list:**

| FR | Title | Tracker |
|---|---|---|
| [JT38C8](../../frs/archive/JT38C8.md) | A meter on the old bootloader (2.1) shows as unknown (bug) | n/a |
| [BKSE70](../../frs/archive/BKSE70.md) | Updating a meter on the old bootloader waits two minutes, then times out (bug) | n/a |
| [QF92WX](../../frs/archive/QF92WX.md) | The firmware probe cannot tell why an old bootloader stays silent (bug) | n/a |
| [31HJ41](../../frs/archive/31HJ41.md) | The Linux permission message points at a file AppImage users do not have (bug) | n/a |

Build order: the table's order (BKSE70 and QF92WX build on JT38C8's mode).

**Tasks:**
1. `DeviceMode::LegacyBootloader`, the classifier, the simulator persona, CLI and desktop labels, protocol docs — JT38C8
   verify: `cargo test -p powerz-protocol -p powerz-sim -p powerz-cli` and `bun run test`.
2. Old-bootloader refusal in flash, jump and the switch wait; desktop update failure; firmware how-to section — BKSE70
   verify: `cargo test -p powerz-protocol --test firmware` and the sim flash tests, plus `bun run test`.
3. Probe endpoints, strings, timeout direction and read-only variants (text and JSON), CLI reference — QF92WX
   verify: `cargo test -p powerz-cli --test commands` and the sim probe tests; hardware runbook under ../qa-runs/.
4. Inline udev command and docs URL in the CLI and desktop messages, .deb rule install, Linux guide — 31HJ41
   verify: `cargo test -p powerz-protocol -p open-powerz-desktop` and `bun run test`.

**Hardware:** a runbook under `../qa-runs/` checks the 2.5 meter end to end (probe in application mode, jump to the bootloader, probe there, jump back) with the new build; a separate test agent runs it. Nothing in this milestone writes to the meter beyond the existing jump.

**Gate:** the CLAUDE.md gating rule.
