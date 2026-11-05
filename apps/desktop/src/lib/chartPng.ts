/**
 * "Save image" as PNG (FR AXHK2V): the chart drawn again by an offscreen uPlot at the
 * size the dialog asked for, on the theme's background (or white), with the title above
 * and the legend below, laid out as the SVG lays them out. A PDF is this PNG, wrapped in
 * one page by the backend.
 */

import uPlot from "uplot";
import { exportAxisRanges, exportInk, exportLayout, type ExportLayout } from "./chartExport";
import { CHART_AXES, chartOptions, seriesStroke, type ChartOptionsInput } from "./chartSeries";
import { DEFAULT_CHART_STYLE, strokeColor } from "./chartStyle";

export interface ChartPngInput extends Omit<ChartOptionsInput, "width" | "height" | "pxRatio"> {
  width: number;
  height: number;
  title: string;
  white: boolean;
  /** The current view's time span, in x units (ms). */
  view: { t0Ms: number; t1Ms: number };
  /** The live plot's aligned data. */
  data: uPlot.AlignedData;
}

/** Draws `layout`'s title and legend in `fg` on `ctx` (at 1 px per CSS px). */
function drawLayout(ctx: CanvasRenderingContext2D, layout: ExportLayout, fg: string): void {
  if (layout.title !== null) {
    ctx.fillStyle = fg;
    ctx.font = "16px sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
    ctx.fillText(layout.title.text, layout.title.x, layout.title.y);
  }
  ctx.font = "12px sans-serif";
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  for (const { look, label, x0, x1, y, textX } of layout.legend) {
    // Whole-pixel width and dashes, as the live chart snaps them.
    const stroke = seriesStroke(look.key, 1, look.width);
    ctx.save();
    ctx.strokeStyle = strokeColor(look.color, look.opacity);
    ctx.lineWidth = stroke.width;
    ctx.setLineDash(stroke.dash);
    ctx.beginPath();
    ctx.moveTo(x0, y);
    ctx.lineTo(x1, y);
    ctx.stroke();
    ctx.restore();
    ctx.fillStyle = fg;
    ctx.fillText(label, textX, y);
  }
}

/** Renders the chart as a `width` × `height` PNG. */
export async function chartPng(input: ChartPngInput): Promise<Uint8Array> {
  const { width, height, title, white, view, data, ...rest } = input;
  const ink = exportInk(input.theme, white);
  const layout = exportLayout({
    width,
    height,
    title,
    theme: input.theme,
    style: input.style ?? DEFAULT_CHART_STYLE,
    visibility: input.visibility,
  });
  const plotHeight = Math.max(1, height - layout.top - layout.bottom);
  const opts = chartOptions({ ...rest, width, height: plotHeight });
  // The same axis ranges as the SVG of this view.
  const ranges = exportAxisRanges({
    style: input.style ?? DEFAULT_CHART_STYLE,
    visibility: input.visibility,
    view,
    data,
    zeroBased: input.zeroBased,
  });
  opts.scales = {
    ...opts.scales,
    ...Object.fromEntries(
      CHART_AXES.map((a): [string, uPlot.Scale] => [
        a.id,
        { ...opts.scales?.[a.id], range: () => ranges[a.id] },
      ]),
    ),
    x: { ...opts.scales?.x, time: false, range: () => [view.t0Ms, view.t1Ms] },
  };
  // Nothing to point at or drag offscreen.
  opts.cursor = { show: false };
  let drawn: () => void = () => {};
  const firstDraw = new Promise<void>((resolve) => (drawn = resolve));
  opts.hooks = { ...opts.hooks, draw: [...(opts.hooks?.draw ?? []), () => drawn()] };

  const host = document.createElement("div");
  host.setAttribute("aria-hidden", "true");
  Object.assign(host.style, {
    position: "fixed",
    left: "-100000px",
    top: "0",
    width: `${width}px`,
    pointerEvents: "none",
  });
  document.body.appendChild(host);
  const u = new uPlot(opts, data, host);
  try {
    await firstDraw;
    const out = document.createElement("canvas");
    out.width = width;
    out.height = height;
    const ctx = out.getContext("2d");
    if (ctx === null) throw new Error("no canvas to draw the image on");
    ctx.fillStyle = ink.bg;
    ctx.fillRect(0, 0, width, height);
    drawLayout(ctx, layout, ink.fg);
    // The plot's canvas is at the device pixel ratio: scaled to the asked size.
    ctx.drawImage(u.ctx.canvas, 0, layout.top, width, plotHeight);
    const blob = await new Promise<Blob | null>((resolve) => out.toBlob(resolve, "image/png"));
    if (blob === null) throw new Error("the image could not be encoded");
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    u.destroy();
    host.remove();
  }
}
