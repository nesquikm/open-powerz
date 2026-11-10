---
title: The first PD request after Detect sometimes times out in the browser
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70RPVKDVSA3VKYYT9AHTZW1
created_at: 2026-11-09T18:21:42.000Z
changelog_category: Fixed
kind: bug
---

# The first PD request after Detect sometimes times out in the browser

## Summary

Right after Detect, the first PD request sometimes failed because the meter did not answer in time.
Pressing the same button again worked.
Detect ends on another protocol, so a request first has to enter PD again and wait for the charger's offers.
Now that entry is tried once more before the request fails, which only ever asks for 5 V.
The browser also writes the trigger session's log to the page console, as the desktop app writes its log.

## Requirement

### Steps to reproduce
1. Connect a meter in the web edition with a PD charger on its input.
2. Open Trigger, press Detect, then press a 9 V offer and confirm.

### Expected
The request is sent and the charger moves to 9 V.

### Actual
About one time in five: "Request failed: The meter did not answer in time." and VBUS stays at 5 V; a second press works.

## Acceptance Criteria

- AC-AHTZW1.1: Regression: when entering PD again before a request gets no offers in time, the engine sends the entry once more, and the request goes out once the offers arrive.
- AC-AHTZW1.2: If the second entry also gets no offers, the request fails with the timeout error and nothing above 5 V was sent.
- AC-AHTZW1.3: A request whose own reply does not come is still not sent again.
- AC-AHTZW1.4: In the browser, every line of the trigger session's log reaches the page console with a trigger prefix.

## Technical Design

### Root cause
Inferred: Detect lists every protocol and ends on a legacy one, so the first request enters PD again; that entry's wait for the first offers is an inferred 3 s and the charger is sometimes slower straight after the automatic list. The web session has no log, so the failing wait could not be seen.

### Fix
The engine's PD re-entry before a request retries the entry once on a timeout. The wasm binding sets a log sink that hands each line to the worker, which posts it to the page for the console.

## Testing

Engine tests over a scripted port that answers the second entry only; Vitest for the log lines from the worker to the page console.

## Notes

Found by the web parity HIL re-run (2026-11-09, step P10). The HIL re-run reads the console log to confirm which wait timed out.

## Hardware evidence (parity HIL 3)

- [x] AC-AHTZW1.1, the device half: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P10) the first request after Detect went out (`pd req=2`, 8.69 V); the console shows `entry pd` twice before it.
- [x] AC-AHTZW1.4: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P16) the page console got every `[trigger]` line of the session (P16-trigger-console.txt).
