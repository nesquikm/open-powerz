---
title: The web device menu lists one meter twice
milestone: M_NA20AD
status: active
archived_at: null
id: fr_01M72V1NGETNSB9S9JRQEHY1JX
created_at: 2026-11-10T13:41:26.000Z
changelog_category: Fixed
kind: bug
---

# The web device menu lists one meter twice

## Summary

The web edition's device menu showed one plugged-in meter as two entries, numbered one and two.
The browser had permission for the meter over both USB and HID, and the menu listed each permission as its own meter.
With serial numbers hidden the two looked like two meters of the same model.
Now each meter is listed once, through USB, which still falls back to HID by itself when another program holds the USB interface.
A meter the browser reaches only over HID is still listed, and its entry says HID.

## Requirement

### Steps to reproduce
1. In the web edition, grant the meter over USB and, once, over HID (Connect over HID).
2. Reload the page with Hide serial numbers on.

### Expected
The device menu lists the meter once.

### Actual
The menu lists it as the model with (1) and the model with (2), while the browser's USB device list holds one device (HIL12 note 4, HIL12b note C).

## Acceptance Criteria

- AC-EHY1JX.1: Regression: with one permitted WebUSB meter and one permitted WebHID meter of the same product, the web device list has one meter entry, the WebUSB one.
- AC-EHY1JX.2: HID meters beyond the WebUSB meters of their product are listed, and each HID entry reads its model followed by "(HID)" in the device menu.
- AC-EHY1JX.3: A HID meter with a live session stays listed while its session runs, even when a WebUSB meter of the same product is listed.
- AC-EHY1JX.4: Connecting the WebUSB entry while another program holds its interface still falls back to the permitted HID meter, as before.

## Technical Design

### Root cause
The web device list maps every permitted WebUSB device and every permitted WebHID device to its own entry. A meter granted over both is listed twice, and the menu label is built from the model, so the HID entry's own "(HID)" label never shows.

### Fix
The device list groups HID meters by product id and leaves out as many as there are WebUSB meters of that product; a HID meter with a live session is always listed and counts against none. The menu label adds "(HID)" for webhid entries.

## Testing

Vitest: the web backend's device list over fake WebUSB and WebHID navigators (both, HID only, two HID with one USB, live HID session), the label in the format tests, and the busy fallback test unchanged.

## Notes

WebHID gives no serial number, so the two permissions cannot be matched one to one; the count per product is the best match.
