---
title: Browser transports and session driver
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6VHDG2BG62PZDW7K4QFTCWZ
created_at: 2026-11-07T17:39:28.900Z
changelog_category: Added
---

# Browser transports and session driver

## Summary

The web page talks to a real meter through the browser's USB access, picking the meter's vendor interface by its class like the desktop app does.
When the browser cannot claim that interface, the page falls back to the meter's HID interface.
A small driver runs the same steps as the desktop session: connect, sign in, read the device info, stream measurements and capture PD traffic.
Only read and stream commands exist in it; there is no way to send a write.

## Requirement

Add TypeScript transports (WebUSB vendor bulk, WebHID fallback, the wasm demo meter) behind one interface, and a session driver that runs connect, auth, device info, queue streaming and passive PD capture using the wasm builders and decoders.

## Acceptance Criteria

- AC-QFTCWZ.1: A `WebTransport` interface offers `transact(frame, timeoutMs)`, `read(timeoutMs)` and `close()`; it has three implementations: WebUSB, WebHID and the demo meter.
- AC-QFTCWZ.2: The WebUSB transport opens a device with vendor ID 0x5FC9, selects the configuration, picks the interface whose class is 0xFF with one bulk IN and one bulk OUT endpoint (never by interface number), claims it and frames requests as the native bulk transport does.
- AC-QFTCWZ.3: When claiming the vendor interface fails (for example, an OS driver holds it), the driver offers the WebHID transport: 64-byte reports on the HID interface with the same commands; a reply over 64 bytes is assembled from consecutive reports.
- AC-QFTCWZ.4: The driver connects, authenticates, reads device info (model, firmware version, serial), starts the queue at the requested rate, polls it on the native poll period and delivers samples in batches, and stops the queue on disconnect; if authentication fails it falls back to ADC snapshots at the meter's rate, as the native worker does.
- AC-QFTCWZ.5: The driver's command set is closed: a test lists every frame it can emit and proves each is CONNECT, auth, GET_DATA, queue start/stop, PD sniffer start/stop/poll or a memory read on the safe list; there is no API that sends caller-supplied bytes.
- AC-QFTCWZ.6: A transfer error or USB disconnect event ends the session with a `disconnected` error, and a timeout with `timeout`; both reach the caller as `UiError` kinds the UI already handles.

## Technical Design

New `apps/desktop/src/lib/web/` holds `transport.ts` (interface), `webusb.ts`, `webhid.ts`, `demoTransport.ts` (wraps the wasm `DemoMeter`, advancing its clock by wall time between calls) and `driver.ts`. Frames come only from the wasm builders; the driver holds no byte literals. Unknown sections are skipped per the decoders. The poll loop uses `setTimeout`, not busy waits.

## Testing

Vitest runs the driver against the demo transport (the built wasm in Node) and against hand-written fake WebUSB/WebHID device objects for interface selection, claim failure fallback, report assembly, disconnect and timeout paths. The closed-command-set test records every frame the driver sends across all paths.

## Notes

Real-meter behaviour of WebUSB and WebHID is checked later by the HIL tester in the user's Chrome; this FR never touches the meter.
