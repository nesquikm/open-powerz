---
fr: TGE83D
impact_set: {"symbols":[{"name":"ChartPdMarker","file":"apps/desktop/e2e/hook.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"4d216171f22c28d28df430fa786fd6d557db5c10127345c6ffa747b610a0ab37"},{"name":"ChartPdStatusLengths","file":"apps/desktop/e2e/hook.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"ee22157f8944d3990b50233f1dd30c40291966d2341ca9ad1efe01add6779e5c"},{"name":"OpenPowerzE2E","file":"apps/desktop/e2e/hook.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"970c97d638abf7e9d52a01af39bb8c2b877f851e757250402ff80636301c20b6"},{"name":"PdRow","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"fbe1d02052a57900a7a202278270874f96c8477dfe2a8c6162e6dc1502093040"},{"name":"PdStatusSample","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"dd8db8b603dd71d26dda8f274f027a7f2eefaabe59f080ac21a5024dcdb7a301"},{"name":"BackendEvent","file":"apps/desktop/src/lib/backend.ts","kind":"type","visibility":"public","change":"modified","signatureHash":"c668c9d0c289148f7426114b96a8926180a281d5e35d97595970497aee1478f3"},{"name":"ChartProbe","file":"apps/desktop/src/lib/e2eHook.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"61689d0b485ede657dc76bef658ed20b1acb567a2fc5c9ffc6c2e6017b2e3a7a"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"1d2e49641bf6ebe54af138399c7d449a6c3eb49883e8affd8138677c0f9ee471"},{"name":"PdCapture","file":"apps/desktop/src/lib/pdCapture.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"ca6b4ddce2e4c7731c92030f83298add31238a0577c1a696fd5f0c5d3eb59e38"},{"name":"PdStore","file":"apps/desktop/src/lib/pdStore.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"d9fa7b8853f8dd6a5891340c6b783720c7d57542c9add8a06b830e3839eca345"},{"name":"THEME_COLORS","file":"apps/desktop/src/lib/theme.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"cbd2a3348f60b36822479303b02429e7b4dec3043b92fbd08dc0a090d856b522"},{"name":"themeVariables","file":"apps/desktop/src/lib/theme.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"e724cd393ffdec93356dc87d3b32be54da513ccb1f3b4f97e6d278cc54d7f859"},{"name":"ThemePalette","file":"apps/desktop/src/lib/theme.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"6c6978e32a6fc4e3e383b7c096485f811826c1587ecd3f19187cd37686395494"}],"routes":[],"configKeys":[],"stateEvents":[{"kind":"action-type","name":"pd_status","file":"apps/desktop/src/lib/backend.ts","change":"added"},{"kind":"action-type","name":"pd_status","file":"apps/desktop/src/lib/mockBackend.ts","change":"added"},{"kind":"action-type","name":"pd_status","file":"apps/desktop/src/lib/pdCapture.svelte.test.ts","change":"added"},{"kind":"case-branch","name":"pd_status","file":"apps/desktop/src/lib/pdCapture.svelte.ts","change":"added"}]}
target_section: how-to
target_file: docs/how-to/analyze-pd-traffic.md
generated_at: 2026-10-22T18:35:51Z
---

## See PD messages on the power chart

While a PD capture runs, every message and event also shows on the Meter tab's
chart, in a lane above the plot at the moment it happened. Colours tell the kind
apart (negotiation, reset, error, attach / detach, other) and a legend names them.
Markers too close to tell apart merge into a badge with a count; zoom in to see
them one by one.

- Click a marker (or a badge) to open the PD analyzer with that message selected.
- Select a row in the packet table to move the chart's playhead to its moment. If it
  is already in view the zoom stays as it is; otherwise the chart centres on it.
- The VBUS, IBUS, CC1 and CC2 values the meter reports with each PD packet are drawn
  as dots on the matching series, and the CC1 / CC2 buttons hide and show them like
  the other series.

For developers: `PdRow` carries its time on the chart's session time base, and
`BackendEvent` gains a batched PD status event of `PdStatusSample` values; `PdCapture`
keeps them for the chart, and `THEME_COLORS` / `themeVariables` add the marker colours.
