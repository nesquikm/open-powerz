<script lang="ts">
  import { untrack } from "svelte";
  import {
    EXPORT_FORMATS,
    exportSizeError,
    type ExportFormat,
    type SaveImageChoice,
  } from "../chartExport";

  /**
   * "Save image" (FR AXHK2V): asks for the image's size and format, and whether to draw
   * on white. Closes on Escape.
   */
  let {
    initialWidth,
    initialHeight,
    onCancel,
    onSave,
  }: {
    initialWidth: number;
    initialHeight: number;
    onCancel: () => void;
    onSave: (choice: SaveImageChoice) => void;
  } = $props();

  const uid = $props.id();
  // Seeded once from the chart's size; the user edits from there.
  let width = $state(untrack(() => initialWidth));
  let height = $state(untrack(() => initialHeight));
  let format: ExportFormat = $state("png");
  let white = $state(false);

  const error = $derived(exportSizeError(width, height));

  function focusOnMount(el: HTMLElement): void {
    el.focus();
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<div class="backdrop">
  <div
    class="save-image panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="{uid}-title">Save image</h2>
    <div class="fields">
      <label for="{uid}-width">Width</label>
      <input
        id="{uid}-width"
        type="number"
        min="320"
        max="7680"
        step="1"
        bind:value={width}
        use:focusOnMount
      />
      <label for="{uid}-height">Height</label>
      <input id="{uid}-height" type="number" min="320" max="7680" step="1" bind:value={height} />
      <label for="{uid}-format">Format</label>
      <select id="{uid}-format" bind:value={format}>
        {#each EXPORT_FORMATS as f (f)}
          <option value={f}>{f.toUpperCase()}</option>
        {/each}
      </select>
      <label class="white"><input type="checkbox" bind:checked={white} /> White background</label>
    </div>
    {#if error}
      <p class="size-error" role="alert">{error}</p>
    {/if}
    <div class="actions">
      <button type="button" onclick={onCancel}>Cancel</button>
      <button
        type="button"
        class="primary"
        disabled={error !== null}
        onclick={() => onSave({ width, height, format, white })}>Save</button
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
  .save-image {
    max-width: 24rem;
    padding: 1rem;
  }
  .save-image h2 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }
  .fields {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.5rem;
    align-items: center;
  }
  .fields .white {
    grid-column: 1 / -1;
  }
  .size-error {
    color: var(--danger);
    font-size: 0.875rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1rem;
  }
</style>
