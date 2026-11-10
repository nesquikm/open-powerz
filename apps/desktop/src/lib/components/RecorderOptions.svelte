<script lang="ts">
  import InfoButton from "./InfoButton.svelte";
  /**
   * The recorder's options (FR AXHK2R): the file's prefix and folder, auto-stop and the row limit.
   * They are saved here and hold for the next recording, so they lock while a meter
   * records. Opens with the dialog focused, keeps Tab inside, and closes on Escape.
   * Also shows how long the chart's memory lasts at the meter's rate.
   */
  import { untrack } from "svelte";
  import { chooseRecordingFolder, features, toUiError } from "../backend";
  import DesktopOnly from "./DesktopOnly.svelte";
  import { LIVE_CAPACITY } from "../chartData";
  import { memoryDepthText } from "../recording.svelte";
  import { recorderSettingsError, type RecorderSettings } from "../settings";

  let {
    settings,
    locked,
    rateSps,
    onCancel,
    onSave,
  }: {
    settings: RecorderSettings;
    /** A meter records: the fields can be read, not changed. */
    locked: boolean;
    /** The rate of the meter it was opened for, `null` until it is known. */
    rateSps: number | null;
    onCancel: () => void;
    onSave: (settings: RecorderSettings) => void;
  } = $props();

  const saved = untrack(() => settings);
  let prefix = $state(saved.prefix);
  let folder = $state(saved.folder);
  let choosing = $state(false);
  let chooseError = $state<string | null>(null);
  let autoStop = $state(saved.autoStop);
  // An empty number field binds as `null`.
  let belowA = $state(saved.belowA);
  let forSeconds = $state(saved.forSeconds);
  let maxRows = $state(saved.maxRows);

  const draft: RecorderSettings = $derived({
    prefix,
    folder,
    autoStop,
    belowA,
    forSeconds,
    maxRows,
  });
  const invalid = $derived(recorderSettingsError(draft));
  const error = $derived(invalid ?? chooseError);

  async function chooseFolder(): Promise<void> {
    choosing = true;
    chooseError = null;
    try {
      folder = (await chooseRecordingFolder()) ?? folder;
    } catch (err) {
      chooseError = toUiError(err).message;
    } finally {
      choosing = false;
    }
  }

  let dialog: HTMLElement;

  /** Focus the dialog, and give focus back to what opened it on the way out. */
  function focusOnMount(el: HTMLElement): { destroy: () => void } {
    const opener = document.activeElement;
    el.focus();
    return {
      destroy: () => {
        if (opener instanceof HTMLElement) opener.focus();
      },
    };
  }

  /** Runs on the window, ahead of its shortcuts: focus can be outside the dialog, on the
   * page behind it after a click on the backdrop. */
  function onKey(e: KeyboardEvent): void {
    // No key reaches the window shortcuts while the dialog is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    } else if (e.key === "Tab") {
      // Keep focus inside the dialog.
      const controls = Array.from(dialog.querySelectorAll<HTMLElement>("input, button")).filter(
        (el) => !(el as HTMLInputElement).disabled,
      );
      const first = controls[0];
      const last = controls[controls.length - 1];
      const active = document.activeElement;
      const onAControl = active instanceof HTMLElement && controls.includes(active);
      if (e.shiftKey && (active === first || !onAControl)) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && (active === last || !onAControl)) {
        e.preventDefault();
        first?.focus();
      }
    }
  }
</script>

<svelte:window onkeydowncapture={onKey} />

<div class="backdrop">
  <div
    bind:this={dialog}
    class="options panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="recorder-options-title"
    tabindex="-1"
    use:focusOnMount
  >
    <div class="titled">
      <h2 id="recorder-options-title">Recorder options</h2>
      <InfoButton key="section.recorder" />
    </div>
    {#if locked}
      <p class="note">These hold for the next recording: stop every recording to change them.</p>
    {/if}
    <label class="field">
      <span>File name prefix</span>
      <input type="text" placeholder="powerz" bind:value={prefix} disabled={locked} />
    </label>
    <div class="field" role="group" aria-labelledby="recorder-folder-label">
      <span id="recorder-folder-label">Folder</span>
      <div class="folder">
        <span class="path" title={folder ?? undefined}
          >{folder ?? (features.recordingFolder ? "Asked at Stop" : "Downloads")}</span
        >
        <DesktopOnly available={features.recordingFolder} label="Choose">
          <button type="button" disabled={locked || choosing} onclick={chooseFolder}>Choose</button>
          {#if folder !== null}
            <button type="button" disabled={locked} onclick={() => (folder = null)}>Clear</button>
          {/if}
        </DesktopOnly>
      </div>
    </div>
    <label class="check">
      <input type="checkbox" bind:checked={autoStop} disabled={locked} />
      <span>Auto-stop when the current stays low</span>
    </label>
    <label class="field">
      <span>Below (A)</span>
      <input type="number" min="0" step="any" bind:value={belowA} disabled={locked || !autoStop} />
    </label>
    <label class="field">
      <span>For (seconds)</span>
      <input
        type="number"
        min="0"
        step="any"
        bind:value={forSeconds}
        disabled={locked || !autoStop}
      />
    </label>
    <label class="field">
      <span>Row limit</span>
      <input type="number" min="1" step="1" bind:value={maxRows} disabled={locked} />
    </label>
    <p class="note">{memoryDepthText(LIVE_CAPACITY, rateSps)}</p>
    {#if error !== null}
      <p class="error" role="alert">{error}</p>
    {/if}
    <div class="actions">
      <button type="button" onclick={onCancel}>Cancel</button>
      <button
        type="button"
        class="primary"
        disabled={locked || invalid !== null}
        onclick={() => onSave(draft)}>Save</button
      >
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
  }
  .options {
    display: grid;
    box-sizing: border-box;
    gap: 0.75rem;
    width: min(26rem, calc(100vw - 2rem));
    max-height: calc(100dvh - 2rem);
    overflow-y: auto;
    padding: 1rem;
  }
  .options h2 {
    margin: 0;
    font-size: 1rem;
  }
  .field {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 14rem);
    align-items: center;
    gap: 0.75rem;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .note {
    margin: 0;
    color: var(--muted);
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
