---
fr: fr_01M65KVDYC49QF1V7N1Y4PJRVK
impact_set: {"symbols":[{"name":"pdClear","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"24f6674235e6b42ae9276a91d723bcebcb0c15b6941ddf0cda00b38eed6f0165"},{"name":"MeterBackend","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"e1db47ec95e4d936574d9f133c0f4186ec626ffe2ca08cc580e4c01cbde221f2"},{"name":"PdCleared","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"2141909e0f3e9906e96ea6c0fa7d6cf99062c1647afbe9432c363494ed65685c"},{"name":"PdOpened","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"8fa45e870d7d3c777f39207944ff165eb3ccfc6aefce1eb0de82f322a797f8a8"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"ef83609860ccfa457aeb1d92f4600d905facdfa4586da5a889afd31d5ab0e4ed"},{"name":"pdContractAt","file":"apps/desktop/src/lib/mockPd.ts","kind":"function","visibility":"public","change":"added","signatureHash":"ec1476b5232513a513fcb1c1b4995a77f6cc1fa71b2539079d5d5ee0ce62b6ab"},{"name":"pdRow","file":"apps/desktop/src/lib/mockPd.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"a48b937c90bce0dc86620eafd1be1d81082c5061dc85ea8ad801ff643905cb1d"},{"name":"PdCapture","file":"apps/desktop/src/lib/pdCapture.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"f07847a40593f88384a17ce63a00b298471468dfc7ce04d4ab4602709dc1f9ad"},{"name":"pdImportNoticeText","file":"apps/desktop/src/lib/pdSave.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"686bbb4d711a706c32db4c7dd966e0a18a7c28c89d4ac079dc427e689eca0f65"},{"name":"pdOpenDamageText","file":"apps/desktop/src/lib/pdSave.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"24e32eaf0f0d3545421e96537871cef79610c34f4b46e57402253ef873803121"},{"name":"pdOpenedNotice","file":"apps/desktop/src/lib/pdSave.ts","kind":"function","visibility":"public","change":"added","signatureHash":"373ddddf5470ef9215fe72dd570c6fd04a16503bec72f7fccc33a0a3ea1f7057"},{"name":"PdOpenedNotice","file":"apps/desktop/src/lib/pdSave.ts","kind":"type","visibility":"public","change":"added","signatureHash":"109d492d2e67ee24573485a4002ac19514ef4a7b8e7ffbaaa550f0079cf29744"},{"name":"PdStore","file":"apps/desktop/src/lib/pdStore.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"37e9280218cd9d62a391c08f896e655cba6125df7ea5af5ec76db4067df435d2"},{"name":"PD_MAX_ROWS","file":"apps/desktop/src/lib/pdStore.ts","kind":"const","visibility":"public","change":"added","signatureHash":"9df501a15f3546d7ffab41f3f52dbea416d3d17fe1c6643513b72d241013adcb"},{"name":"pdDroppedText","file":"apps/desktop/src/lib/pdStore.ts","kind":"function","visibility":"public","change":"added","signatureHash":"60a709c62d27b6c44c4788af84c0a360ed27ea886f2a632f379a11a364970dea"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: reference
target_file: docs/reference/desktop-backend.md
generated_at: 2026-10-30T16:11:43Z
---

Capture memory, clearing and opened files behave differently in the desktop
backend:

- `pdClear` now answers a `PdCleared`: the index of the next record and the
  status time at the clear. The page uses it to drop status samples that were
  sent before the clear and arrive after it.
- `PdOpened` gains `skipped` (vendor log rows without a whole record) and
  `dropped` (the oldest records past the capture's limit). Its rows are only
  the ones the capture keeps.
- A capture keeps its latest 100,000 records and drops the oldest, with a
  notice. Status samples are not part of this limit.
- A file damaged on its first entry opens as an empty capture and names the
  line; a file with no pdcap header is still refused.
- An opened file is released when a session reports connected, on a Connect or
  an automatic reconnect. A connect or reconnect that fails keeps it.
