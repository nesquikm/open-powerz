---
title: A confirmed request in the browser never reaches the trigger engine
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70N4CY9TVW72BW6N9K4DWCF
created_at: 2026-11-09T17:19:12.000Z
changelog_category: Fixed
kind: bug
---

# A confirmed request in the browser never reaches the trigger engine

## Summary

In the web edition a PD request above 5 V failed right after the user confirmed it.
The panel said something went wrong inside the app and the charger stayed at 5 V.
The confirmed request was held in a form the browser cannot pass to the trigger engine's worker.
Now every trigger command hands the worker a plain copy of its values, so the request is sent.

## Requirement

### Steps to reproduce
1. Connect a meter in the web edition with a PD charger on its input.
2. Open Trigger, detect, then press a 9 V offer and confirm.

### Expected
The request is sent and the charger moves to 9 V, as with the desktop app and the CLI.

### Actual
"Request failed: Something went wrong inside the app." and VBUS stays at 5 V; the worker never receives the request.

## Acceptance Criteria

- AC-K4DWCF.1: Regression: a trigger command whose values come from the panel's reactive state (the confirmed pending request) reaches the worker and is answered, with no "Something went wrong" error.
- AC-K4DWCF.2: The worker receives plain values equal to the ones the panel passed, for every trigger command.
- AC-K4DWCF.3: A value that cannot be copied at all still fails the one call with an error and leaves the session usable.

## Technical Design

### Root cause
The panel keeps the pending request in deep reactive state, which wraps it in a proxy. The page posts call arguments to the worker with the structured clone algorithm, which refuses proxies. The RPC client turned that refusal into the generic internal error, so nothing was sent.

### Fix
The RPC client posts a plain deep copy of the arguments (shared memory passes as is), so every command is covered wherever its values come from.

## Testing

Vitest: the RPC client and the web trigger backend over a fake worker whose postMessage structured-clones like a real one, fed a reactive-state proxy.

## Notes

Found by the web parity HIL (2026-11-09, step P10).
