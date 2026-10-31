<script lang="ts">
  /**
   * An inspected firmware file's metadata and English release notes (FR V9SK53, AC .3).
   * Display only: choosing the file and updating the meter live elsewhere.
   */
  import type { FirmwareFileInfo } from "../backend";
  import { firmwareFileRows, firmwareNotes, notesNumbered } from "../firmwareFile";

  let {
    info,
  }: {
    /** The file's metadata, from `firmwareInspect`. */
    info: FirmwareFileInfo;
  } = $props();

  const rows = $derived(firmwareFileRows(info));
  const notes = $derived(firmwareNotes(info));
</script>

<section class="firmware-file" aria-label="Firmware file">
  <dl>
    {#each rows as row (row.label)}
      <dt>{row.label}</dt>
      <dd>{row.value}</dd>
    {/each}
  </dl>
  <h3>Release notes</h3>
  {#if notes.length > 0}
    <ul class:numbered={notesNumbered(notes)}>
      {#each notes as line, i (i)}<li>{line}</li>{/each}
    </ul>
  {:else}
    <p class="none">This file has no English release notes.</p>
  {/if}
</section>

<style>
  .firmware-file {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.25rem 1rem;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  h3 {
    margin: 1rem 0 0.25rem;
    font-size: 1rem;
  }
  ul {
    margin: 0;
    padding-left: 1.25rem;
  }
  ul.numbered {
    list-style: none;
    padding-left: 0;
  }
  .none {
    margin: 0;
  }
</style>
