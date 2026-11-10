<script lang="ts">
  import { features } from "../backend";
  import DesktopOnly from "./DesktopOnly.svelte";
  import InfoButton from "./InfoButton.svelte";
  import Term from "./Term.svelte";
  import {
    EDITABLE_SETTINGS,
    meterSettingRows,
    validSettingCode,
    type EditableSetting,
    type MeterSettingsPanel,
  } from "../meterSettings.svelte";
  import HintText from "./HintText.svelte";

  let {
    panel,
    connected,
    held = false,
    target,
  }: {
    panel: MeterSettingsPanel;
    /** A meter is connected, so its settings can be read. */
    connected: boolean;
    /** A logo write holds the meter (AC-MEHQC0.3): nothing is read or applied until it ends. */
    held?: boolean;
    /** Who Apply writes to (AC-NWGY72.5). */
    target: string;
  } = $props();

  const rows = $derived(panel.settings ? meterSettingRows(panel.settings) : []);
  /** A settings block the meter has not saved yet is marked for the e2e checks (FR QG6RNY). */
  const errorTestId = $derived(
    panel.error?.kind === "settings_not_saved" ? "settings-not-saved" : undefined,
  );
  /** Fields whose typed value is not valid, so Apply stays disabled. */
  let invalid = $state<Record<string, boolean>>({});
  const canApply = $derived(
    !held && panel.summary.length > 0 && !panel.applying && !Object.values(invalid).some(Boolean),
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
    <div class="titled">
      <span class="title" id="meter-settings-title">Meter settings</span>
      <InfoButton key="section.settings" />
    </div>
    {#if !panel.editing}
      <div class="actions">
        <DesktopOnly available={features.settingsRead} label="Refresh">
          <button
            type="button"
            onclick={() => void panel.refresh()}
            disabled={!connected || panel.loading || held}
          >
            Refresh
          </button>
        </DesktopOnly>
        <DesktopOnly available={features.settingsWrite} label="Edit">
          <button
            type="button"
            onclick={startEdit}
            disabled={!connected || panel.loading || !panel.settings}
          >
            Edit
          </button>
        </DesktopOnly>
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
        <p class="error" role="alert" data-testid={errorTestId}>
          {panel.error.message}
          <HintText hint={panel.error.hint} />
        </p>
      {/if}
      <div class="actions">
        <span class="target">Writes to {target}</span>
        <button type="button" onclick={() => panel.cancelEdit()} disabled={panel.applying}>
          Cancel
        </button>
        <button type="submit" disabled={!canApply}>
          {panel.applying ? "Applying…" : "Apply"}
        </button>
      </div>
    </form>
  {:else if panel.error}
    <p class="error" role="alert" data-testid={errorTestId}>
      {panel.error.message}
      <HintText hint={panel.error.hint} />
    </p>
  {:else if rows.length === 0}
    <p class="hint">{panel.loading ? "Reading…" : "No settings read yet."}</p>
  {:else}
    {#if panel.notice}
      <p class="notice" role="status">{panel.notice}</p>
    {/if}
    {#each panel.warnings as warning (warning)}
      <p class="notice" role="status">{warning}</p>
    {/each}
    <dl>
      {#each rows as row (row.key)}
        <div class="row">
          <dt>{row.label}</dt>
          <dd>
            <span class="value"
              >{#if row.value.endsWith(" SPS")}{row.value.slice(0, -4)}
                <Term key="term.sps">SPS</Term>{:else}{row.value}{/if}</span
            >
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
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    margin-left: auto;
  }
  .actions .target {
    margin-right: auto;
    align-self: center;
    font-size: 0.8125rem;
    color: var(--muted);
    overflow-wrap: anywhere;
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
    white-space: nowrap;
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
  .notice {
    color: var(--muted);
    font-size: 0.8rem;
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
