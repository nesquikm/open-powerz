---
title: PD capture save and load
milestone: M_NE3C7K
status: archived
archived_at: 2026-10-22T20:50:59Z
id: fr_01M5HVWJ41WV7P8G50RPTGE83E
created_at: 2026-10-22T13:12:57.217Z
changelog_category: Added
---

# PD capture save and load

## Summary

A capture is only useful later if it can be saved and opened again.
This feature saves PD captures in an open, documented file format and loads them back exactly.
The command line tool can save a live capture and decode a saved file.
The meter's serial number is never written into the file.
Opening the vendor's own PD log database is included as a read-only import.

## Requirement

PD captures save to and load from a documented open format without loss, and vendor PD log databases can be imported read-only.

## Acceptance Criteria

- AC-TGE83E.1: Save writes a pdcap file in JSON Lines: a header line with the format version, app version, meter model and firmware version, then one line per status sample or record with its session time and raw bytes in hex. The meter's serial number is never written.
- AC-TGE83E.2: Loading a saved file restores the packet table, the details and the timeline exactly as they were, which a round-trip property test checks.
- AC-TGE83E.3: A truncated or corrupt file loads every valid line before the damage and reports the line number where reading stopped.
- AC-TGE83E.4: The command line tool saves a live capture with a save option on the pd command, and decodes a saved file with a decode subcommand.
- AC-TGE83E.5: The format is documented in the reference docs with a worked example.
- AC-TGE83E.6: A vendor PD log database with the pd_table layout from the protocol notes imports read-only into the analyzer view, tested with a database built from that layout.

## Technical Design

A small format module next to the capture core reads and writes the lines with a streaming reader. The desktop app's save and open dialogs reuse the existing export notices. The vendor import opens the database read-only with an embedded SQLite library, maps its relative times to session time, takes pd_chart rows as status samples, and skips rows whose raw bytes are not exactly one record or exceed 4 KiB, counting them in its notice. An opened or imported capture is shown in place of the live one, without a meter; Save stays off while it is shown, and table rows are numbered from the last Clear.

## Testing

Round-trip property tests, truncation tests at every byte offset of a small file, CLI tests for save and decode, and an import test with a generated database.

## Notes

A vendor pdStream import is out of scope.

## Implementation notes

- opening a large .pdcap collects every entry and sends it in one IPC message; no entry cap — lines are now capped at 64 KiB; a record-count cap for opened files is an M7 follow-up
- `pd decode` reports a damaged file with exit code 5 (the device-error code) — the CLI has no file-error code yet; on the M7 follow-ups
- record_from is written twice (pdcap and the vendor import) — on the M7 follow-ups
- a .pdcap damaged on its first entry is refused instead of opening empty with the damage notice — the error names the line; on the M7 follow-ups
- an automatic reconnect after a replug does not release an opened capture's memory — the Connect command does; on the M7 follow-ups
- a CLI-saved and a desktop-saved file of the same capture differ by a constant session_ms offset — documented in docs/reference/pdcap.md
- the desktop save's off-thread write has no timing test (a slow fsync during polling) — the worker only answers a snapshot command now, which a unit test pins
