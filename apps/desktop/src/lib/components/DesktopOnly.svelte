<script lang="ts">
  /**
   * A control the build may not offer (FR QFTCX0, AC .6): with `available` it renders
   * `children` untouched; otherwise a disabled button named `label`, the text "Desktop
   * app only" and a link to the releases page, and the real control is not rendered.
   */
  import type { Snippet } from "svelte";
  import { DESKTOP_ONLY_TEXT, RELEASES_URL } from "../web/features";

  let { available, label, children }: { available: boolean; label: string; children: Snippet } =
    $props();
</script>

{#if available}
  {@render children()}
{:else}
  <span class="desktop-only">
    <button type="button" disabled>{label}</button>
    <span class="note"
      >{DESKTOP_ONLY_TEXT} ·
      <a href={RELEASES_URL} target="_blank" rel="noopener noreferrer">Download</a></span
    >
  </span>
{/if}

<style>
  .desktop-only {
    display: inline-flex;
    align-items: center;
    gap: 0.5em;
    flex-wrap: wrap;
  }
  .note {
    font-size: 0.85em;
    opacity: 0.8;
  }
</style>
