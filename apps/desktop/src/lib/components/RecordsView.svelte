<script lang="ts">
  import {
    formatRecordDuration,
    recordRows,
    type RecordMinMaxAvg,
    type RecordsPanel,
  } from "../records.svelte";
  import type { ResolvedTheme } from "../theme";
  import RecordChart from "./RecordChart.svelte";

  let {
    panel,
    connected,
    theme,
  }: {
    panel: RecordsPanel;
    /** A meter is connected, so its records can be read. */
    connected: boolean;
    theme: ResolvedTheme;
  } = $props();

  const rows = $derived(panel.records ? recordRows({ records: panel.records }) : []);
  const busy = $derived(panel.loading || panel.downloading !== null || panel.progress !== null);
  const anyDownloadable = $derived(rows.some((r) => r.downloadable));
  const stats = $derived(panel.openedStats);

  function fmt(v: number, decimals: number): string {
    return v.toFixed(decimals);
  }
  function triple(m: RecordMinMaxAvg | null, decimals: number): [string, string, string] {
    return m ? [fmt(m.min, decimals), fmt(m.max, decimals), fmt(m.avg, decimals)] : ["—", "—", "—"];
  }
</script>

{#if panel.opened && stats}
  {@const rec = panel.opened}
  <section class="record" aria-label="Offline record">
    <div class="head">
      <h2>{rec.name}</h2>
      <button type="button" onclick={() => panel.close()}>Back to records</button>
    </div>
    <dl class="totals">
      <div>
        <dt>Duration</dt>
        <dd>{formatRecordDuration(stats.duration_s)}</dd>
      </div>
      <div>
        <dt>Samples</dt>
        <dd>{stats.samples}</dd>
      </div>
      <div>
        <dt>Interval</dt>
        <dd>{rec.interval_s} s</dd>
      </div>
      <div>
        <dt>Charge</dt>
        <dd>{fmt(stats.charge_ah, 6)} Ah</dd>
      </div>
      <div>
        <dt>Energy</dt>
        <dd>{fmt(stats.energy_wh, 6)} Wh</dd>
      </div>
    </dl>
    <table class="stats">
      <thead>
        <tr
          ><th scope="col">Channel</th><th scope="col">Min</th><th scope="col">Max</th><th
            scope="col">Avg</th
          ></tr
        >
      </thead>
      <tbody>
        {#each [["VBUS (V)", triple(stats.vbus, 3)], ["IBUS (A)", triple(stats.ibus, 4)], ["Power (W)", triple(stats.power, 3)]] as const as [label, [min, max, avg]] (label)}
          <tr><th scope="row">{label}</th><td>{min}</td><td>{max}</td><td>{avg}</td></tr>
        {/each}
      </tbody>
    </table>
    <RecordChart record={rec} {theme} />
  </section>
{:else}
  <section class="records" aria-label="Offline records">
    <div class="head">
      <h2>Offline records</h2>
      <div class="actions">
        {#if panel.progress}
          <span class="progress" role="status">
            Downloading {panel.progress.done + 1} of {panel.progress.total}…
          </span>
        {:else if panel.lastAll !== null}
          <span class="progress" role="status">
            Downloaded {panel.lastAll} of {panel.lastAll}
          </span>
        {/if}
        <button type="button" onclick={() => void panel.refresh()} disabled={!connected || busy}>
          Refresh
        </button>
        {#if connected && anyDownloadable}
          <button type="button" onclick={() => void panel.downloadAll()} disabled={busy}>
            Download all
          </button>
        {/if}
      </div>
    </div>
    {#if panel.error}
      <p class="error" role="alert">{panel.error.message}</p>
    {/if}
    {#if !connected}
      <p class="hint">Connect a meter to list its offline records.</p>
    {:else if rows.length === 0}
      <p class="hint">
        {panel.loading ? "Reading the record list…" : panel.error ? "" : "No records on the meter."}
      </p>
    {:else}
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th scope="col">#</th>
              <th scope="col">Name</th>
              <th scope="col">Interval</th>
              <th scope="col">Points</th>
              <th scope="col">Duration</th>
              <th scope="col">Charge</th>
              <th scope="col">Energy</th>
              <th scope="col">Offset</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each rows as row (row.index)}
              <tr>
                <td>{row.index}</td>
                <td class="name">{row.name}</td>
                <td>{row.interval}</td>
                <td>{row.points}</td>
                <td>{row.duration}</td>
                <td>{row.charge}</td>
                <td>{row.energy}</td>
                <td class="offset">{row.offset}</td>
                <td class="row-actions">
                  {#if !row.downloadable}
                    <span class="muted">empty</span>
                  {:else}
                    {#if panel.isDownloaded(row.index)}
                      <span class="done">Downloaded</span>
                    {:else if panel.downloading === row.index}
                      <span class="muted">Downloading…</span>
                    {:else}
                      <button
                        type="button"
                        onclick={() => void panel.download(row.index)}
                        disabled={busy}
                        aria-label="Download {row.name}">Download</button
                      >
                    {/if}
                    <button
                      type="button"
                      onclick={() => void panel.open(row.index)}
                      disabled={busy && !panel.isDownloaded(row.index)}
                      aria-label="Open {row.name}">Open</button
                    >
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
{/if}

<style>
  section {
    display: grid;
    align-content: start;
    gap: 0.75rem;
    min-width: 0;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }
  p {
    margin: 0;
    font-size: 0.875rem;
  }
  .hint,
  .muted {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
  }
  .progress {
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
  }
  .table-wrap table {
    width: 100%;
  }
  th,
  td {
    text-align: left;
    padding: 0.3rem 0.6rem;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  th {
    color: var(--muted);
    font-weight: 600;
  }
  .name {
    font-weight: 600;
  }
  .offset {
    font-family: ui-monospace, monospace;
  }
  .row-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .done {
    color: var(--text);
  }
  .totals {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 1.5rem;
    margin: 0;
    font-size: 0.875rem;
  }
  .totals dt {
    color: var(--muted);
  }
  .totals dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
