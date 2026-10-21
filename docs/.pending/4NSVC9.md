---
fr: 4NSVC9
impact_set: {"symbols":[{"name":"LiveBuffer","file":"apps/desktop/src/lib/chartData.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"cf78f06b4b28e68e208936b80b958001474cd995027301a943ddf52d27138017"},{"name":"chartOptions","file":"apps/desktop/src/lib/chartSeries.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"c790d22a8a3056a64000d1be84597fbd0b4b7b75dda514e1e2007fbd4b8c584c"},{"name":"dashedPaths","file":"apps/desktop/src/lib/chartSeries.ts","kind":"function","visibility":"public","change":"added","signatureHash":"5953d31ba1a23e45260b5f10d3edc1cc0db0b3167830485a114871751a2b51c3"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: explanation
target_file: docs/explanation/architecture.md
generated_at: 2026-10-21T17:44:46.000Z
---

## Keeping the live chart's memory steady

On macOS the app's window is a WebKit web view, and WebKit does not count the native memory of canvas path objects when it decides to collect garbage. A chart that built new path objects every frame therefore grew by tens of megabytes a second. The chart now strokes every series straight onto the canvas, splitting dashed lines into short solid pieces, and refills its live and overview frames in place instead of allocating new ones. Memory still climbs for a few minutes while the web view warms up, then levels off.
