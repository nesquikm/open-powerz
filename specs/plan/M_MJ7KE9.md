---
milestone: M_MJ7KE9
id: fr_01M5PHM08GH7R61TR259MJ7KE9
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Flash Gordon
---

# Implementation Plan

## M_MJ7KE9 — Firmware Update {#M_MJ7KE9}

**Goal:** Update the meter's application firmware from a local vendor file, safely: read-only device and file inspection first, then a flash engine that never writes the bootloader, a guarded desktop screen, and staged checks on the real meter.
**Prerequisites:** M_34BAXK (session, memory reads); M_R4WG4Q (desktop shell); M_X4VEQ3 (safety-gate patterns).
**Status:** FRs written 2026-10-24; ready to implement. User decision 2026-10-24: build M5 now, staged; every real-meter stage needs the user's go-ahead at that time.

**Scope:**
- Mode detection (application or bootloader), version read-out and old-firmware warnings.
- Firmware container inspection: metadata, model check, no decryption, no vendor files in the repo.
- Flash engine: JUMP_DFU/JUMP_APROM, the vendor sequence over bulk, a 0x4000 floor, typed errors, no automatic retry, version read-back; simulated bootloader.
- Desktop update screen behind an advanced switch and a typed confirmation.
- Staged real-meter checks (discovery, JUMP round trip, 2.0.6 → 2.0.7), each run by a fresh test agent from a runbook.

**Out of scope:** the vendor's online update channel, bundling firmware, decrypting firmware, OTP, calibration and factory paths.

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [V9SK52](../frs/V9SK52.md) | Device mode and firmware version | n/a |
| [V9SK53](../frs/V9SK53.md) | Firmware file inspection | n/a |
| [V9SK54](../frs/V9SK54.md) | Firmware flashing engine | n/a |
| [V9SK55](../frs/V9SK55.md) | Desktop firmware update | n/a |
| [V9SK56](../frs/V9SK56.md) | Staged firmware checks on the real meter | n/a |

Build order: V9SK52, V9SK53, V9SK54, V9SK55, then V9SK56 (stage 1 can run once V9SK52 is done).

**Tasks:**
- [ ] Device mode and firmware version — V9SK52
- [x] Firmware file inspection — V9SK53
- [x] Firmware flashing engine — V9SK54
- [ ] Desktop firmware update — V9SK55
- [ ] Staged firmware checks on the real meter — V9SK56

**M7 follow-ups:**
- `.claude/settings.json` has no `Bash(claude:*)` allow rule, so gate probe `spawn_pattern_allowlist` fails. It has failed since the bootstrap commit, and the fix is a settings change only the user can approve.
- The desktop UI copies the firmware thresholds (1.7.0, 2.0.7) by hand from the Rust constants. Have the backend send them, or pin both sides with one test (V9SK52 review).
- `Version::parse` is strict: `v2.0.7` or `2.0.7-beta` give no warning. The meter reports plain `2.0.6`; settle a policy if other strings appear (V9SK52 review).
- The firmware notice's live region mounts with its text already in it, so screen readers may not announce it (V9SK52 review).
- `device_info()` reads the info blocks with `?`, so a block that fails there still aborts `powerz info` and the desktop connect before the tolerant status read, and each connect reads the blocks twice. Make it reuse the status read (V9SK52 HEAD fix review).

**Device safety:** JUMP_DFU (09), JUMP_APROM (08), the 0x42 descriptor and payload writes stay forbidden on the real meter until the user approves the stage that needs them.

**Gate:** same gating rule as CLAUDE.md.
