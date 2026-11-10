---
title: Some abbreviations have no glossary popover
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
id: fr_01M70N5N8DT420JMX7K062VEDM
created_at: 2026-11-09T17:19:53.614Z
changelog_category: Fixed
kind: bug
---

# Some abbreviations have no glossary popover

## Summary

The user found protocol names on the Trigger tab that were not dotted-underline terms.
VFCP, TFCP and SFCP had no glossary entry, and the USB PD card title and the PD emulation labels were plain text.
These terms now open the same short popovers as the others.
A sweep of every view wrapped the other abbreviations that were still shown without one.
A browser check now fails when a glossary term shows up as plain text on a view.

## Requirement

### Steps to reproduce
1. Open the Trigger tab with the demo meter and press Detect.
2. Look at the protocol card titles and the PD emulation block.

### Expected
Every abbreviation is a dotted-underline term that opens its glossary popover.

### Actual
VFCP, TFCP, SFCP, the USB PD title and the E-marker label are plain text.

## Acceptance Criteria

- AC-62VEDM.1: Regression: the glossary has entries for VFCP, TFCP and SFCP, each with a short sourced definition, and their Trigger card titles open them.
- AC-62VEDM.2: The USB PD card title, the PD emulation heading and its E-marker label, and the cable card's EPR, VID and PID labels are glossary terms.
- AC-62VEDM.3: Every other abbreviation the sweep found in plain text on the Meter, PD analyzer, Records, Startup logo, Firmware and Trigger views and in the web edition's unsupported-browser notice is a glossary term, or the FR Notes name it with the reason it stays plain.
- AC-62VEDM.4: A browser check on the mock build visits those views and fails when a glossary term's text shows outside a glossary term in a section that has no wrapped copy of it, ignoring text inside controls and table data cells.
- AC-62VEDM.5: The glossary unit checks still pass: every key used, every entry used, every source our definition, a protocol.md section or a named spec.

## Technical Design

Add the entries to the one glossary module and wrap the first plain occurrence per section with the existing Term component. The browser check walks the visible text nodes of each view after it renders, matches each term entry's spellings, and groups hits by the nearest section; a section passes when each matched term has at least one wrapped copy there.

## Testing

Vitest glossary checks; a new Playwright sweep spec on Chromium and WebKit; the layout and select-width specs rerun.

## Notes

Reported 2026-11-09. Plain on purpose: text in buttons, tabs, options and checkbox labels (no button in a button), table cells, PD message and decoded field names (VDM, PDO, RDO), file formats, the model name, backend hint text, raw meter setting codes (50v-240w-epr).
