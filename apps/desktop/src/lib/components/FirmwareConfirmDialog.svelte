<script lang="ts">
  /**
   * The firmware update's typed confirmation (FR V9SK55, AC .3): a modal dialog that
   * names the target version, warns to keep the meter connected and the laptop on power,
   * opens with focus in the version field, enables Start update only for the exact
   * version, keeps Tab inside, and treats Escape as Cancel.
   */
  import { UPDATE_WARNING } from "../firmwareUpdate.svelte";

  let {
    version,
    confirmation = $bindable(),
    confirmed,
    onCancel,
    onConfirm,
  }: {
    /** The version the file would install. */
    version: string;
    /** What the user typed. */
    confirmation: string;
    /** The typed text is the target version. */
    confirmed: boolean;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();

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
        (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(
          "input, button:not([disabled])",
        ),
      );
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first?.focus();
      }
    }
  }

  function onSubmit(e: SubmitEvent): void {
    e.preventDefault();
    if (confirmed) onConfirm();
  }
</script>

<!-- Escape cancels even when focus has left the dialog (a click on the backdrop). -->
<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  }}
/>

<div class="backdrop">
  <div
    class="confirm panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="firmware-confirm-title"
    aria-describedby="firmware-confirm-text"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="firmware-confirm-title">Confirm firmware update</h2>
    <p id="firmware-confirm-text">
      This installs firmware {version} on the meter. {UPDATE_WARNING}
    </p>
    <form onsubmit={onSubmit}>
      <label for="firmware-confirm-version"
        >Type the version <strong>{version}</strong> to confirm</label
      >
      <input
        id="firmware-confirm-version"
        type="text"
        autocomplete="off"
        spellcheck="false"
        bind:value={confirmation}
        use:focusOnMount
      />
      <div class="actions">
        <button type="button" onclick={onCancel}>Cancel</button>
        <button type="submit" class="primary" disabled={!confirmed}>Start update</button>
      </div>
    </form>
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
    max-width: 26rem;
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
  label {
    display: block;
    margin-bottom: 0.25rem;
    font-size: 0.875rem;
  }
  input {
    width: 100%;
    box-sizing: border-box;
    margin-bottom: 1rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
