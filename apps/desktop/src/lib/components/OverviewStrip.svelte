<script lang="ts" module>
  import type { ChartFrame } from "../chartData";
  import type { SessionExtent, TimeRange } from "../chartView";

  /** What the strip shows: the range on screen within the session, and where session
   * time starts (for the slider's value text). */
  export interface OverviewState {
    range: TimeRange;
    session: SessionExtent;
    originMs: number;
  }

  /** A frame to paint, up to (not including) `beforeMs`. */
  export interface EnvelopeSource {
    frame: ChartFrame;
    beforeMs: number;
  }

  /** The viewport window, CSS px from the strip's left edge. */
  export interface OverviewBox {
    leftPx: number;
    widthPx: number;
  }

  /** A collapsed gap on the strip: its box (CSS px) and its length (AC-FENCD9.11). */
  export interface OverviewBreak extends OverviewBox {
    label: string;
  }
</script>

<script lang="ts">
  import { RateGate } from "../chartView";
  import {
    dragOverview,
    overviewClick,
    overviewWindow,
    type OverviewDrag,
    type TimeWarp,
  } from "../timeline";
  import { THEME_COLORS, type ResolvedTheme } from "../theme";
  import { formatDuration } from "../chartSeries";

  /**
   * The overview strip under the plot: the whole session's VBUS / IBUS envelope and a
   * window over the range on screen (AC-FENCD9.1), dragged, resized, clicked or moved
   * with the keyboard to change that range (AC-FENCD9.2, AC-FENCD9.13).
   */
  let {
    theme,
    current,
    warpFor,
    gapsVersion,
    sources,
    onrange,
    onresize,
    onbox,
    onbreaks,
  }: {
    theme: ResolvedTheme;
    /** The state right now, or `null` before any range. */
    current: () => OverviewState | null;
    /** The warp for `[t0Ms, t1Ms]` drawn across `widthPx`, as the plot builds it. */
    warpFor: (t0Ms: number, t1Ms: number, widthPx: number) => TimeWarp;
    /** Changes whenever the session's gaps do (the strip's warp is cached on it). */
    gapsVersion: () => number;
    /** The frames the envelope is painted from, decimated to `cols` where it applies. */
    sources: (cols: number) => readonly EnvelopeSource[];
    /** The strip asks for this range on screen (paused). */
    onrange: (range: TimeRange) => void;
    /** The strip changed size. */
    onresize: () => void;
    /** The window after each draw, `null` with none (for the e2e hook). */
    onbox?: (box: OverviewBox | null) => void;
    /** The collapsed gaps after each draw (for the e2e hook). */
    onbreaks?: (breaks: readonly OverviewBreak[]) => void;
  } = $props();

  let el: HTMLDivElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  /** The viewport window's element, focused by a pointer press on the strip. */
  let windowEl: HTMLDivElement | undefined = $state();
  /** The window took focus from a pointer press: no focus ring until focus leaves. */
  let pointerFocus = $state(false);
  /** The viewport window; `null` before any range. */
  let box: OverviewBox | null = $state(null);
  /** The window's range as the slider's value text: "0:10 to 0:20". */
  let valueText = $state("");
  /** The window's start and its latest possible start, in session seconds to a tenth,
   * for aria-valuenow / max. */
  let valueNow = $state(0);
  let valueMax = $state(0);
  /** Envelope repaints while the session grows: at most 4 per second. */
  const paintGate = new RateGate(250);
  /** What the last envelope was painted from; a change repaints at once. */
  let paintKey = "";

  /** The window's edge handles' width (and its minimum width), CSS px. */
  const HANDLE_PX = 6;

  /** Session time as m:ss (h as minutes), with tenths for a short view. */
  function clockText(ms: number, tenths: boolean): string {
    const total = Math.max(0, ms) / 1000;
    const minutes = Math.floor(total / 60);
    const seconds = total - minutes * 60;
    const sec = tenths ? seconds.toFixed(1) : String(Math.floor(seconds));
    return `${minutes}:${sec.padStart(tenths ? 4 : 2, "0")}`;
  }

  /** The whole-session warp the strip was last painted with; rebuilt with each paint,
   * so the window moves every frame on a map at most a quarter-second old. */
  let stripWarp: TimeWarp | null = null;

  /** Repositions the window for `state` and repaints the envelope when the session, its
   * gaps, the size or `key` changed, else at the gate's pace (one paint per chart draw
   * at most). */
  export function draw(state: OverviewState | null, key: string): void {
    if (!el || !canvas || state === null) {
      if (box !== null) box = null;
      onbox?.(null);
      // Nothing to show (a clear, a new session): forget what was painted, so the old
      // envelope and its breaks go now, not at the next sample (FR 5X154V).
      if (state === null && paintKey !== "") clearStrip();
      return;
    }
    const { range, session, originMs } = state;
    const width = el.clientWidth;
    const height = el.clientHeight;
    const fullKey = `${session.firstMs}|${width}|${height}|${theme}|${gapsVersion()}|${key}`;
    const now = performance.now();
    let paintDue: boolean;
    if (fullKey === paintKey && stripWarp !== null) {
      // Only the newest data moved: repaint at the gate's pace.
      paintDue = paintGate.ready(now);
    } else {
      paintGate.reset();
      paintGate.ready(now);
      paintKey = fullKey;
      paintDue = true;
    }
    if (paintDue || stripWarp === null) {
      stripWarp = warpFor(session.firstMs, session.lastMs, width);
      stripBreaks = breaksOn(stripWarp, session, width);
      onbreaks?.(stripBreaks);
    }
    const owarp = stripWarp;
    const next = overviewWindow(range, session, width, owarp);
    if (
      box === null ||
      Math.abs(box.leftPx - next.leftPx) > 0.25 ||
      Math.abs(box.widthPx - next.widthPx) > 0.25
    ) {
      box = next;
    }
    onbox?.(next);
    const tenths = range.t1Ms - range.t0Ms < 10_000;
    const text = `${clockText(range.t0Ms - originMs, tenths)} to ${clockText(range.t1Ms - originMs, tenths)}`;
    if (text !== valueText) valueText = text;
    // The window's start can reach the session end less its span.
    const seconds = (ms: number): number => Math.round(Math.max(0, ms - originMs) / 100) / 10;
    valueNow = seconds(range.t0Ms);
    valueMax = Math.max(valueNow, seconds(session.lastMs - (range.t1Ms - range.t0Ms)));
    if (paintDue) paint(canvas, width, height, session, owarp);
  }

  /** Wipes the strip: no envelope, no breaks, no window text. */
  function clearStrip(): void {
    paintKey = "";
    stripWarp = null;
    stripBreaks = [];
    onbreaks?.([]);
    valueText = "";
    valueNow = 0;
    valueMax = 0;
    canvas?.getContext("2d")?.clearRect(0, 0, canvas.width, canvas.height);
  }

  /** The strip's collapsed gaps, as drawn last. */
  let stripBreaks: OverviewBreak[] = [];

  /** Where each collapsed gap sits on a strip `width` CSS px wide, and its length. */
  function breaksOn(owarp: TimeWarp, session: SessionExtent, width: number): OverviewBreak[] {
    const d0 = owarp.fwd(session.firstMs);
    const d1 = owarp.fwd(session.lastMs);
    if (!(d1 > d0)) return [];
    const x = (t: number): number => ((owarp.fwd(t) - d0) / (d1 - d0)) * width;
    return owarp.breaks.map((b) => {
      const leftPx = x(b.fromMs);
      return { leftPx, widthPx: x(b.toMs) - leftPx, label: formatDuration(b.toMs - b.fromMs) };
    });
  }

  /** Marks each collapsed gap: a faint band, two slanted strokes, and its length when it
   * fits (AC-FENCD9.11). */
  function paintBreaks(ctx: CanvasRenderingContext2D, h: number, ratio: number): void {
    const palette = THEME_COLORS[theme];
    ctx.save();
    ctx.font = `${Math.round(10 * ratio)}px system-ui, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const b of stripBreaks) {
      const x0 = b.leftPx * ratio;
      const x1 = (b.leftPx + b.widthPx) * ratio;
      ctx.globalAlpha = 0.18;
      ctx.fillStyle = palette.muted;
      ctx.fillRect(x0, 0, Math.max(ratio, x1 - x0), h);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = palette.muted;
      ctx.lineWidth = ratio;
      for (const xs of [x0, x1]) {
        ctx.beginPath();
        ctx.moveTo(xs - 2 * ratio, h);
        ctx.lineTo(xs + 2 * ratio, 0);
        ctx.stroke();
      }
      ctx.fillStyle = palette.muted;
      if (ctx.measureText(b.label).width + 4 * ratio < x1 - x0) {
        ctx.fillText(b.label, (x0 + x1) / 2, h / 2);
      }
    }
    ctx.restore();
  }

  /** Paints the per-pixel min / max of VBUS and IBUS over the whole session. */
  function paint(
    canvas: HTMLCanvasElement,
    width: number,
    height: number,
    session: SessionExtent,
    owarp: TimeWarp,
  ): void {
    const ratio = globalThis.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(width * ratio));
    const h = Math.max(1, Math.round(height * ratio));
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, w, h);
    const cols = Math.max(1, Math.floor(width));
    const d0 = owarp.fwd(session.firstMs);
    const d1 = owarp.fwd(session.lastMs);
    if (!(d1 > d0)) return;
    const keys = ["vbus", "ibus"] as const;
    const lo = keys.map(() => new Float64Array(cols).fill(Infinity));
    const hi = keys.map(() => new Float64Array(cols).fill(-Infinity));
    for (const { frame, beforeMs } of sources(cols)) {
      for (let i = 0; i < frame.x.length; i++) {
        const t = frame.x[i]!;
        if (t >= beforeMs) break;
        const c = Math.min(
          cols - 1,
          Math.max(0, Math.floor(((owarp.fwd(t) - d0) / (d1 - d0)) * cols)),
        );
        keys.forEach((k, s) => {
          const v = frame.y[k][i];
          if (v === null || v === undefined) return;
          if (v < lo[s]![c]!) lo[s]![c] = v;
          if (v > hi[s]![c]!) hi[s]![c] = v;
        });
      }
    }
    const palette = THEME_COLORS[theme];
    const colors = [palette.chart.voltage, palette.chart.current];
    keys.forEach((_k, s) => {
      let min = Infinity;
      let max = -Infinity;
      for (let c = 0; c < cols; c++) {
        if (lo[s]![c]! < min) min = lo[s]![c]!;
        if (hi[s]![c]! > max) max = hi[s]![c]!;
      }
      if (!(max >= min)) return;
      const spanV = max - min || 1;
      const pad = 2 * ratio;
      const yOf = (v: number): number => h - pad - ((v - min) / spanV) * (h - 2 * pad);
      ctx.save();
      ctx.globalAlpha = 0.75;
      ctx.fillStyle = colors[s] ?? palette.text;
      for (let c = 0; c < cols; c++) {
        const a = lo[s]![c]!;
        const b = hi[s]![c]!;
        if (!(b >= a)) continue;
        const top = yOf(b);
        ctx.fillRect(c * ratio, top, Math.max(1, ratio), Math.max(ratio, yOf(a) - top));
      }
      ctx.restore();
    });
    paintBreaks(ctx, h, ratio);
  }

  // The strip's own size changes redraw it.
  $effect(() => {
    const strip = el;
    if (!strip) return;
    const observer = new ResizeObserver(() => onresize());
    observer.observe(strip);
    return () => observer.disconnect();
  });

  /** A drag in progress: what it moves, where it started and the strip's map at that
   * moment, so each step is one small pure computation (AC-FENCD9.2). */
  let drag: {
    kind: OverviewDrag;
    pointerId: number;
    startX: number;
    start: TimeRange;
    session: SessionExtent;
    widthPx: number;
    warp: TimeWarp;
  } | null = null;

  /** The strip's geometry for the range on screen, or `null` before any range. */
  function geometry(): {
    range: TimeRange;
    session: SessionExtent;
    widthPx: number;
    warp: TimeWarp;
  } | null {
    const state = current();
    if (!el || state === null) return null;
    const widthPx = el.clientWidth;
    if (!(widthPx > 0)) return null;
    const { range, session } = state;
    return { range, session, widthPx, warp: warpFor(session.firstMs, session.lastMs, widthPx) };
  }

  /** Pointer down: the window moves it, a handle resizes from that side, anywhere else
   * centres the view there; each pauses the chart (AC-FENCD9.2). */
  function onPointerDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    const strip = el;
    const geo = geometry();
    if (!strip || geo === null) return;
    const target = e.target as Element | null;
    const { range, session, widthPx, warp } = geo;
    e.preventDefault();
    // The press lands the keyboard on the window, as a click on any slider would.
    // No focus ring for a focus the mouse gave (until focus leaves the window).
    pointerFocus = true;
    windowEl?.focus({ preventScroll: true });
    const grabbed = target?.closest(".overview-window");
    if (grabbed == null) {
      const x = e.clientX - strip.getBoundingClientRect().left;
      onrange(overviewClick(x, range, session, widthPx, warp));
      return;
    }
    // The edges by position, so a window narrower than its two handles still moves
    // when grabbed in the middle.
    const win = grabbed.getBoundingClientRect();
    const x = e.clientX - win.left;
    const edge = Math.min(HANDLE_PX, win.width / 3);
    const kind: OverviewDrag = x < edge ? "left" : x > win.width - edge ? "right" : "move";
    drag = {
      kind,
      pointerId: e.pointerId,
      startX: e.clientX,
      start: range,
      session,
      widthPx,
      warp,
    };
    strip.setPointerCapture?.(e.pointerId);
  }

  function onPointerMove(e: PointerEvent): void {
    const d = drag;
    if (d === null || e.pointerId !== d.pointerId) return;
    onrange(dragOverview(d.start, d.kind, e.clientX - d.startX, d.session, d.widthPx, d.warp));
  }

  function onPointerUp(e: PointerEvent): void {
    const d = drag;
    if (d === null || e.pointerId !== d.pointerId) return;
    drag = null;
    if (el?.hasPointerCapture?.(e.pointerId)) el.releasePointerCapture(e.pointerId);
  }

  /** Keys on the focused window (AC-FENCD9.13): ← / → move it by a tenth of its width,
   * Shift+← / → shrink or grow it from the right edge. The global shortcuts do not see
   * these keys. */
  function onKeydown(e: KeyboardEvent): void {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    e.stopPropagation();
    const geo = geometry();
    if (geo === null) return;
    const { range, session, widthPx, warp } = geo;
    const shown = overviewWindow(range, session, widthPx, warp);
    const step = Math.max(2, shown.widthPx / 10) * (e.key === "ArrowLeft" ? -1 : 1);
    onrange(dragOverview(range, e.shiftKey ? "right" : "move", step, session, widthPx, warp));
  }
</script>

<!-- Pointer shortcut only: the window inside is the keyboard slider for the same actions. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="overview"
  bind:this={el}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerUp}
>
  <canvas class="overview-canvas" bind:this={canvas} aria-hidden="true"></canvas>
  {#if box !== null}
    <div
      class="overview-window"
      bind:this={windowEl}
      class:pointer-focus={pointerFocus}
      onblur={() => (pointerFocus = false)}
      role="slider"
      aria-label="Visible range"
      aria-valuetext={valueText}
      aria-valuenow={valueNow}
      aria-valuemin={0}
      aria-valuemax={valueMax}
      tabindex="0"
      onkeydown={onKeydown}
      style:left="min({box.leftPx}px, calc(100% - {Math.max(HANDLE_PX, box.widthPx)}px))"
      style:width="{box.widthPx}px"
    >
      <span class="overview-handle left"></span>
      <span class="overview-handle right"></span>
    </div>
  {/if}
</div>

<style>
  .overview {
    position: relative;
    flex: none;
    height: 2.5rem;
    min-width: 0;
    border: 1px solid var(--border, GrayText);
    border-radius: 0.25rem;
    background: var(--surface, Canvas);
    overflow: hidden;
    cursor: pointer;
    touch-action: none;
    user-select: none;
  }
  .overview-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .overview-window {
    position: absolute;
    top: 0;
    bottom: 0;
    /* Never thinner than its two handles, so it stays visible and grabbable. */
    min-width: 6px;
    box-sizing: border-box;
    border: 1px solid var(--accent, Highlight);
    background: color-mix(in srgb, var(--accent, Highlight) 18%, transparent);
    cursor: grab;
  }
  /* A focus the pointer gave shows no ring until focus leaves the window. */
  .overview-window.pointer-focus:focus-visible {
    outline: none;
  }
  .overview-window:focus-visible:not(.pointer-focus) {
    outline: 2px solid var(--focus, Highlight);
    outline-offset: 1px;
  }
  .overview-handle {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 6px;
    cursor: ew-resize;
  }
  .overview-handle.left {
    left: -3px;
  }
  .overview-handle.right {
    right: -3px;
  }
</style>
