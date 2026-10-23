---
title: Legacy and vendor protocol triggering
milestone: M_X4VEQ3
status: active
archived_at: null
id: fr_01M5KJYD899REQQGB2GZDGSQ6Q
created_at: 2026-10-23T05:15:09.449Z
changelog_category: Added
---

# Legacy and vendor protocol triggering

## Summary

Many chargers speak older or vendor fast-charge protocols alongside or instead of USB PD.
This feature triggers Quick Charge 2.0 and 3.0, Huawei FCP and SCP, and Samsung AFC through the meter.
It also covers vivo VFCP, Spreadtrum SFCP, Transsion TFCP and UFCS.
Protocols that report their limits show them decoded before a request.
A charger that lacks a protocol gets a clear message instead of a hang.
Every request above 5 V needs an explicit confirmation.

## Requirement

The library and the command line tool enter each supported non-PD protocol and request voltages or currents in it, with decoded capabilities where the protocol reports them and a confirmation for every request above 5 V.

## Acceptance Criteria

- AC-DGSQ6Q.1: Quick Charge 2.0 requests 5, 9, 12 and 20 V, and Quick Charge 3.0 requests a voltage in 200 mV steps from 3.6 to 20 V and steps up or down by a count.
- AC-DGSQ6Q.2: FCP, AFC and SFCP request 5, 9 and 12 V.
- AC-DGSQ6Q.3: SCP, VFCP and TFCP read their capability block, show its decoded limits, and request a voltage and current inside them.
- AC-DGSQ6Q.4: UFCS reads its output capabilities, requests a capability by number with voltage and current, and sends the UFCS control commands the device documents.
- AC-DGSQ6Q.5: When the charger does not answer a protocol (the device replies with a timeout), the tool reports that the charger does not support it and leaves the meter at 5 V.
- AC-DGSQ6Q.6: The command line tool has a trigger subcommand per protocol, and a request above 5 V fails with a clear message unless the allow-high-voltage flag is given.
- AC-DGSQ6Q.7: Each protocol that one of the user's chargers supports is checked on the real meter with nothing on the output, and the result is recorded in the Notes. Protocols no available charger supports are recorded as not verified on hardware.

## Technical Design

One small module per protocol renders commands and parses its capability block, sharing the PDM session, the risk classifier and the reply handling. The command line subcommands share argument parsing and the high-voltage flag. Requests must also fit the block's maximum power, a QC 3.0 step count of 0 is refused, and `ufcs cmd` needs the high-voltage flag because UFCS control numbers are not proven safe. The CLI leaves every protocol with `reset`, then `pdm close`.

## Testing

Simulator tests per protocol, including capability parsing, range validation and the unsupported-charger timeout, and CLI tests for every subcommand. Hardware checks are manual.

## Notes

Hardware 2026-10-23, nothing on the output: QC 2.0 9 V (9.07 and 9.14 V) and QC 3.0 7.0 V (7.20 V) verified. Not verified on hardware: QC 3.0 steps, FCP and AFC (no reply or VBUS unchanged), UFCS requests (VBUS unchanged), SCP, TFCP, VFCP and SFCP.
