---
title: Protocol triggering in the browser
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHXC
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Added
---

# Protocol triggering in the browser

## Summary

The trigger panel now works in Chrome and Edge through Web Serial on the meter's virtual serial port.
It offers the same detection, offers, requests, PD messages and console as the desktop app.
The same trigger engine runs in both, so every safeguard is the same.
Anything above 5 V still asks first, and leaving the panel goes back to 5 V.

## Requirement

The web edition runs the desktop app's trigger engine, unchanged, compiled to WebAssembly, over the meter's serial port opened with Web Serial. The browser asks for the port the first time and reuses it after.

## Acceptance Criteria

- AC-W8SHXC.1: Every trigger command the desktop app offers (open, detect, list, risk, request, armed, disarm, charger gone, PD entered, back to 5 V, close, message risk and send, cable read, console risk and send) works in the browser with the same answers and errors for the same port traffic.
- AC-W8SHXC.2: The trigger engine is one Rust implementation used by the desktop app and the browser; the desktop app's trigger tests still pass.
- AC-W8SHXC.3: A request above 5 V is refused unless confirmed, or covered by "don't ask again", which only a PD request of at most 20 V can set and which ends at 5 V, a disconnect or any event that ends it on the desktop; EPR requests and console lines always need a confirmation.
- AC-W8SHXC.4: Leaving the trigger view, closing the session, disconnecting the meter or closing the page tab returns the charger to 5 V where the protocol allows it and sends pdm close, as far as the browser lets the page finish.
- AC-W8SHXC.5: The VBUS watch (charger gone, back under 5 V) gets the session's VBUS readings as on the desktop.
- AC-W8SHXC.6: The serial port is picked as in the settings change (granted port reused, plain message on cancel, busy port or no Web Serial) and shared with it: a settings change waits for an open trigger session or is refused as on the desktop.
- AC-W8SHXC.7: The web build switches the trigger feature on; a HID session can still trigger, since triggering uses only the serial port.

## Technical Design

The engine moves from the desktop crate into a shared crate behind a byte-port and clock interface. In the browser it runs in a dedicated worker with a blocking port: the page owns the Web Serial port and answers the worker's reads and writes through shared memory, so the page must be cross-origin isolated; a small service worker adds the isolation headers on GitHub Pages.

## Testing

The moved engine keeps its Rust tests; wasm tests over a scripted port; Vitest for the bridge and backend commands over a fake serial port; desktop trigger e2e unchanged.

## Notes

Above-5 V triggering on the real meter is approved only in the wall-charger setup (laptop on the data port, output empty).

Closing the tab is best effort: the page tries; a closed tab cannot wait. Above 5 V the browser asks before the tab closes.
