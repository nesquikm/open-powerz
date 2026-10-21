---
fr: SX3EDV
impact_set:
  {
    "symbols":
      [
        {
          "name": "rangeAnnouncement",
          "file": "apps/desktop/src/lib/chartView.ts",
          "kind": "function",
          "visibility": "public",
          "change": "modified",
          "signatureHash": "664a3c081c9cefb63c0173850767b677680f6d5454c61a9a3424060ee3b1dadd",
        },
        {
          "name": "rangeDecimals",
          "file": "apps/desktop/src/lib/chartView.ts",
          "kind": "function",
          "visibility": "public",
          "change": "added",
          "signatureHash": "f1ab02393e710049380d680a2bf4865b56f8fb2c645ac4b4bd7cd84ae69a9189",
        },
      ],
    "routes": [],
    "configKeys": [],
    "stateEvents": [],
  }
target_section: how-to
target_file: docs/how-to/explore-a-session.md
generated_at: 2026-10-21T20:35:54.836Z
---

## A steadier window in narrow layouts

Starting an export no longer moves anything in the top bar at any window width. The progress fills the Export button itself, and the exported count is in its tooltip. When the window is so narrow that Export lives in the "⋯" menu, a thin progress line runs along the bottom edge of the top bar instead.

Below 1200 px wide, notices about a saved recording or a finished export appear in the page between the chart and the Stats panel, so they never cover a reading or the chart. In wider windows they still float over the side panel.

After a disconnect, the "Meter disconnected, showing the last session" message sits in the line above the plot, where the hover readout appears while you point at the chart.

## Reading short ranges and slow rates

The range status that screen readers announce now writes as many decimals as the visible span needs (`rangeDecimals`, used by `rangeAnnouncement`): one decimal from 1 s up, two from 100 ms, three below that, so a 40 ms view reads "Paused: 11.200 s to 11.240 s".

The overview strip draws slow stretches (10 or 50 samples per second over a long session) as continuous lines instead of dots. Gaps and breaks are still left open.
