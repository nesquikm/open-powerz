---
fr: TGE83C
impact_set: {"symbols":[{"name":"pdClear","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"707cafa1ba60108b1fb2dbb2fe123be375429454991b6601794b7a2bbf8244b9"},{"name":"pdDetails","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"ebefbe31d2cf91f56a8be25cf26722f12574f992bfbe412082da0d8b69d11d68"},{"name":"pdStart","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"050097308b00adbc25e64249b8f7556752b0e5dd8f3bce988e51ff057883dabf"},{"name":"pdStop","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"9b7213263cacd6f391085117949d0e7bd36dc3b62dd95cfb7cc325811ba1ea7e"},{"name":"PdDetails","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"d5687ac4e0383d280d9891a6cf349f83cc8ad498e421b901b7c50c31d3175053"},{"name":"PdField","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"f799591708b9791993ca79897719732cd2a82eed4d3310a642cad3b608a6f1d1"},{"name":"PdRow","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"26dc9e34679d3182981cfb236693d6592ab6a67e0bf2d89a9ee642397cb48356"},{"name":"BackendEvent","file":"apps/desktop/src/lib/backend.ts","kind":"type","visibility":"public","change":"modified","signatureHash":"d8212b5e1be4792ce49b56c8fe8d94f6a5e49f0fc1ae4cb9bca5e8f4a1aea1bc"},{"name":"PdDirection","file":"apps/desktop/src/lib/backend.ts","kind":"type","visibility":"public","change":"added","signatureHash":"3b2a8dbc0848e5e492694d6abd55eac0f4dea7d9d3260dc63bcec567aa426be7"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"808991c24c772e3e44cd55f0a647d81072c462666c3b94857abf71f5285d95f2"},{"name":"pdSyntheticFromUrl","file":"apps/desktop/src/lib/mockBackend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"31f0ea0b475113bfd51f56f283194a913e862503c56ccd440868a8239432cd80"},{"name":"MockOptions","file":"apps/desktop/src/lib/mockBackend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"15279b3201df38e57c404e9aeee987353f3622379c8f833d34006db8d81bdae7"}],"routes":[],"configKeys":[],"stateEvents":[{"kind":"action-type","name":"pd_rows","file":"apps/desktop/src/lib/backend.ts","change":"added"},{"kind":"action-type","name":"pd_state","file":"apps/desktop/src/lib/backend.ts","change":"added"},{"kind":"action-type","name":"pd_rows","file":"apps/desktop/src/lib/mockBackend.ts","change":"added"},{"kind":"action-type","name":"pd_state","file":"apps/desktop/src/lib/mockBackend.ts","change":"added"}]}
target_section: how-to
target_file: docs/how-to/analyze-pd-traffic.md
generated_at: 2026-10-22T17:30:28Z
---

# Watch a USB PD negotiation

The **PD analyzer** tab captures the USB Power Delivery messages on the cable that
passes through the meter. The capture is passive: the meter keeps passing power and
nothing is triggered.

1. Connect the meter (or pick a demo device, which plays a scripted negotiation).
2. Open the **PD analyzer** tab and press **Start capture**. Start and Stop are
   disabled, with the reason shown, while no meter is connected.
3. Plug the charger into the meter's input and the phone or laptop into its output.
   Each message and event appears as a row: index, time since the capture started,
   SOP, message name, message ID, direction, object count, spec revision, VBUS/IBUS
   at that moment and a one-line summary. Events (attach, detach, resets) look
   different from messages.
4. Press **Stop capture** when you are done.

## Narrow the table

- GoodCRC replies are hidden by default; tick **Show GoodCRC** to see them.
- **Message type** and **SOP** narrow the table further. Filters only hide rows;
  nothing captured is dropped.

## Read a message

Select a row (click, or use the arrow keys, Page Up/Down, Home and End). The
**Packet details** pane shows every decoded field with its bit range and meaning,
and the raw bytes in hex. Selecting a field highlights its bytes.

While capturing, the table follows new rows. Scroll up to stop following;
**Follow** jumps back to the newest row. **Clear capture** empties the table after
you confirm.

For developers: the backend seam gains `pdStart`, `pdStop`, `pdDetails`
(a record's `PdDetails`: raw bytes plus a `PdField` tree with byte ranges) and
`pdClear` (answers the next record index). Rows arrive as `PdRow` batches on the
session channel (`BackendEvent` gains `pd_rows` and `pd_state`), with a
`PdDirection` per message. `MockBackend` plays a looping capture, and
`pdSyntheticFromUrl` reads `?pdsynthetic=<n>` to seed `n` records for
performance work (`MockOptions`).
