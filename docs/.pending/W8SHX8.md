---
fr: W8SHX8
impact_set: {"symbols":[{"name":"MeterDriver","file":"apps/desktop/src/lib/web/driver.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"422f91c6f8219c026f2898fe99a864289e3c9692b56909c4cef5be381f7d0ab2"},{"name":"PowerzWasm","file":"apps/desktop/src/lib/web/driver.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"7642936150de5d926eb05d2f3189c14bab3c334ed090c0ebad4c546e7fcfe99d"},{"name":"RecordBookLike","file":"apps/desktop/src/lib/web/driver.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"30e756464981889213b527e67321c331e557565fd41c6b4e35b4a357dde82922"},{"name":"WEB_FEATURES","file":"apps/desktop/src/lib/web/features.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"8482ea16e94476f45aaf651760ba2dd9961ad2907047a3abec3237327023cc6b"},{"name":"WebSession","file":"apps/desktop/src/lib/web/session.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"90e02a8552ff7731cfe60c5e25b013ded724c997adb839bf5e1d394c64aa4c1e"},{"name":"SESSION_COMMANDS","file":"apps/desktop/src/lib/web/session.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"7791d6fb0e8b96377c5ab59010ef5b52675d038c9c433bdaf3d270cb55253628"},{"name":"READ_MASKS","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"051c52d397d7e56344ffcdaea82b34e1b8d9847691d8a2485aac791a4459ad8c"},{"name":"classify","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"45f4e877c032244bd27676fb8622a79d2c2bbece0cd5d24cca7903eab6413d97"},{"name":"demoRecordListReply","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"function","visibility":"public","change":"added","signatureHash":"fcb8f23cf538939073e2c02a29ac9adb280c28eb45f47acae85ef41deb23c774"},{"name":"recordWasm","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"function","visibility":"public","change":"added","signatureHash":"61141b81b6d7f2f7a97e3a66b3bb77215e10e767644ceda2e3adc4042be0d40c"},{"name":"RecordBookLike","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"7d6c1bec3fdbdee6cec915efd0f2a064c5c139831e59c7bcf3872a7300b185be"},{"name":"RecordWasm","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"f934f802bdb8e080ffb615200b8a8dc1b0c4217ba826fc39dbe3d03ff167792f"},{"name":"FrameName","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"type","visibility":"public","change":"modified","signatureHash":"cccb7510faeac4a3da649b81b5071cb9ad1bd804d9697ce08c3db391bdee6691"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/web-edition.md
generated_at: 2026-11-09T11:14:08Z
---

## Meter settings and offline records in the browser

Over WebUSB the web edition now reads the meter's settings panel and its offline records, so
`WEB_FEATURES` switches the settings read, the record list and the record download on. The
settings panel shows the same rows as the desktop app; the Records view lists the records and
opens one in the record chart. The browser sends only the reads the desktop app sends: GET_DATA
SETTINGS, the record list (after a level-1 sign-in when needed) and memory reads of that
record's own listed window, at most 256 bytes each. `MeterDriver` answers these through the
protocol core compiled to WebAssembly, and `WebSession` lists them in `SESSION_COMMANDS`.

A HID session (basic readings only) refuses all three. Editing the settings, deleting records,
the startup logo, protocol triggering and firmware update still say "Desktop app only".
