---
title: Chart appearance and image export
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M69QVAS7BY12FMV0Y1AXHK2V
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Added
---

# Chart appearance and image export

## Summary

The chart's look is fixed today, and saving it needs a screenshot.
This adds axis range presets, a title, and colour, width and transparency for each series.
A chart can also be saved as a PNG, SVG or PDF file at a chosen size.
Settings persist across restarts.
Saved images hold no serial number.

## Requirement

The chart gains axis presets, per-series style, a title and image export.

## Acceptance Criteria

- AC-AXHK2V.1: VBUS offers Auto, Auto from zero, 0-6 V, 0-12 V and 0-24 V. IBUS offers Auto, Auto from zero, 0-2 A, 0-5 A and 0-8 A.
- AC-AXHK2V.2: Each series has a colour, line width and transparency setting, reflected in the legend, with a Reset. Dash patterns stay distinct for colour-blind reading.
- AC-AXHK2V.3: A low-contrast colour warns the user but is allowed.
- AC-AXHK2V.4: An editable title shows above the chart and in exports. Its text is rendered as text, never as markup.
- AC-AXHK2V.5: "Save image" asks for width, height and format (PNG, SVG, PDF) and writes that file. Sizes outside 320 to 7680 px are refused.
- AC-AXHK2V.6: The export shows the current view with its title, axes and legend, in the current theme or on white by choice.
- AC-AXHK2V.7: SVG output is valid XML with series as paths. PDF output opens as one page. A test parses both.
- AC-AXHK2V.8: Settings persist per app, not per meter. A README line and the how-to describe them.
- AC-AXHK2V.9: On a 1280 × 800 window the firmware warning card covers no chart, settings control or settings row. A test pins it.
- AC-AXHK2V.10: Saved chart images, their default file names and default chart titles carry no serial or UID, with "Hide serial numbers" on or off (AC-AXHK2Q.5). A test pins it.
- AC-AXHK2V.11: A ruler cursor's edge-marker tag separates its name from its time, as in "◀ T1 · 0.000 s".
- AC-AXHK2V.12: Segment-mark labels never overlap an axis tick label. When they don't fit, they thin out (every other one) or are left out, and the dashed lines stay. Checked at 1280 and 375 px wide.
- AC-AXHK2V.13: The E.D. line has its own colour, distinct from every series colour, and a small "E.D. %" label at its right end, so it can't be read against the volts axis.
- AC-AXHK2V.14: Ruler tags never overlap. An edge-marker tag and an in-view cursor tag that would touch are moved apart, so each name and time stays readable.
- AC-AXHK2V.15: The "E.D. %" label never overlaps a segment-mark label or an axis tick label. Mark labels give way to it.
- AC-AXHK2V.16: The E.D. swatch in the explainer note uses the E.D. line's colour and dash in both themes.

## Technical Design

`chartStyle.ts` holds the settings and feeds the uPlot options. PNG renders from an offscreen uPlot. SVG renders from the frame in a pure module. PDF wraps the PNG in a small hand-written writer. The native dialog saves through a Tauri command.

## Testing

Vitest for the settings and SVG, Rust tests for the PDF writer, Playwright and a visual check.

## Notes

ACs .14 to .16 come from the dbc94f8 visual check (qa-runs/AXHK2V-visual): T2's tag covered T1's edge tag, "E.D. %" sat on the 95% mark label, and the note's swatch stayed blue.

## Implementation notes

GIF, BMP and JPG from the vendor list are left out: PNG, SVG and PDF cover the use.
The image holds the series, axes, title and legend of the current view. The chart's overlays (PD markers, segment marks, E.D., ruler, playhead) stay on screen only.
