---
fr: fr_01M65KVDYC49QF1V7N1Y4PJRVM
impact_set: {"symbols":[{"name":"currentMeterCommands","file":"apps/desktop/src/lib/backend.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"009a282fb5919fa10949cd6f015de6c565b3a74874d61be8d4a680742183e98d"},{"name":"meterBackend","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"b6fb6ae58a4962409a312f87f5b95f005b919690d988d50d12f66333232022cc"},{"name":"saveRecordingAs","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"ecc975e549be8ea742092165af3679b919eb0598b23baa786b7288514925de52"},{"name":"MeterBackend","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"d491ef19093a44ad316c6e02d4e4a689d888591e50cf1a8bae4a060faf509d69"},{"name":"MeterSettings","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"38e7995f118cb7de045b6e72c2b73eb075828cad650e005fd466906559be920d"},{"name":"RecordingSnapshot","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"dbc318ceb933a1aec62d8dbd3194896584ea455fd8309a62e6c907ea07eb5685"},{"name":"FirmwareUpdate","file":"apps/desktop/src/lib/firmwareUpdate.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"1bed1bb5e8e3eea78c2c9c834d251a4058a5fee60aeae8503a81e02a2f1c018f"},{"name":"controlsLabel","file":"apps/desktop/src/lib/meterGrid.ts","kind":"function","visibility":"public","change":"added","signatureHash":"63b618242abc79193f2a757a2a4af394eaf511902d3c9ab878f824cb3465ad81"},{"name":"Meters","file":"apps/desktop/src/lib/meters.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"d79d716858f1102451e90f6e5dd0ee6256693cb21d0e45ffdbe3b8c958fc5885"},{"name":"MeterEntry","file":"apps/desktop/src/lib/meters.svelte.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"cb8bb99a07c100928e2d25ac481378874a9681109d9fbffa9e4326f09a943684"},{"name":"MeterSettingsPanel","file":"apps/desktop/src/lib/meterSettings.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"227d1ec8362be93d53f109cbf0e8a75429b1e8a6044b73ee6b9929278fd8d9f9"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"2769a4252569c80e8d4170e64683c2789972d5f0b12687721b7d34eef0312841"},{"name":"PdCapture","file":"apps/desktop/src/lib/pdCapture.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"30c4a6ce8f460b12d303482cacaedfb3fa2d24a09ce91a13994b19d9fe51341a"},{"name":"Recording","file":"apps/desktop/src/lib/recording.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"4fca43de0ea5f4554255f9a89b57d3229aaf00a7c1099b207c2ee78265214969"},{"name":"recordButton","file":"apps/desktop/src/lib/recording.svelte.ts","kind":"function","visibility":"public","change":"added","signatureHash":"1a788c1445b9644df3849b594f5402feedb0155ee8623c2c9f647599199118fc"},{"name":"RecordingDeps","file":"apps/desktop/src/lib/recording.svelte.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"49dfe969ccf890efff061e0754814dcc017c45ee86cd1ef1ead7cf90f87119c7"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: reference
target_file: docs/reference/desktop-backend.md
generated_at: 2026-10-31T05:52:58Z
---

<!-- warning: output includes startRecording, record_delete_table, write_settings_checked, PortHeld, open_listed, warnings: names the impact set does not extract -->

Several meters, records and settings change as follows:

- In the grid each tile has its own controls: connect, rate, Record and Export
  act on that meter only, and the top bar drops them.
- `startRecording` starts at once on a new file in the recordings folder and
  opens no dialog. `saveRecordingAs` asks where to keep the file at Stop.
  `RecordingSnapshot` gains `path`, so a reloaded page can still offer the save.
- Another meter can connect during a firmware update. A meter that may be the
  update's target is refused, including one whose mode cannot be read.
- A parked meter keeps its PD capture while another tile is in the main view.
- The app writes `app.log` with connects, session ends and replugs, with no
  serial or UID.
- `record_delete_table` refuses a delete before any backup is written.
- `write_settings_checked` adds `warnings` for undecoded bytes that changed.
- A serial port another program holds is `PortHeld`, from `open_listed`.
- `Settings::decode` accepts a block A the meter saved in an earlier boot. Only
  the word at 0x10 may differ from what the CRC covered, and that word must be
  0xFFFFFFFF, or a Unix time from 2020 on no later than the time shown there.
- An I/O failure on a bulk pipe whose meter has left USB is
  `transport::Error::Disconnected`, and `map_io_error` reads ENXIO and ENODEV
  the same way outside Windows, so the banner and `app.log` both say unplugged.
