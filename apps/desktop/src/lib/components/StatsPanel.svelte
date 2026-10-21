<script lang="ts">
  import { isMissing, statsView } from "../stats";
  import type { StatsControls, StatsDeps } from "../stats.svelte";
  import type { StatsSnapshot } from "../backend";
  import Notice from "./Notice.svelte";

  let {
    stats,
    controls,
    deps,
    canReset,
  }: {
    /** Latest snapshot, null before the first batch. */
    stats: StatsSnapshot | null;
    controls: StatsControls;
    deps: StatsDeps;
    /** A stream is running, so there is something to reset or clear. */
    canReset: boolean;
  } = $props();

  let cancelButton = $state<HTMLButtonElement | null>(null);
  let clearButton = $state<HTMLButtonElement | null>(null);
  /** The confirm was open, so focus goes back to "Clear chart history…" when it closes. */
  let confirmWasOpen = false;

  // A confirm left open when the stream ends must not fire into the next session.
  $effect(() => {
    if (!canReset) controls.cancelClear();
  });

  // Keyboard users keep their place: the confirm takes focus on its safe choice,
  // and hands it back to the button that opened it.
  $effect(() => {
    if (controls.clearPending) {
      confirmWasOpen = true;
      cancelButton?.focus();
    } else if (confirmWasOpen && clearButton) {
      confirmWasOpen = false;
      clearButton.focus();
    }
  });

  const view = $derived(statsView(stats));
  const rows = $derived([
    { name: "VBUS", values: view.vbus },
    { name: "IBUS", values: view.ibus },
    { name: "Power", values: view.power },
  ]);

  /** "−0.120 Ah" → digits "−0.120", unit "Ah" (the unit follows the last space). */
  function split(text: string): { digits: string; unit: string } {
    const at = text.lastIndexOf(" ");
    return { digits: text.slice(0, at), unit: text.slice(at + 1) };
  }
</script>

{#snippet value(text: string)}
  {@const parts = split(text)}
  <span class="value"
    ><span class="digits" class:empty={isMissing(text)}>{parts.digits}</span><span class="unit"
      >{parts.unit}</span
    ></span
  >
{/snippet}

<aside class="stats panel" aria-label="Statistics">
  <div class="head">
    <h2>Stats</h2>
    <button
      type="button"
      onclick={() => void controls.reset(deps)}
      disabled={!canReset}
      aria-keyshortcuts="r"
      title="Zero the statistics (min, max, average, charge, energy). The chart history stays."
      >Reset stats</button
    >
  </div>

  {#if controls.error}
    <Notice
      tone="error"
      message={controls.error.message}
      onDismiss={() => (controls.error = null)}
    />
  {/if}

  <dl class="totals">
    <div>
      <dt>Elapsed</dt>
      <dd><span class="value"><span class="digits">{view.elapsed}</span></span></dd>
    </div>
    <div>
      <dt>Charge</dt>
      <dd>{@render value(view.charge)}</dd>
    </div>
    <div>
      <dt>Energy</dt>
      <dd>{@render value(view.energy)}</dd>
    </div>
  </dl>

  <table>
    <thead>
      <tr>
        <th scope="col"><span class="visually-hidden">Quantity</span></th>
        <th scope="col">Min</th>
        <th scope="col">Max</th>
        <th scope="col">Avg</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as row (row.name)}
        <tr>
          <th scope="row">{row.name}</th>
          <td>{@render value(row.values.min)}</td>
          <td>{@render value(row.values.max)}</td>
          <td>{@render value(row.values.avg)}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  <div class="clear">
    {#if controls.clearPending}
      <span class="confirm" role="group" aria-labelledby="clear-question">
        <span id="clear-question"
          >Clear the chart history? The meter stays connected and keeps measuring.</span
        >
        <span class="choices">
          <button
            type="button"
            onclick={() => void controls.confirmClear(deps)}
            disabled={!canReset}>Clear</button
          >
          <button type="button" bind:this={cancelButton} onclick={() => controls.cancelClear()}
            >Cancel</button
          >
        </span>
      </span>
    {:else}
      <button
        type="button"
        bind:this={clearButton}
        onclick={() => controls.requestClear()}
        disabled={!canReset}
        title="Erase the chart history and what Export saves. The meter stays connected; recordings are not affected."
        >Clear chart history…</button
      >
    {/if}
  </div>
</aside>

<style>
  .stats {
    min-width: 0;
    display: grid;
    gap: 0.75rem;
    padding: 1rem 1.25rem;
    font-variant-numeric: tabular-nums;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  /* The Notice brings its own page margins; inside the panel it sits flush. */
  .stats :global(.notice) {
    margin: 0;
  }
  .totals {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.5rem;
    margin: 0;
  }
  .totals div {
    display: grid;
    gap: 0.1rem;
    min-width: 0;
  }
  dt {
    font-size: 0.8rem;
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-size: 0.8rem;
  }
  th,
  td {
    padding: 0.2rem 0;
    text-align: right;
    white-space: nowrap;
  }
  thead th {
    font-weight: 400;
    font-size: 0.8rem;
    color: var(--muted);
  }
  tbody th {
    text-align: left;
    font-weight: 400;
    color: var(--muted);
    width: 3rem;
  }
  tbody tr + tr :where(th, td) {
    border-top: 1px solid var(--border);
  }
  .value {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    font-size: 0.8em;
    margin-left: 0.2em;
    font-weight: 400;
  }
  /* The "—" of a missing value: quieter than a reading, as in the readout. */
  .digits.empty {
    color: var(--muted);
  }
  .clear {
    display: flex;
    justify-content: flex-end;
  }
  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
    font-size: 0.875rem;
  }
  /* The two answers stay side by side; only the question may wrap above them. */
  .choices {
    display: flex;
    gap: 0.5rem;
  }
</style>
