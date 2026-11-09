---
title: Web edition docs
milestone: M_QFTCWW
status: archived
archived_at: 2026-11-08T21:34:13Z
id: fr_01M6VHDG2BG62PZDW7K4QFTCX2
created_at: 2026-11-07T17:39:28.900Z
changelog_category: Changed
---

# Web edition docs

## Summary

Users learn that the web edition exists, where it lives, and which browsers it supports.
A docs page explains the permissions the browser asks for.
It says why some features stay in the desktop app.
It also shows how to fix the common access problems on each system.

## Requirement

Add a README section with the web edition link and a docs page on browser support and permissions; update the protocol and architecture docs for the new crate and transports.

## Acceptance Criteria

- AC-QFTCX2.1: The README has a "Web edition" section with the Pages link, the supported browsers (Chrome and Edge on desktop), the demo meter and the list of desktop-only features.
- AC-QFTCX2.2: A docs page covers browser support, the device picker and permission flow, the WebHID fallback, Linux udev access, Windows driver notes for WebUSB, and that the web edition only reads and streams.
- AC-QFTCX2.3: The project CLAUDE.md architecture tree and the technical spec list the `powerz-wasm` crate and the web backend folder, and the gate chain includes the wasm build.
- AC-QFTCX2.4: The docs index links the new page, and existing docs tests (README feature lines) pass.

## Technical Design

Docs-only change plus the CLAUDE.md and technical-spec updates; the README link uses the Pages address of the public repository.

## Testing

Existing README and docs Vitest checks run; a new check asserts the README section and the docs link exist.

## Notes

The Pages link goes live only after Pages is enabled and the first deploy runs.
