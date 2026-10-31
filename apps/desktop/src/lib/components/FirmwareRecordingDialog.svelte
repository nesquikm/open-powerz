<script lang="ts">
  /**
   * Asked before a firmware update ends a running recording (FR 4PJRVJ, AC .2): a modal
   * dialog that opens with Keep recording focused, keeps Tab inside, and treats Escape
   * as Keep recording.
   */
  let {
    onKeep,
    onEnd,
  }: {
    onKeep: () => void;
    onEnd: () => void;
  } = $props();

  function focusOnMount(el: HTMLElement): void {
    el.focus();
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key !== "Tab") return;
    // Keep focus inside the dialog.
    const buttons = Array.from(
      (e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>("button"),
    );
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  }
</script>

<!-- Escape keeps recording even when focus has left the dialog (a click on the backdrop). -->
<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onKeep();
    }
  }}
/>

<div class="backdrop">
  <div
    class="confirm panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="firmware-recording-title"
    aria-describedby="firmware-recording-text"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="firmware-recording-title">End the running recording?</h2>
    <p id="firmware-recording-text">
      This meter is recording, and the update disconnects it. Ending the recording saves the rows
      written so far.
    </p>
    <div class="actions">
      <button type="button" use:focusOnMount onclick={onKeep}>Keep recording</button>
      <button type="button" class="primary" onclick={onEnd}>End recording and update</button>
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
  .confirm {
    max-width: 24rem;
    padding: 1rem;
  }
  .confirm h2 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }
  .confirm p {
    margin: 0 0 1rem;
    font-size: 0.875rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
