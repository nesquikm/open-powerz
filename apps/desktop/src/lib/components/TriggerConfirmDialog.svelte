<script lang="ts">
  /**
   * The trigger view's confirmation for a request above 5 V (FR DGSQ6S): a modal
   * dialog that names the voltage and the output, opens with Cancel focused (the safer
   * choice), keeps Tab inside, and treats Escape as Cancel. A request the backend calls
   * rememberable also gets the "don't ask again" box (FR AXHK2N).
   */
  let {
    voltageMv,
    text,
    rememberable = false,
    remember = $bindable(false),
    onCancel,
    onConfirm,
  }: {
    /** The voltage the request asks for, mV (0 when not known). */
    voltageMv: number;
    /** What the dialog says. */
    text: string;
    /** The dialog offers "don't ask again". */
    rememberable?: boolean;
    /** The box is checked. */
    remember?: boolean;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

  const title = $derived(
    voltageMv > 0
      ? `Request ${Number((voltageMv / 1000).toFixed(2))} V?`
      : "Request more than 5 V?",
  );

  let box = $state<HTMLElement>();

  /** Focuses `el` on mount and gives the focus back to what had it when the dialog closes. */
  function focusOnMount(el: HTMLElement): { destroy: () => void } {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    el.focus();
    return {
      destroy() {
        if (previous?.isConnected) previous.focus();
      },
    };
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    } else if (e.key === "Tab") {
      // Keep focus inside the dialog.
      const controls = Array.from(
        (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("input, button"),
      );
      const first = controls[0];
      const last = controls[controls.length - 1];
      // A click on the dialog's text leaves the focus on the dialog itself.
      const atStart =
        document.activeElement === first || document.activeElement === e.currentTarget;
      if (e.shiftKey && atStart) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first?.focus();
      }
    }
  }
</script>

<!-- Escape cancels and Tab comes back, even when focus has left the dialog (a click on the backdrop). -->
<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    } else if (e.key === "Tab" && !box?.contains(document.activeElement)) {
      e.preventDefault();
      box?.querySelector("button")?.focus();
    }
  }}
/>

<div class="backdrop">
  <div
    bind:this={box}
    class="confirm panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="trigger-confirm-title"
    aria-describedby="trigger-confirm-text"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="trigger-confirm-title">{title}</h2>
    <p id="trigger-confirm-text">{text}</p>
    {#if rememberable}
      <label class="remember">
        <input type="checkbox" bind:checked={remember} />
        Don't ask again until I go back to 5 V or disconnect
      </label>
    {/if}
    <div class="actions">
      <button type="button" use:focusOnMount onclick={onCancel}>Cancel</button>
      <button type="button" class="primary" onclick={onConfirm}>Confirm</button>
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
  .remember {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin: 0 0 1rem;
    font-size: 0.875rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
