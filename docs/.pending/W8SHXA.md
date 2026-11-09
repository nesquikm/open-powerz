---
fr: W8SHXA
impact_set: {"symbols":[{"name":"MeterDriver","file":"apps/desktop/src/lib/web/driver.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"db8abb117d64692e9c6d97078174a12ce518a60b14cc7898230594d16cdc3b91"},{"name":"BackupFile","file":"apps/desktop/src/lib/web/driver.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"baa595d1ceec7b94124cca09da21e99b5f03efcaaef5e4ef12843ac8303b040e"},{"name":"RecordBookLike","file":"apps/desktop/src/lib/web/driver.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"a624ca20433444a87cfef31098f29e82e7c8b3a1181bdb1f4c4182c513a9c4f5"},{"name":"WEB_FEATURES","file":"apps/desktop/src/lib/web/features.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"ab5493bc9856efe29cb5d6f1e8fbe681f922a78f7ab10672ca5de9c45326e8a8"},{"name":"WebSession","file":"apps/desktop/src/lib/web/session.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"e54c4b61f61cc87bd9f60bb74a7290c84e429e8993137d2449d20e0f01653908"},{"name":"SESSION_COMMANDS","file":"apps/desktop/src/lib/web/session.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"c123e02af95d45d5da1abd8a338b55ea55c4b3ce1738d182707d8e8122b0b014"},{"name":"WebSessionOptions","file":"apps/desktop/src/lib/web/session.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"31f51d96886a612fbed51513aa36b47447a3d64c88b2db57cb0ff9dcd3f269b8"},{"name":"classify","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"ff9c88af6f9e5db736843c116e45a0288b59424e68c23f4aa6a254dd13d4f62e"},{"name":"RecordDeleteBookLike","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"90e74b3befa7cc8631719de81e6c9280836d218b580c5acdd3204ea7784adae6"},{"name":"FrameName","file":"apps/desktop/src/lib/web/testWasm.test.ts","kind":"type","visibility":"public","change":"modified","signatureHash":"46fa7e5ea83774623db68d0f816bdc46a0e54071fa390bfcb047d7385daba2e7"},{"name":"createWebBackend","file":"apps/desktop/src/lib/web/webBackend.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"3f6c861d947e28e6bafadfb99c670b44d1464ec48874f1e5d26aed18ef344490"},{"name":"WebBackendOptions","file":"apps/desktop/src/lib/web/webBackend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"cb00cf27704c43b4691ba5ba47689662fca328674693ae08ee4a784ae6f2c09a"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/web-edition.md
generated_at: 2026-11-09T12:44:25Z
---

## Delete offline records in the browser

The Records view can delete one record or all of them in the browser, as in the desktop
app (`WEB_FEATURES` switches the record delete on). Type the record's name (or `all`) to
confirm. The browser first asks for a folder: every affected record is downloaded from the
meter and written there as a CSV backup, under `open-powerz-backups/records-<time>`, before
the meter's record table is rewritten once and read back. A cancelled folder picker or a
failed backup deletes nothing. A browser that cannot write a folder saves the backups as
downloads instead. A HID session cannot delete records.
