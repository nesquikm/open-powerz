---
title: powerz command-line tool
milestone: M_34BAXK
status: active
archived_at: null
id: fr_01M5CYB13QGG54679JSMSTQEA1
created_at: 2026-10-20T15:25:32.899Z
changelog_category: Added
---

# powerz command-line tool

## Summary

The powerz command gives scripts and people direct access to the meter from a terminal.
It lists meters, shows device details, and takes one reading or a live view.
It also records high-rate streams to a spreadsheet file.
Its output works both for humans and as JSON for other programs.
It is also our main tool for checking the library against a real meter.

## Requirement

Implement the `powerz` binary in `crates/powerz-cli` on top of `powerz-protocol`, with subcommands `list`, `info`, `read`, `watch` and `stream`, device and transport selection, frame tracing, and stable exit codes.

## Acceptance Criteria

- AC-STQEA1.1: `powerz list` prints one line per attached meter (model, serial, bus/address) and exits 0; with no meter it prints `no POWER-Z devices found` to stderr and still exits 0; `--json` prints a JSON array of objects with `model`, `serial`, `bus` and `address`.
- AC-STQEA1.2: `powerz info` prints model, firmware version, hardware version, serial and UID; `--json` prints them as one JSON object.
- AC-STQEA1.3: `powerz read` takes one ADC snapshot and prints VBUS (V), IBUS (A), power (W), temperature (°C), D+, D−, CC1 and CC2 (V), with 3 decimals except temperature, which has 1 (`30.5 °C`); `--json` prints one object with SI-unit number fields `vbus_v`, `ibus_a`, `power_w`, `temp_c` (null when invalid), `dp_v`, `dm_v`, `cc1_v` and `cc2_v`, written as plain decimals, never in exponent form.
- AC-STQEA1.4: `powerz watch [--interval MS]` refreshes the `read` view in place every MS milliseconds (default 200, minimum 50) until Ctrl-C, then exits 0.
- AC-STQEA1.5: `powerz stream --rate {2,10,50,1000} --csv FILE [--duration SECONDS]` starts the queue at that rate and writes a CSV with the header `ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR`: ElapsedTime as `h:mm:ss.mmm` and Unix as seconds with 3 decimals, both counted from the first received record (an empty poll does not start the clock), numeric columns with at most 8 significant digits as plain decimals (trailing zeros trimmed, never exponent form, e.g. `-0.000000124`), TEMP empty, CHARGE in Ah and ENERGY in Wh integrated from signed current, PWR = VBUS × IBUS. It stops the queue on exit (duration reached or Ctrl-C) and prints the sample count and any gaps to stderr.
- AC-STQEA1.6: With several meters attached, every device command requires `--device SERIAL` and otherwise exits with code 2 naming the serials; `--transport bulk|cdc|hid` selects the pipe (default bulk), and `hid` and `cdc` are accepted only for `read` and `watch` (FW 2.0.6 answers no memory read over CDC, so device info and auth need bulk).
- AC-STQEA1.7: `--trace` writes every frame sent and received to stderr as `>` or `<` followed by hex, and a frame the safety guard refused before sending as `x` followed by hex.
- AC-STQEA1.8: Exit codes are 0 success, 2 usage error, 3 no matching device, 4 permission denied (on Linux the message names the udev rule from the transport FR), and 5 device or protocol error.

## Technical Design

`clap` derive for arguments. Each subcommand is a library function over a `Session` and a `Write` sink, so tests drive it with `MockTransport` and an in-memory buffer; `main` only wires real devices, signals (`ctrlc`) and exit codes. CSV rows are written directly (the `csv` crate is a test-only dependency used to parse them back). `watch` redraws with ANSI cursor movement only when stdout is a terminal, and prints one line per refresh otherwise.

## Testing

Unit tests for formatting, CSV rows, charge/energy integration and elapsed-time formatting. Command-function tests against scripted `MockTransport` sessions for AC .1–.7. `assert_cmd` tests for `--help`, argument errors and exit code 2. A real-device run of each subcommand is a manual step documented in the PR.

## Notes

The CSV layout matches the vendor recorder so files open in the same tools. Desktop CSV export (M2) will reuse this writer.
