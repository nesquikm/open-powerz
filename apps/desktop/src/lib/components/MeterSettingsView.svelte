<script lang="ts">
  import { meterSettingRows, type MeterSettingsPanel } from "../meterSettings.svelte";

  let {
    panel,
    connected,
  }: {
    panel: MeterSettingsPanel;
    /** A meter is connected, so its settings can be read. */
    connected: boolean;
  } = $props();

  const rows = $derived(panel.settings ? meterSettingRows(panel.settings) : []);
</script>

<section class="meter-settings" aria-labelledby="meter-settings-title">
  <div class="head">
    <!-- Not a heading: the card's one heading is Stats; the region carries the name. -->
    <span class="title" id="meter-settings-title">Meter settings</span>
    <button
      type="button"
      onclick={() => void panel.refresh()}
      disabled={!connected || panel.loading}
    >
      Refresh
    </button>
  </div>
  {#if !connected}
    <p class="hint">Connect a meter to read its settings.</p>
  {:else if panel.error}
    <p class="error" role="alert">{panel.error.message}</p>
  {:else if rows.length === 0}
    <p class="hint">{panel.loading ? "Reading…" : "No settings read yet."}</p>
  {:else}
    <dl>
      {#each rows as row (row.key)}
        <div class="row">
          <dt>{row.label}</dt>
          <dd>
            <span class="value">{row.value}</span>
            <span class="tag" data-tag={row.tag}>{row.tagText}</span>
          </dd>
        </div>
      {/each}
    </dl>
  {/if}
</section>

<style>
  .meter-settings {
    min-width: 0;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: 0.75rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--border);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .title {
    font-size: 0.875rem;
    font-weight: 600;
  }
  p {
    margin: 0;
    font-size: 0.875rem;
  }
  .hint {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
  }
  dl {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    font-size: 0.8rem;
  }
  .row {
    display: grid;
    gap: 0.1rem;
    min-width: 0;
  }
  .row + .row {
    border-top: 1px solid var(--border);
    padding-top: 0.35rem;
  }
  dt {
    color: var(--muted);
  }
  dd {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 0.5rem;
    margin: 0;
    min-width: 0;
  }
  .value {
    min-width: 0;
    overflow-wrap: anywhere;
    font-variant-numeric: tabular-nums;
  }
  .tag {
    flex: none;
    font-size: 0.7rem;
    padding: 0 0.35rem;
    border: 1px solid var(--border);
    border-radius: 0.25rem;
    color: var(--muted);
  }
  .tag[data-tag="confirmed"] {
    color: var(--text);
  }
</style>
