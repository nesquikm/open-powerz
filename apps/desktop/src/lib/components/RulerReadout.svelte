<script lang="ts">
  import type { RulerReadoutItem } from "../ruler";
  import Term from "./Term.svelte";

  /** The ruler's values between its cursors (FR AXHK2T) and "Reset ruler". The values
   * change on every step, so a screen reader hears them from a separate polite region
   * once they settle (AC-AXHK2T.3). */
  let {
    items,
    announcement,
    onReset,
  }: {
    items: RulerReadoutItem[];
    /** What the polite live region says. */
    announcement: string;
    /** Puts both cursors back in the view. */
    onReset: () => void;
  } = $props();
</script>

<div class="ruler-bar">
  <div role="group" aria-label="Ruler readout">
    <dl class="ruler-readout">
      {#each items as item (item.name)}
        <div>
          <dt title={item.name}>
            {#if item.label.startsWith("Δ")}<Term key="term.delta">{item.label}</Term
              >{:else}{item.label}{/if}
          </dt>
          <dd aria-label={item.name}>{item.text}</dd>
        </div>
      {/each}
    </dl>
  </div>
  <button type="button" title="Puts T1 and T2 back in the range on screen" onclick={onReset}
    >Reset ruler</button
  >
  <p class="visually-hidden" role="status" aria-live="polite" aria-label="Ruler announcement">
    {announcement}
  </p>
</div>

<style>
  .ruler-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 1rem;
    min-width: 0;
  }
  .ruler-readout {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 1rem;
    margin: 0;
    font-size: 0.85em;
    font-variant-numeric: tabular-nums;
  }
  .ruler-readout div {
    display: flex;
    gap: 0.375rem;
  }
  dt {
    color: var(--muted, GrayText);
  }
  dd {
    margin: 0;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    border: 0;
  }
</style>
