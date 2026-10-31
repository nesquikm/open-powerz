<script lang="ts">
  /**
   * The connected meter's firmware warnings (FR V9SK52, AC .4). Not one of the event
   * notices: it describes the meter, so it shows for as long as that meter is connected
   * and has no controls of its own. The live region stays mounted with nothing in it,
   * so a screen reader sees the text arrive (AC-4PJRVJ.6).
   */
  let {
    warnings,
  }: {
    /** One line per warning. */
    warnings: readonly string[];
  } = $props();
</script>

<div class="firmware-live" role="status" aria-live="polite">
  {#if warnings.length > 0}
    <div class="firmware-notice">
      <div class="text">
        <p class="message">Firmware warning</p>
        {#each warnings as text, i (i)}<p>{text}</p>{/each}
      </div>
    </div>
  {/if}
</div>

<style>
  /* Out of the stack's flow, not display: none: a hidden region announces nothing. */
  .firmware-live:empty {
    position: absolute;
  }
  .firmware-notice {
    margin: 0.75rem 1rem 0;
    padding: 0.5rem 0.875rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface);
    min-width: 0;
  }
  .text {
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
  }
  .message {
    font-weight: 600;
  }
</style>
