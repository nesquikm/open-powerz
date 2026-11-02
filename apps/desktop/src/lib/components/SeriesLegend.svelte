<script lang="ts">
  import {
    CHART_SERIES,
    seriesStyles,
    type ChartSeriesKey,
    type SeriesVisibility,
  } from "../chartSeries";
  import type { ResolvedTheme } from "../theme";

  /** The chart legend: one toggle per series, in its line's colour and dash. Shared by the
   * live chart and the recording file view. */
  let {
    theme,
    visibility,
    onToggle,
  }: {
    theme: ResolvedTheme;
    visibility: SeriesVisibility;
    onToggle: (key: ChartSeriesKey) => void;
  } = $props();

  const styles = $derived(seriesStyles(theme));
</script>

<div class="legend" role="group" aria-label="Series">
  {#each CHART_SERIES as s, i (s.key)}
    <button
      type="button"
      class="series"
      aria-pressed={visibility[s.key]}
      title={`Show or hide ${s.label}`}
      onclick={() => onToggle(s.key)}
    >
      <svg class="swatch" width="24" height="8" aria-hidden="true">
        <line
          x1="0"
          y1="4"
          x2="24"
          y2="4"
          stroke={styles[i]?.stroke}
          stroke-width="2"
          stroke-dasharray={styles[i]?.dash.join(" ") || undefined}
        />
      </svg>
      {s.label} <span class="unit">({s.unit})</span>
    </button>
  {/each}
</div>

<style>
  .legend {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }
  .series {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
  }
  /* Off state: muted text (still ≥ 4.5:1), struck through, faded swatch. */
  .series[aria-pressed="false"] {
    color: var(--muted);
    border-style: dashed;
    text-decoration: line-through;
  }
  .series[aria-pressed="false"] .swatch {
    opacity: 0.35;
  }
  .unit {
    color: var(--muted);
  }
</style>
