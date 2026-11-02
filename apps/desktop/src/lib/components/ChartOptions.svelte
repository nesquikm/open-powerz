<script lang="ts">
  import type { ChartExtras, MarkKind } from "../chartExtras";
  import MarkOptions from "./MarkOptions.svelte";

  /**
   * The "Chart options" group (FR AXHK2T) of the live chart and of an opened recording.
   * E.D. and the sub-chart are offered only where their handler is given.
   */
  let {
    extras,
    onRuler,
    onMarks,
    onEd,
    onSubChart,
    onReverseCurrent,
  }: {
    extras: ChartExtras;
    onRuler: () => void;
    onMarks: (marks: MarkKind, step: number) => void;
    onEd?: () => void;
    onSubChart?: () => void;
    onReverseCurrent: () => void;
  } = $props();
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
</div>

<style>
  .chart-options {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }
</style>
