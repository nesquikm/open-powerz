---
fr: CWWKJ6
impact_set: {"symbols":[{"name":"MIN_AXIS_SPAN","file":"apps/desktop/src/lib/chartSeries.ts","kind":"const","visibility":"public","change":"added","signatureHash":"bbbb26b235b78ce4b467186b2545ffd6eb0f715ff3c831c831af9325ba5b686a"},{"name":"chartOptions","file":"apps/desktop/src/lib/chartSeries.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"982363fc1738add38ee98b866ed88dba7be2ae1a5b37ae4fd2e8d7fddc773674"},{"name":"envelopeRange","file":"apps/desktop/src/lib/chartSeries.ts","kind":"function","visibility":"public","change":"added","signatureHash":"3f9f34f4f44d4ab5616d6e3e554988a7fbd813878a50eaff48fd25abf0fdec01"},{"name":"minSpanRange","file":"apps/desktop/src/lib/chartSeries.ts","kind":"function","visibility":"public","change":"added","signatureHash":"ac010708e2b68c2768a1cba00a6caa26864c4637bfbfd76895ae32091ada6ed1"},{"name":"ChartProbe","file":"apps/desktop/src/lib/e2eHook.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"f0460befee00a25067c83c7f117d6b20f2832dce36725c6e63120419ac64cbd8"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"a14bf05c244b9fdd42d347c651c865426214f1b46ada5003442408cefeb11437"},{"name":"MOCK_BLIP","file":"apps/desktop/src/lib/mockBackend.ts","kind":"const","visibility":"public","change":"added","signatureHash":"184ff85846a525236c266a9efb4e24e6f732746702fd9459d92aee910db2f06f"},{"name":"MOCK_MODES","file":"apps/desktop/src/lib/mockBackend.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"554a0f49cbdca0088c14a1d09cfb13539bcc67bad64d2eae2018859178c85844"},{"name":"stripBreaks","file":"apps/desktop/src/lib/timeline.ts","kind":"function","visibility":"public","change":"added","signatureHash":"4856e64331907d0d5e6217c263146eb2c411b335bf8998e4582873297d9e4f18"},{"name":"StripBreak","file":"apps/desktop/src/lib/timeline.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"b282be9460e93ac2c9e1ba8756eeed6ed1f8c26525ec9f36348f88d0ac04da1e"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/explore-a-session.md
generated_at: 2026-10-21T18:18:10.000Z
---

## Gaps on the overview strip and quiet axes

Every stretch without data now shows on the overview strip as a faint band, the same way the main chart shades it, with its length when the label fits. Gaps longer than a minute are still squeezed to a narrow break.

With zero-based axes off, each value axis spans at least 0.1 V, 0.02 A or 0.1 W. Noise around an idle output no longer fills the whole axis; real swings still autoscale. The overview strip uses the same minimum span.

In recordings and exports the TEMP column stays empty once the meter's last valid temperature reading is more than 2 s old.
