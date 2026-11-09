---
title: Web edition docs for the new browser features
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHXF
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Changed
---

# Web edition docs for the new browser features

## Summary

The web edition docs and the README said the browser could only read and stream.
They now list what works in the browser: settings, records, record delete, logo upload and protocol triggering.
They say which features need Web Serial.
They also name the two that stay in the desktop app: firmware update and showing a file.

## Requirement

The user docs, the README web section and the browser-support page match the web build's feature map.

## Acceptance Criteria

- AC-W8SHXF.1: The browser-support page lists every feature with the interface it needs (WebUSB, Web Serial or none) and which features stay desktop-only, with the reason.
- AC-W8SHXF.2: The README's web edition section no longer says the browser is read-only and names the two desktop-only features.
- AC-W8SHXF.3: A docs test checks that every feature the web build switches off is named on the browser-support page as desktop-only, and every feature it switches on is not.

## Technical Design

Edit the existing docs pages; extend the existing docs Vitest check to read the web feature map.

## Testing

The docs Vitest suite.

## Notes

Written after the feature FRs land.
