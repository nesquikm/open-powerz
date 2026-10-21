---
fr: X2SPDW
impact_set: {"symbols":[{"name":"temperatureDisplay","file":"apps/desktop/src/lib/format.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"05599f084b512a2b193db5060d0a3262c4ab7ede2ef3b3eb7acce41849482724"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"ff0fede6e15b2ae45ea25fe51c68983ba6388dee90ca051716023476d1b1a1fa"},{"name":"LINUX_PERMISSION_DENIED","file":"apps/desktop/src/lib/mockBackend.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"450988e0b2b91fa35b209a73c39b168cc89da7577490a3cb33afc31df834eacb"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/record-a-csv.md
generated_at: 2026-10-21T17:44:46.000Z
---

## Temperature, gaps and times in the CSV

The TEMP column carries the meter's latest valid temperature reading at or before each row; it stays empty until the meter has reported one. Recordings, exports and the powerz stream command fill it the same way, so a recording of a session still matches what the command line writes.

At 1000 SPS the readout shows the temperature with the tooltip "Updated once a second at 1000 SPS", because the meter reports it from a separate reading. A meter whose firmware reports no temperature at that rate shows "—" instead.

The stop notice counts only the gaps that happened after the first written row, so a recording that starts mid-stream never reports a gap it does not contain. A sample carries the same Unix time in a recording and in an export of the same session, also after the chart history was cleared.

If the meter stops answering after the app was closed while streaming, the app now clears the stuck USB pipe and retries the connection by itself; you only need to replug when it asks you to.
