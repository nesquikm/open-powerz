<script lang="ts">
  import {
    EDITABLE_SETTINGS,
    meterSettingRows,
    validSettingCode,
    type EditableSetting,
    type MeterSettingsPanel,
  } from "../meterSettings.svelte";

  let {
    panel,
    connected,
  }: {
    panel: MeterSettingsPanel;
    /** A meter is connected, so its settings can be read. */
    connected: boolean;
  } = $props();

  const rows = $derived(panel.settings ? meterSettingRows(panel.settings) : []);
  /** Fields whose typed value is not valid, so Apply stays disabled. */
  let invalid = $state<Record<string, boolean>>({});
  const canApply = $derived(
    panel.summary.length > 0 && !panel.applying && !Object.values(invalid).some(Boolean),
  );

  function startEdit(): void {
    invalid = {};
    panel.startEdit();
  }

  function edit(setting: EditableSetting, raw: string): void {
    const code = Number(raw);
    const ok =
      raw.trim() !== "" && validSettingCode(setting, code) && panel.setDraft(setting.key, code);
    invalid = { ...invalid, [setting.key]: !ok };
  }
</script>

<section class="meter-settings" aria-labelledby="meter-settings-title">
  <div class="head">
    <!-- Not a heading: the card's one heading is Stats; the region carries the name. -->
    <span class="title" id="meter-settings-title">Meter settings</span>
    {#if !panel.editing}
      <div class="actions">
        <button
          type="button"
          onclick={() => void panel.refresh()}
          disabled={!connected || panel.loading}
        >
          Refresh
        </button>
        <button
          type="button"
          onclick={startEdit}
          disabled={!connected || panel.loading || !panel.settings}
        >
          Edit
        </button>
      </div>
    {/if}
  </div>
  {#if !connected}
    <p class="hint">Connect a meter to read its settings.</p>
  {:else if panel.editing}
    <form
      class="edit"
      onsubmit={(e) => {
        e.preventDefault();
        if (canApply) void panel.apply();
      }}
    >
      {#each EDITABLE_SETTINGS as setting (setting.key)}
        {@const id = `meter-setting-${setting.key}`}
        <div class="field">
          <label for={id}>{setting.label}</label>
          {#if setting.options}
            <select
              {id}
              value={String(panel.draft[setting.key] ?? "")}
              disabled={panel.applying}
              onchange={(e) => edit(setting, e.currentTarget.value)}
            >
              {#each setting.options as option (option.code)}
                <option value={String(option.code)}>{option.label}</option>
              {/each}
            </select>
          {:else}
            <input
              {id}
              type="number"
              min={setting.min}
              max={setting.max}
              step="1"
              value={String(panel.draft[setting.key] ?? "")}
              disabled={panel.applying}
              aria-invalid={invalid[setting.key] ? "true" : undefined}
              oninput={(e) => edit(setting, e.currentTarget.value)}
            />
          {/if}
        </div>
      {/each}
      {#if panel.summary.length > 0}
        <div class="summary">
          <span>The meter will change:</span>
          <ul>
            {#each panel.summary as line (line)}
              <li>{line}</li>
            {/each}
          </ul>
        </div>
      {:else}
        <p class="hint">Nothing changed yet.</p>
      {/if}
      {#if panel.error}
        <p class="error" role="alert">{panel.error.message}</p>
      {/if}
      <div class="actions">
        <button type="button" onclick={() => panel.cancelEdit()} disabled={panel.applying}>
          Cancel
        </button>
        <button type="submit" disabled={!canApply}>
          {panel.applying ? "Applying…" : "Apply"}
        </button>
      </div>
    </form>
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
  .actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
  }
  .edit {
    display: grid;
    gap: 0.5rem;
    font-size: 0.8rem;
  }
  .field {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 8rem);
    align-items: center;
    gap: 0.5rem;
  }
  .field label {
    color: var(--muted);
  }
  .field input,
  .field select {
    min-width: 0;
  }
  .field input[aria-invalid="true"] {
    border-color: var(--danger);
  }
  .summary ul {
    margin: 0.25rem 0 0;
    padding-left: 1.1rem;
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
