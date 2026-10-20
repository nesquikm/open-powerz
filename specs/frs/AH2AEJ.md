---
title: Session history, decimation and statistics
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5DDP0TWKNS6QSV917AH2AEJ
created_at: 2026-10-20T20:07:53.038Z
changelog_category: Added
---

# Session history, decimation and statistics

## Summary

Every session keeps its full sample history in the backend, up to ten million points.
Charts ask for a summary sized to their width, so long recordings stay smooth.
A stats panel shows charge, energy, minimum, maximum, average and elapsed time.
Charge and energy follow the sign of the current, and a reset starts the counters afresh.

## Requirement

Add a per-session sample history with min/max decimation queries and running statistics to the desktop backend, and a stats panel to the UI.

## Acceptance Criteria

- AC-AH2AEJ.1: The backend appends every received queue record to a per-session columnar history stored in raw device units (time, VBUS, IBUS, D+, D−, CC1, CC2); 10 million samples take at most 250 MB, and appending is amortized constant time.
- AC-AH2AEJ.2: `history_range(series, t0, t1, buckets)` returns, per requested series, at most `buckets` buckets of (time, min, max) covering `t0..t1`; the minimum and maximum over all returned buckets equal the minimum and maximum of the raw samples in that range.
- AC-AH2AEJ.3: On a 10-million-sample history, a full-range query with 2000 buckets completes in at most 50 ms in a release build; a release-mode test in CI asserts it.
- AC-AH2AEJ.4: `stats` reports, since the session start or the last reset: elapsed time, charge (Ah) and energy (Wh) integrated from signed current with the same integrator as the CLI recorder, and the min, max and average of VBUS, IBUS and power; on a simulator scenario the values match a reference computed in the test within 1e-9 relative error.
- AC-AH2AEJ.5: `reset_stats` zeroes charge, energy, min/max/avg and elapsed time from the next sample on, and leaves the history untouched; `clear_history` discards the history and is only reachable through a confirmation in the UI.
- AC-AH2AEJ.6: The stats panel shows every value with its unit, refreshes at least 4 times per second while streaming, shows signed charge and energy (for example `−0.120 Ah`), and has a Reset button also bound to the R key.

## Technical Design

Shared integrator from the `powerz-record` crate (FR 0P7HSJ). The `history` module stores `Vec<u32>` time plus raw `i32`/`u16` columns (20 bytes per sample) and an incremental min/max pyramid of `f32` values over blocks of 64·4^k samples. A query picks the coarsest pyramid level giving at least `buckets` blocks in range, merges blocks into buckets and reads raw samples only at the two edges. The `stats` module keeps running sums (compensated summation for averages). Commands `history_range`, `stats`, `reset_stats`, `clear_history`; a stats snapshot also rides every batch. UI: `src/lib/stats.ts` (formatting) and `StatsPanel.svelte`.

## Testing

Property tests compare bucket min/max with a brute-force scan over random data and ranges. A release-mode timing test over 10 million synthetic samples. Integrator parity with the CLI on a simulator scenario. Vitest for the stats formatting.

## Notes

Raw device units keep the history exact, so a session export matches the CLI byte for byte while staying within the memory budget.
