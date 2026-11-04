---
title: The Device select's tooltip is tested under Hide serial numbers
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M6JZC11NJJG1Y82GFPNKAD6S
created_at: 2026-11-04T09:48:46.005Z
changelog_category: Fixed
kind: bug
---

# The Device select's tooltip is tested under Hide serial numbers

## Summary

In the r4 hardware run the Device select's tooltip showed the meter's serial.
Hide serial numbers was off, its default, so the select showed the serial too.
With the switch on, the tooltip already uses the masked label.
This change pins that with a test in both states and changes no behaviour.

## Requirement

### Steps to reproduce
1. List a real meter and pick it in the Device select, with Hide serial numbers off.
2. Hover the select.

### Expected
With the switch off, the tooltip shows the same label as the select. With it on, the tooltip shows the masked label.

### Actual
The tooltip showed the serial with the switch off, as designed; no test pinned the tooltip on its own.

## Acceptance Criteria

- AC-NKAD6S.1: With Hide serial numbers on and a real meter selected, the Device select's tooltip reads "KM003C — Hidden" and holds no serial.
- AC-NKAD6S.2: With the switch off, the tooltip is the full label with the serial, the same text as the selected option.
- AC-NKAD6S.3: Turning the switch on or off changes the tooltip at once, with no reconnect.
- AC-NKAD6S.4: Regression: a Playwright test on the mocked backend with a listed real meter covers AC.1 to AC.3 and passes on WebKit.

## Technical Design

### Root cause
Not a bug: the tooltip is the selected entry's label from the same function as the option text, which honours the switch. The tester ran with the switch off.

### Fix
No code change; a WebKit test reads the select's title attribute with the switch on, then off, then on again.

## Testing

One Playwright test in the hide-serial spec on the replug mock, run on WebKit.

## Notes

Source: M7b r4 hardware run, finding 7 in its report. Owning feature: AXHK2Q (hide serial numbers). The orchestrator agreed it works as designed.
