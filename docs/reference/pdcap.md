# pdcap file format

A `.pdcap` file holds one USB PD capture: the meter's status samples and the
PD records it reported. The format is open and plain text, so any tool can read
it. The `powerz` CLI writes it with `powerz pd --save FILE` and reads it with
`powerz pd decode FILE` (see the [CLI reference](cli.md)). The desktop app saves
and opens the same files.

The file is [JSON Lines](https://jsonlines.org/): UTF-8 text, one JSON object
per line, each line ended by `\n`. Line 1 is the header. Every further line is
one entry: a status sample or a record, in the order they were captured.
Blank lines are skipped.

The meter's serial number (and its UID) is never written. No field in the
format holds it, so a file is safe to share.

## Header line

| field | type | meaning |
|---|---|---|
| `format` | string | Always `"pdcap"`. |
| `format_version` | integer | Version of the format. This document describes version `1`. |
| `app_version` | string | Version of the app that wrote the file, for example `"0.2.0"`. |
| `model` | string | Meter model, for example `"KM003C"`. |
| `firmware_version` | string | Meter firmware version, for example `"2.0.6"`. |

## Entry lines

Every entry line has a `kind` field: `status` or `record`.

All times are whole milliseconds. `session_ms` is the time on the session
time axis of the app that wrote the file, the one its timeline uses. Each app
uses its own time base: the CLI writes the meter's own clock, the desktop app
writes its session time (the time axis of its meter readings). The same
capture saved by both can therefore differ by a constant offset in
`session_ms`; the gaps between entries, `elapsed_ms` and `raw` are the same.
Compare captures by their spacing, not by the absolute value.

`raw` holds the entry's bytes exactly as the meter sent them, as hex: two
digits per byte, no separators. Writers use lowercase digits; readers accept
either case. Readers decode everything else they show from `raw`.

### Status line

| field | type | meaning |
|---|---|---|
| `kind` | string | `"status"` |
| `session_ms` | integer | Session time of the sample, ms. |
| `raw` | hex string | The 12-byte status block. |

The status block is little-endian:

| bytes | type | meaning |
|---|---|---|
| 0..4 | u32 | meter time, ms |
| 4..6 | u16 | VBUS, mV |
| 6..8 | i16 | IBUS, mA (signed) |
| 8..10 | u16 | CC1, mV |
| 10..12 | u16 | CC2, mV |

### Record line

| field | type | meaning |
|---|---|---|
| `kind` | string | `"record"` |
| `session_ms` | integer | Session time of the record, ms. |
| `elapsed_ms` | integer | Time since the capture's first record, ms. |
| `raw` | hex string | One whole PD record, its first byte included. |

The first byte of a record (`b0`) gives its type and length:

- Bit 7 set: a PD message, `(b0 & 0x3F) + 1` bytes.
- Bit 5 set: a big message, `(b0 & 0x1F) + 1` bytes plus the u16 at offset 6.
- Otherwise: an event, `(b0 & 0x1F) + 1` bytes.
- Bit 6 set: the meter sent the message itself.

Bytes 1..5 are the meter time (u32, ms, little-endian). For a message, byte 5 is
the SOP (0 = SOP, 1 = SOP', and so on), bytes 6..8 the 16-bit PD header
(little-endian) and the rest the data objects. For an event, byte 5 is the
event: low nibble the code (1 attach, 2 detach, 3 bad message ID, 4 hard reset,
5 cable reset, 6 receive error, 7 monitor type, 8 big message), high nibble its
parameter (for attach and detach, 1 = CC1 and 2 = CC2). The
[USB protocol reference](../protocol.md#pd-capture) has the details.

## Worked example

A short capture from the demo device (`powerz --transport sim --scenario
pd-9v-2a pd --save capture.pdcap`), trimmed:

```jsonl
{"format":"pdcap","format_version":1,"app_version":"0.2.0","model":"KM003C","firmware_version":"2.0.6"}
{"kind":"status","session_ms":15,"raw":"0f0000008813000000000000"}
{"kind":"status","session_ms":20,"raw":"1400000088130000f8050000"}
{"kind":"record","session_ms":20,"elapsed_ms":0,"raw":"051400000011"}
{"kind":"status","session_ms":300,"raw":"2c01000088130000f8050000"}
{"kind":"record","session_ms":300,"elapsed_ms":280,"raw":"972c01000000a1412c9181082cd102002cb10400f4410600"}
{"kind":"record","session_ms":302,"elapsed_ms":282,"raw":"8b2e0100000082102cb14410"}
{"kind":"record","session_ms":304,"elapsed_ms":284,"raw":"873001000000a303"}
```

Line by line:

1. The header: format version 1, written by app 0.2.0 for a KM003C on firmware
   2.0.6. There is no serial number.
2. Status at 15 ms: meter time `0f000000` = 15 ms, VBUS `8813` = 5000 mV,
   IBUS 0 mA, CC1 and CC2 0 mV.
3. Status at 20 ms: CC1 is now `f805` = 1528 mV.
4. An event record: `b0` = `05` (event, 6 bytes), meter time `14000000` = 20 ms,
   event `11` = attach on CC1. It is the first record, so `elapsed_ms` is 0.
5. Status at 300 ms: still 5 V on VBUS.
6. A PD message: `b0` = `97` (message, 24 bytes), meter time 300 ms, SOP 0,
   PD header `a141` = 0x41A1, then four data objects. `powerz pd decode` prints
   it as `280 ms  line  SOP  header 0x41a1 2c9181082cd102002cb10400f4410600`.
7. and 8. Two more messages, 2 ms apart: PD headers 0x1082 and 0x03A3.

## Damaged files

A reader takes every valid line up to the first damaged one, then stops and
reports that line's number (the header is line 1). A line is damaged when it:

- is longer than 64 KiB (an entry line is a few hundred bytes); the reader
  stops there without reading the rest of that line;
- is not UTF-8 text or not a JSON object of the shape above;
- has a `kind` other than `status` or `record`;
- has a `raw` that is not whole hex bytes;
- is a status line whose `raw` is not exactly 12 bytes;
- is a record line whose `raw` is not exactly one whole record.

A truncated file therefore loads everything before the cut. `powerz pd decode`
prints those records, then the error with the line number, and exits with
code 5. The desktop app shows the loaded part and names the line.

A file whose first line is missing, is not a header, or has a `format` other
than `pdcap` is not loaded at all.

## Compatibility

Readers ignore fields they do not know. A writer may add fields without a new
version, as long as older readers stay correct without them. Any change an
older reader would get wrong raises `format_version`. A reader opens every file
with a `format_version` up to its own, and refuses a newer one with an error
that names both versions.
