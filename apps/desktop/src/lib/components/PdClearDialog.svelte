<script lang="ts">
  /**
   * The PD view's in-app Clear confirmation (FR TGE83C): an alert dialog that opens with
   * Cancel focused, keeps Tab inside, and closes on Escape.
   */
  let {
    onCancel,
    onConfirm,
  }: {
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  function focusOnMount(el: HTMLElement): void {
    el.focus();
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    } else if (e.key === "Tab") {
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
  }
</script>

<div class="backdrop">
  <div
    class="confirm panel"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="pd-clear-title"
    aria-describedby="pd-clear-text"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="pd-clear-title">Clear the capture?</h2>
    <p id="pd-clear-text">
      Every captured PD packet is removed from the table. A running capture goes on.
    </p>
    <div class="actions">
      <button type="button" use:focusOnMount onclick={onCancel}>Cancel</button>
      <button type="button" class="primary" onclick={onConfirm}>Clear</button>
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
