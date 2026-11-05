---
title: Settings writes and unplugs are detected reliably
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2J
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Fixed
kind: bug
---

# Settings writes and unplugs are detected reliably

## Summary

A settings write sends every display key at its read value.
A corrupted read that passes the weak block check is written back and saved.
This fix reads the block twice and refuses to write if the two reads differ.
It also catches an unplug that surfaces as a timeout.
Replugged meters are matched more carefully, and port errors use plain words.

## Requirement

### Steps to reproduce
1. Corrupt a display field in the simulator's settings reply with bytes that pass the block A check.
2. Change one setting.
3. Separately, unplug a meter so that the transfer ends as cancelled.

### Expected
The write is refused with nothing sent, and the unplug ends the session as an unplug.

### Actual
The wrong field is saved on the meter, and the unplug reads as a timeout.

## Acceptance Criteria

- AC-AXHK2J.1: Regression: `crates/powerz-protocol/src/session/` tests › `settings_write_refuses_when_two_reads_differ` reproduces the failure; it fails before the fix and passes after it.
- AC-AXHK2J.2: A settings write reads the block twice (GET_DATA 0x008 only) and compares all 180 bytes. If they differ it refuses with a clear message and sends no type 0x0A frame. It never retries until two reads match. This holds for the CLI and the desktop app.
- AC-AXHK2J.3: A transfer that ends cancelled while the device is gone from the bus ends the session as an unplug within 1 s. A cancelled transfer with the device still present stays a timeout.
- AC-AXHK2J.4: A replug that lands on a new bus address reconnects the same session when serials match, with a test using a fake listing. A listing match no longer relies on bus, address and PID alone when a serial is known.
- AC-AXHK2J.5: OS errors 6 and 19 at open time count as "meter gone, keep waiting" with one app log line, not as a silent "no meter yet". A test pins the two codes.
- AC-AXHK2J.6: A flash path test covers an Io error followed by re-enumeration.
- AC-AXHK2J.7: On a port held by another program, the trigger and PDM paths say so in plain words, as the settings path does. A vanished port is not reported as held.
- AC-AXHK2J.8: The AC-AXHK2J.3 check never runs for a transfer whose wait is under 100 ms. The firmware erase path's expected 10 ms and 20 ms timeouts neither list USB devices nor pause, so its wire timing does not change.
- AC-AXHK2J.9: After a successful settings write the desktop app says the meter saved the change and that its screen shows it after that setting is opened in the meter's menu. A Playwright test pins it.

## Technical Design

### Root cause
`transfer_error` in `bulk.rs` maps `Cancelled` to `Error::Timeout`, hiding the unplug. Settings writes trust one read whose block A check has about a 1-in-20 false pass.

Add a second read in the session's write path. Map `Cancelled` to its own variant and resolve it through the existing presence check. The double read adds only safe GET_DATA requests.

## Testing

Unit tests with fake transports and the simulator. The tester re-runs a settings write and an unplug on the real meter from a runbook.

## Notes

Source: the M7a settings CRC follow-ups. Timer-sensitive test timings stay as they are.

## Hardware evidence (r4 to r6)

- [x] AC-AXHK2J.3, the unplug half: the AXHK2J run (ec17ae3, 2026-11-02, qa-runs/AXHK2J-20261102-0015) ended 4 of 4 unplugs as an unplug within 1 s (13 to 28 ms from the node going to "Waiting for meter"). r4 P9 logged `unplugged`, `replug`, `connected` with no timeout. A cancelled transfer with the meter still present was not exercised on hardware.
- [x] AC-AXHK2J.4: the same run moved the replug from `usb:02:1` to `usb:01:1`, and it stayed the same session, one tile.
- [x] AC-AXHK2J.7, the held-port half: in the same run `powerz detect` on a held port said the port "is in use by another program" (exit 5). A vanished port was not tried on hardware.
- AC-AXHK2J.5 stays unticked: it cannot be provoked on demand, since OS errors 6 and 19 at open never occurred on the meter and are not to be forced. Its evidence is automated: `crates/powerz-protocol/tests/open_errors.rs` › `os_errors_6_and_19_at_open_mean_the_meter_is_gone`, and `transport/cdc.rs` › `enxio_and_enodev_at_open_are_the_meter_gone` and `another_unknown_open_failure_stays_an_io_error`.
- AC-AXHK2J.9 stays unticked: hardware check carried to the M7c sitting. No settings write has run from the desktop app on a real meter yet. The CLI printed the same note in the AXHK2J run, and the user confirmed the behaviour it describes; a Playwright test pins the app's text.

## Implementation notes

- A vanished port on Windows is told from a held one by a fresh listing, because the OS text is localized.
- The presence check spends at most 800 ms in all. Timeouts of 100 ms or more with the meter present cost up to that.
- Left as they are: the two `serial_error` functions (different error enums), and the held-port branches of `settings_port_error`, which a test pins.
- A listing that loses a known serial reads as gone. Not seen on a stable listing.
- `gone_at_open` does not fire on the macOS nusb path, where `os_error` is an IOReturn value. macOS relies on the kind mapping.
- Hardware run on ec17ae3: AC.2, .3, .4 and .7 passed, on the CLI for the write. AC.5 is simulator-only: errors 6 and 19 at open never occurred on the real meter, and they are not to be forced there.
- The double read lives in the library, so the app and the CLI share it. No app write has run on a real meter.
