<script lang="ts">
  import type { ChartExtras, MarkKind } from "../chartExtras";
  import { CHART_SERIES } from "../chartSeries";
  import {
    IBUS_PRESETS,
    VBUS_PRESETS,
    lowContrastWarning,
    resetSeriesLook,
    seriesLooks,
    setSeriesLook,
    type ChartStyle,
  } from "../chartStyle";
  import type { ResolvedTheme } from "../theme";
  import MarkOptions from "./MarkOptions.svelte";

  /**
   * The "Chart options" group (FR AXHK2T) of the live chart and of an opened recording.
   * E.D., the sub-chart and the axis ranges are offered only where their handler is given.
   */
  let {
    extras,
    onRuler,
    onMarks,
    onEd,
    onSubChart,
    onReverseCurrent,
    style,
    onStyle,
    theme = "light",
  }: {
    extras: ChartExtras;
    onRuler: () => void;
    onMarks: (marks: MarkKind, step: number) => void;
    onEd?: () => void;
    onSubChart?: () => void;
    onReverseCurrent: () => void;
    /** The chart's appearance settings (FR AXHK2V) and their change handler. */
    style?: ChartStyle;
    onStyle?: (style: ChartStyle) => void;
    /** The theme whose colours a series not yet styled shows. */
    theme?: ResolvedTheme;
  } = $props();

  const looks = $derived(style ? seriesLooks(theme, style) : []);

  /** Each chart has its own selects. */
  const uid = $props.id();
</script>

<div class="chart-options" role="group" aria-label="Chart options">
  <button
    type="button"
    class:primary={extras.ruler}
    aria-pressed={extras.ruler}
    title="Two cursors, T1 and T2, and the time, charge, energy and averages between them"
    onclick={onRuler}>Ruler</button
  >
  <MarkOptions marks={extras.marks} step={extras.markStep} onChange={onMarks} />
  {#if onEd}
    <button
      type="button"
      class:primary={extras.ed}
      aria-pressed={extras.ed}
      title="The energy share: energy so far as a percentage of the session's"
      onclick={onEd}>E.D.</button
    >
  {/if}
  {#if onSubChart}
    <button
      type="button"
      class:primary={extras.subChart}
      aria-pressed={extras.subChart}
      title="A second plot under this one, with its own series, on the same time range"
      onclick={onSubChart}>Sub-chart</button
    >
  {/if}
  <button
    type="button"
    class:primary={extras.reverseCurrent}
    aria-pressed={extras.reverseCurrent}
    title="Draws the current axis upside down; the readings, statistics and exports keep their sign"
    onclick={onReverseCurrent}>Reverse current</button
  >
  {#if style && onStyle}
    <span class="range-option">
      <label for="{uid}-vbus-range">VBUS range</label>
      <select
        id="{uid}-vbus-range"
        value={style.vbusRange}
        onchange={(e) => onStyle({ ...style, vbusRange: e.currentTarget.value })}
      >
        {#each VBUS_PRESETS as p (p.id)}
          <option value={p.id}>{p.label}</option>
        {/each}
      </select>
    </span>
    <span class="range-option">
      <label for="{uid}-ibus-range">IBUS range</label>
      <select
        id="{uid}-ibus-range"
        value={style.ibusRange}
        onchange={(e) => onStyle({ ...style, ibusRange: e.currentTarget.value })}
      >
        {#each IBUS_PRESETS as p (p.id)}
          <option value={p.id}>{p.label}</option>
        {/each}
      </select>
    </span>
    <span class="range-option">
      <label for="{uid}-title">Chart title</label>
      <input
        id="{uid}-title"
        type="text"
        value={style.title}
        oninput={(e) => onStyle({ ...style, title: e.currentTarget.value })}
      />
    </span>
    <div class="series-style" role="group" aria-label="Series style">
      {#each CHART_SERIES as s, i (s.key)}
        {@const look = looks[i]}
        <span class="series-look">
          <span aria-hidden="true">{s.label}</span>
          <input
            type="color"
            aria-label="{s.label} colour"
            value={look?.color}
            oninput={(e) => onStyle(setSeriesLook(style, s.key, { color: e.currentTarget.value }))}
          />
          <label>
            width
            <input
              type="number"
              aria-label="{s.label} width"
              min="0.5"
              max="8"
              step="0.5"
              value={look?.width}
              onchange={(e) => {
                const width = Number(e.currentTarget.value);
                if (Number.isFinite(width) && width > 0) {
                  onStyle(setSeriesLook(style, s.key, { width }));
                }
              }}
            />
          </label>
          <label>
            transparency
            <input
              type="number"
              aria-label="{s.label} transparency"
              min="0"
              max="90"
              step="5"
              value={Math.round((1 - (look?.opacity ?? 1)) * 100)}
              onchange={(e) => {
                const pct = Number(e.currentTarget.value);
                if (Number.isFinite(pct)) {
                  const t = Math.min(90, Math.max(0, pct));
                  onStyle(setSeriesLook(style, s.key, { opacity: 1 - t / 100 }));
                }
              }}
            />
            %
          </label>
          <button type="button" onclick={() => onStyle(resetSeriesLook(style, s.key))}
            >Reset {s.label} style</button
          >
          {#if look}
            {@const warning = lowContrastWarning(look.color, theme)}
            {#if warning}
              <span class="contrast-warning" role="status">{s.label}: {warning}</span>
            {/if}
          {/if}
        </span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .chart-options {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }
  .series-style {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.75rem;
    flex-basis: 100%;
  }
  .series-look {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }
  .contrast-warning {
    color: var(--danger);
    font-size: 0.85em;
  }
  .series-look input[type="number"] {
    width: 4rem;
  }
  .range-option {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }
</style>
