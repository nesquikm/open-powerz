---
title: Hidden serials read as the model, and *** where a value must show
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6RFJY4Y160W7PPWV9CCF47K
created_at: 2026-11-06T13:08:33.000Z
changelog_category: Changed
---

# Hidden serials read as the model, and *** where a value must show

## Summary

With serials hidden, the picker read "KM003C — Hidden", which looked odd.
Labels that join a model and a serial now show the model alone.
A value that must show, like the Serial row in About, reads three stars.
Two meters of one model get a number after the model.
The command line prints three stars too, and no serial shows anywhere.

## Requirement

With serial numbers hidden, every model-and-serial label shows the model alone, numbered when two listed meters share a model, and every place that shows a serial value shows "***"; the CLI's `--hide-serial` prints "***" for the serial and the UID.

## Acceptance Criteria

- AC-CCF47K.1: With Hide serial numbers on, a real meter in the Device select, its tooltip, the Add meter dialog's list and the firmware view's device history is named by its model alone, for example "KM003C", with no dash and no word after it; a meter in bootloader mode reads "KM003C (bootloader mode)".
- AC-CCF47K.2: With the switch on and two or more real meters of the same model listed, each of them reads "KM003C (1)", "KM003C (2)" and so on, in the picker and in the Add meter dialog. The numbers follow the meters' serials in ascending order, with meters that have no serial after them in device-id order, so a meter keeps its number while the same meters stay listed. A model listed once gets no number, and demo devices are never numbered.
- AC-CCF47K.3: With the switch on, the About panel's Serial row reads "***", and a backend label that joins a model and a serial (the firmware session prompt) shows the model alone; a label with no model part reads "***". No shown text reads "Hidden".
- AC-CCF47K.4: With the switch off, every label and value reads as before this change, serial included, and turning the switch on or off changes them at once with no reconnect.
- AC-CCF47K.5: The no-serial guarantee of the hide-serial feature still holds: with the switch on, the Playwright DOM scan finds no serial and no serial suffix in the page text, attributes, form values or title, with one and with two meters listed.
- AC-CCF47K.6: `powerz list --hide-serial` and `powerz info --hide-serial` print "***" where they printed "hidden", in text and JSON, for the serial and the UID, and the rest of their output is unchanged; the `--help` text says so.
- AC-CCF47K.7: The user docs and the CLI reference describe the new wording: the model alone in labels, a number for same-model meters, and "***" for a shown value.

## Technical Design

`displaySerial` returns "***". `displayMeterLabel` keeps only the part before " — " when hidden, else "***". `deviceOptionLabel(device, hidden, among = [])` drops the serial when hidden and, when `among` holds two or more non-demo listings of the device's model, appends " (n)" by the order in AC.2 (serial with numeric collation, then id). The picker and the Add meter dialog pass their lists; the tooltip passes the picker's list. Tile titles keep model and position. The CLI's `HIDDEN` constant becomes "***".

## Testing

Vitest for the helpers (numbering, order, bootloader, demo, switch off) and the details row; existing tests move from "Hidden" to the new text. Playwright on the mocked backend: picker, tooltip and About on WebKit and Chromium, two-meter scan. CLI integration tests for both formats.

## Notes

User request 2026-11-06 (brief in the research workspace's M7c folder). Changes the wording set by the archived hide-serial FRs; their DOM-scan and CLI tests stay and are updated.
