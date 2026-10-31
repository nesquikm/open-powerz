---
title: Memory trim at 1000 SPS
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2W
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Changed
---

# Memory trim at 1000 SPS

## Summary

The macOS app's web view settles near 540 MB after six minutes at 1000 samples per second.
Most of that is steady garbage from the chart.
PD status data also grows without limit during long captures.
This change caps the status data and cuts the chart's repeated allocations.
Charts and data stay the same.

## Requirement

Long sessions at 1000 SPS and long PD captures stay within a known memory bound, and the page's plateau drops.

## Acceptance Criteria

- AC-AXHK2W.1: In the shipped macOS app, the web content process stays at or below 400 MB after 6 minutes of a 1000 SPS demo stream. The figure is recorded in the Notes.
- AC-AXHK2W.2: The backend status log and the page's `PdStatusBuffer` are capped at 100,000 samples each. The oldest go first, with a notice, and timeline ranges stay correct across the drop.
- AC-AXHK2W.3: A drop reported by the backend also reaches a page that attaches later.
- AC-AXHK2W.4: Dropping the oldest rows from `pdStore` costs no column shift per row: an append at the cap takes constant time, with a test counting moved elements.
- AC-AXHK2W.5: The overview strip and axis redraws reuse buffers. A test shows no new typed array per frame on a steady stream.
- AC-AXHK2W.6: Parked meters' buffers obey the same caps.
- AC-AXHK2W.7: `e2e/ui/memory.spec.ts` still passes on WebKit in CI with no timing loosened.

## Technical Design

A shared ring for columns replaces array `push` and `shift` in `PdStatusBuffer` and `pdStore`. The backend log becomes a bounded deque. The strip keeps preallocated `Float64Array`s. uPlot axis values get cached per tick set. The cap stays the same as `MAX_RECORDS`.

## Testing

Vitest with element counters, Rust unit tests for caps, the existing memory spec, and a local measurement of the demo stream.

## Notes

Source: inventory note on 4NSVC9 and the follow-up on PD status data. Measured on the demo device, no meter needed.
