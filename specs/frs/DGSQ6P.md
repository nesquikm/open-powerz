---
title: USB PD triggering
milestone: M_X4VEQ3
status: active
archived_at: null
id: fr_01M5KJYD899REQQGB2GZDGSQ6P
created_at: 2026-10-23T05:15:09.449Z
changelog_category: Added
---

# USB PD triggering

## Summary

This feature asks a USB PD charger for a specific voltage through the meter.
It covers fixed voltages, PPS and AVS ranges, and EPR voltages up to 48 V.
A hard reset returns the charger to 5 V.
The charger's offers are read from the meter and shown decoded, so the user picks a real option.
The meter's e-marker and sink emulation settings can be read and changed.
Any request above 5 V needs an explicit confirmation every time.

## Requirement

The library and the command line tool request fixed, PPS, AVS and EPR power from a PD charger through the meter, read and set the PD emulation settings, and send a hard reset, with a confirmation for every request above 5 V.

## Acceptance Criteria

- AC-DGSQ6P.1: After entry pd, the charger's SPR and EPR offers from the pdo blocks are decoded with the PD decoder and listed with position, type, voltage or range, and current.
- AC-DGSQ6P.2: A fixed request by position, and a PPS or AVS request with voltage and current, are sent in the device's command form, validated against the offered range first, and the req and ready replies are reported with the delivered voltage and current.
- AC-DGSQ6P.3: EPR positions above 7 are requested in the same way, and the tool reports when the charger or cable cannot enter EPR.
- AC-DGSQ6P.4: The PD settings (type, e-marker emulation and sink capabilities) are read with pdm parm and changed with pdm set, using named values instead of raw numbers.
- AC-DGSQ6P.5: A hard reset command returns the charger to 5 V, and the tool reports the new offers it receives afterwards.
- AC-DGSQ6P.6: The command line tool has a trigger pd command (list, request, settings and reset). A request above 5 V fails with a clear message unless an allow-high-voltage flag is given.
- AC-DGSQ6P.7: On the real meter with the 140 W charger and nothing on the output, requests for 9, 15, 20 and 28 V, an AVS request and a hard reset back to 5 V deliver the requested voltage within 5 percent. The results are recorded in the Notes.

## Technical Design

A pd trigger layer on top of the PDM session renders requests, maps pdo blocks to the PD decoder's PDO types and validates requests against them. The CLI subcommand uses the session guard so the meter returns to 5 V and closes the session on exit. EPR uses the device default em=2 unless the user changes it.

## Testing

Simulator tests for every request type, the range validation, EPR entry failure and hard reset; CLI tests for the high-voltage flag; the real-meter check is manual.

## Notes

Session 2 confirmed 9, 15, 20 and 28 V and AVS on the 140 W charger. PPS is untested on hardware until a PPS charger is available.
