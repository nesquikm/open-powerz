---
title: A request after a failed PD entry goes out against old offers
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70RPVKDVSA3VKYYT9AHTZW4
created_at: 2026-11-09T18:21:42.000Z
changelog_category: Fixed
kind: bug
---

# A request after a failed PD entry goes out against old offers

## Summary

When an entry into PD got no offers, the session still counted PD as entered.
The next request then went out at once, checked against the offers from before.
If the charger now offers something else, that request could ask for the wrong offer.
Now PD counts as ready for a request only after fresh offers arrived, so the next press enters PD again first.

## Requirement

### Steps to reproduce
1. In a trigger session, enter another protocol, then press a PD offer so the engine enters PD again.
2. Let that entry get no offers, then press a PD offer again.

### Expected
The second press enters PD again and checks the request against the offers it gets.

### Actual
The second press sends the request straight away, checked against the offers from the earlier negotiation.

## Acceptance Criteria

- AC-AHTZW4.1: Regression: after an entry into PD that got no offers, the next PD request enters PD again before it writes anything.
- AC-AHTZW4.2: PD counts as ready for a request only once offers arrived after the last entry, reset, hard reset, open or close; anything else leaves it not ready.
- AC-AHTZW4.3: A request is checked against the offers of the latest negotiation; a position those offers lack is refused before anything is written.
- AC-AHTZW4.4: The way back to 5 V still treats a written entry into PD as possibly in PD, as before.

## Technical Design

### Root cause
The PDM session records a protocol as entered when the entry is written, which the way back to 5 V needs; the request path read the same flag as "offers are fresh".

### Fix
The session keeps a separate PD-ready state, set by a finished negotiation and cleared by every entry, reset, hard reset, open and close write. The request path enters PD unless it is ready; the way back to 5 V keeps using the written-entry flag.

## Testing

powerz-protocol tests over a scripted port; trigger engine tests for the desktop and browser path; the existing desktop trigger tests stay green.

## Notes

Found by the audit of AHTZW1 (2026-11-09). Safety: a request must never be judged against offers from before the current negotiation.
