---
title: USB PD 3.2 message decoder
milestone: M_NE3C7K
status: active
archived_at: null
id: fr_01M5HVWJ41WV7P8G50RPTGE83B
created_at: 2026-10-22T13:12:57.217Z
changelog_category: Added
---

# USB PD 3.2 message decoder

## Summary

Captured PD records are raw bytes until something explains them.
This feature decodes every USB PD revision 3.2 message into named fields with their bit ranges and units.
It covers power offers and requests, including PPS, AVS and EPR, vendor defined messages and extended messages.
Each message also gets a one-line summary for the packet table.
The decoder is a pure library.
The desktop app, the command line tool and a later web edition all share it.

## Requirement

A pure decoding library turns any PD message record into a typed message with a summary line and a field tree, following USB PD revision 3.2, without panicking on any input.

## Acceptance Criteria

- AC-TGE83B.1: The message header decodes all fields, with the cable plug and reserved variants for SOP' and SOP'', and names every control, data and extended message type in revision 3.2, EPR messages included.
- AC-TGE83B.2: Source and sink power data objects decode for fixed, battery, variable, PPS, SPR AVS and EPR AVS, with the field widths and units of the spec.
- AC-TGE83B.3: A Request decodes against the last Source_Capabilities seen on SOP in the same capture. That memory is cleared by Soft_Reset on SOP, by a hard reset and by a detach. EPR_Request decodes its RDO and its copy of the PDO.
- AC-TGE83B.4: Structured VDMs decode the VDM header, Discover Identity (ID header, cert stat, product, passive and active cable, UFP and DFP VDOs), Discover SVIDs, Discover Modes, Enter Mode, Exit Mode and Attention. Unstructured VDMs are shown with their header and raw data.
- AC-TGE83B.5: Extended messages decode their extended header and the data blocks for Source_Capabilities_Extended, Status, Battery_Capabilities, Get_Battery_Cap, Get_Battery_Status, Manufacturer_Info, PPS_Status, Sink_Capabilities_Extended and Extended_Control. Data messages Battery_Status, Sink_Capabilities, Alert, BIST, Get_Country_Info, Enter_USB, EPR_Mode, Source_Info and Revision decode as well.
- AC-TGE83B.6: Chunked extended messages, such as a chunked EPR_Source_Capabilities, are reassembled across records into one decoded message, and a missing chunk is reported.
- AC-TGE83B.7: Every decoded message has a one-line summary and a field tree in which each field carries its name, bit range, raw value and meaning. Reserved or invalid values are flagged, and no input makes the decoder panic (property test).
- AC-TGE83B.8: Golden tests cover examples from the spec and the session-2 vectors, including EPR mode entry and EPR_KeepAlive.
- AC-TGE83B.9: The decoder does no I/O and builds for the WebAssembly target.

## Technical Design

A new pd module in the protocol crate, or a small sibling crate if that keeps the protocol crate lean. Message types are enums generated from tables of the spec. Fields are described by a shared table of name, bit range and value formatter, which feeds both the typed decode and the field tree. A capture context object holds the last source capabilities and pending chunks; it is fed messages plus the capture's attach, detach, hard reset and cable reset events. Problems such as a body or chunk shorter than its header says are reported as issues on the decoded message.

## Testing

Table-driven golden tests per message type, property tests that random bytes never panic, a reassembly test with out-of-order and missing chunks, and a wasm32 build check in CI.

## Notes

The vendor's custom battery VDMs and a vendor ID to name table are optional and not required here.
