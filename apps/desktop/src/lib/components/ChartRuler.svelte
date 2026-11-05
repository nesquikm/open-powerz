<script lang="ts">
  import { untrack } from "svelte";
  import type uPlot from "uplot";
  import { formatSessionTime } from "../chartSeries";
  import type { TimeRange } from "../chartView";
  import {
    moveRulerCursor,
    rulerCursorSide,
    rulerKey,
    rulerTag,
    rulerTagOffsets,
    type RulerCursorId,
  } from "../ruler";
  import type { RulerState } from "../rulerState.svelte";

  /** The ruler's two cursors over a plot (FR AXHK2T): sliders a mouse drags and the keys
   * move. A layer over the plot's container; the plot itself is not touched. A cursor out
   * of view stays focusable and shows at the near edge with its name and time. Only the
   * 24 px grip below `laneTopPx` takes the pointer, so clicks elsewhere reach the plot. */
  let {
    ruler,
    plot,
    bounds,
    originMs = () => 0,
    laneTopPx = 0,
  }: {
    ruler: RulerState;
    /** The plot the cursors ride on, or null while there is none. */
    plot: () => uPlot | null;
    /** Where a cursor may go. */
    bounds: () => TimeRange;
    /** Where the time the cursors announce counts from. */
    originMs?: () => number;
    /** The top strip of the plot that keeps its own clicks (the PD marker lane), CSS px. */
    laneTopPx?: number;
  } = $props();

  type Side = "before" | "in" | "after";

  let layer: HTMLDivElement | undefined = $state();
  /** The plot area inside the layer, CSS px, and each cursor's x in it and side of the view. */
  let area = $state({ left: 0, top: 0, width: 0, height: 0 });
  let at: Record<RulerCursorId, { x: number; side: Side }> = $state({
    t1: { x: 0, side: "in" },
    t2: { x: 0, side: "in" },
  });

  /** Puts the cursors where the plot's scale says; the chart calls it on every draw. */
  export function layout(): void {
    const u = plot();
    const r = ruler.ruler;
    if (u === null || layer === undefined || r === null) return;
    const over = u.over.getBoundingClientRect();
    const own = layer.getBoundingClientRect();
    const next = {
      left: over.left - own.left,
      top: over.top - own.top,
      width: over.width,
      height: over.height,
    };
    if (
      next.left !== area.left ||
      next.top !== area.top ||
      next.width !== area.width ||
      next.height !== area.height
    ) {
      area = next;
    }
    const min = u.scales.x?.min;
    const max = u.scales.x?.max;
    const place = (ms: number): { x: number; side: Side } => {
      if (min == null || max == null) return { x: 0, side: "in" };
      const side = rulerCursorSide(ms, { t0Ms: min, t1Ms: max });
      if (side === "before") return { x: 0, side };
      if (side === "after") return { x: next.width, side };
      return { x: u.valToPos(ms, "x"), side };
    };
    const t1 = place(r.t1Ms);
    const t2 = place(r.t2Ms);
    if (t1.x !== at.t1.x || t1.side !== at.t1.side || t2.x !== at.t2.x || t2.side !== at.t2.side) {
      at = { t1, t2 };
    }
  }

  /** Each cursor tag's element, and how far it is moved down so no two tags touch. */
  const tagEls: Partial<Record<RulerCursorId, HTMLSpanElement>> = $state({});
  let tagDy: Record<RulerCursorId, number> = $state({ t1: 0, t2: 0 });

  // Once the tags are drawn where the cursors now are, move apart any that touch
  // (AC-AXHK2V.14). Measured as if unmoved: a tag's own offset is taken off its box.
  $effect(() => {
    void cursors;
    void area;
    const ids: RulerCursorId[] = ["t1", "t2"];
    untrack(() => {
      const boxes = [];
      for (const id of ids) {
        const el = tagEls[id];
        if (el === undefined) return;
        const b = el.getBoundingClientRect();
        const dy = tagDy[id];
        boxes.push({ left: b.left, top: b.top - dy, right: b.right, bottom: b.bottom - dy });
      }
      const [d1 = 0, d2 = 0] = rulerTagOffsets(boxes);
      if (d1 !== tagDy.t1 || d2 !== tagDy.t2) tagDy = { t1: d1, t2: d2 };
    });
  });

  // A moved cursor goes where it now is at once, not on the next draw.
  $effect(() => {
    void ruler.ruler;
    untrack(layout);
  });

  const cursors = $derived.by(() => {
    const r = ruler.ruler;
    if (r === null) return [];
    const b = bounds();
    const origin = originMs();
    const one = (id: RulerCursorId, name: string, ms: number) => {
      const text = formatSessionTime(Math.max(0, ms - origin), 3);
      const side = at[id].side;
      return {
        id,
        name,
        x: at[id].x,
        side,
        now: Math.round(ms - origin),
        text,
        valueText:
          side === "before"
            ? `${text}, before the view`
            : side === "after"
              ? `${text}, after the view`
              : text,
        min: Math.round(b.t0Ms - origin),
        max: Math.round(b.t1Ms - origin),
      };
    };
    return [one("t1", "T1", r.t1Ms), one("t2", "T2", r.t2Ms)];
  });

  function timeAt(u: uPlot, clientX: number): number {
    const over = u.over.getBoundingClientRect();
    return u.posToVal(Math.min(Math.max(clientX - over.left, 0), over.width), "x");
  }

  function onMouseDown(e: MouseEvent, id: RulerCursorId): void {
    if (e.button !== 0 || plot() === null) return;
    // Captured by neither the plot's own drags nor uPlot's: this press is the cursor's.
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as HTMLElement).focus({ preventScroll: true });
    const onMove = (ev: MouseEvent): void => {
      // The plot may be rebuilt mid-drag (a theme or option change): read the one there
      // is now, and stop when there is none.
      const u = plot();
      const r = ruler.ruler;
      if (ev.buttons === 0 || u === null || r === null) {
        end();
        return;
      }
      ruler.set(moveRulerCursor(r, id, timeAt(u, ev.clientX), bounds()));
    };
    const end = (): void => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", end);
      window.removeEventListener("blur", end);
      ruler.settle();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", end);
    window.addEventListener("blur", end);
  }

  function onKeyDown(e: KeyboardEvent, id: RulerCursorId): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const u = plot();
    const r = ruler.ruler;
    const min = u?.scales.x?.min;
    const max = u?.scales.x?.max;
    if (u === null || r === null || min == null || max == null) return;
    const next = rulerKey(
      r,
      id,
      e.key,
      e.shiftKey,
      { t0Ms: min, t1Ms: max },
      bounds(),
      u.over.clientWidth,
    );
    if (next === null) return;
    // These keys move this cursor only: not the playhead, not the view, not an app-wide
    // shortcut.
    e.preventDefault();
    e.stopPropagation();
    ruler.set(next);
  }
</script>

<div class="ruler-layer" bind:this={layer}>
  {#each cursors as c (c.id)}
    <div
      class="ruler-cursor"
      class:edge-before={c.side === "before"}
      class:edge-after={c.side === "after"}
      role="slider"
      tabindex="0"
      aria-label="Ruler cursor {c.name}"
      aria-orientation="horizontal"
      aria-valuemin={c.min}
      aria-valuemax={c.max}
      aria-valuenow={c.now}
      aria-valuetext={c.valueText}
      style:left="{area.left + c.x}px"
      style:top="{area.top}px"
      style:height="{area.height}px"
      onmousedown={(e) => onMouseDown(e, c.id)}
      onkeydown={(e) => onKeyDown(e, c.id)}
      onkeyup={() => ruler.settle()}
    >
      <span class="grip" style:top="{laneTopPx}px"></span>
      <span
        class="tag"
        aria-hidden="true"
        bind:this={tagEls[c.id]}
        style:top="{laneTopPx + tagDy[c.id]}px">{rulerTag(c.name, c.side, c.text)}</span
      >
    </div>
  {/each}
</div>

<style>
  .ruler-layer {
    position: absolute;
    inset: 0;
    z-index: 2;
    pointer-events: none;
    overflow: hidden;
  }
  .ruler-cursor {
    position: absolute;
    /* WCAG 2.5.8: a 24 px target. */
    width: 24px;
    transform: translateX(-50%);
    pointer-events: none;
  }
  .ruler-cursor::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 11px;
    border-left: 1px dashed var(--accent, Highlight);
  }
  .ruler-cursor.edge-before::before,
  .ruler-cursor.edge-after::before {
    border-left-width: 3px;
    border-left-style: solid;
    left: 10px;
  }
  .grip {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    cursor: ew-resize;
    pointer-events: auto;
  }
  .ruler-cursor:focus-visible {
    outline: 2px solid var(--accent, Highlight);
    outline-offset: 0;
  }
  .tag {
    position: absolute;
    left: 14px;
    padding: 0 0.25rem;
    border-radius: 0.25rem;
    background: var(--accent, Highlight);
    color: var(--bg, Canvas);
    font-size: 0.7rem;
    line-height: 1.3;
    white-space: nowrap;
    pointer-events: none;
  }
  .edge-after .tag {
    left: auto;
    right: 14px;
  }
</style>
