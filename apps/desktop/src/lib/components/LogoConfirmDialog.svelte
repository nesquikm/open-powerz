<script lang="ts">
  /**
   * The Startup logo view's write confirmation (FR MEHQC0, AC .2): an alert dialog that
   * asks for the action's word to be typed; its button stays disabled until it matches
   * exactly. The same style as the record delete dialog: keeps Tab inside and closes on
   * Escape.
   */
  import { confirmWord, type LogoAction } from "../logo.svelte";

  let {
    action,
    durationText,
    onCancel,
    onConfirm,
  }: {
    /** The write to confirm. */
    action: LogoAction;
    /** What the write does to the startup duration (AC-QG6RNY.1). */
    durationText: string;
    onCancel: () => void;
    /** Called with the typed confirmation once it matches. */
    onConfirm: (typed: string) => void;
  } = $props();

  let typed = $state("");
  const want = $derived(confirmWord(action));
  const matches = $derived(typed === want);

  function focusOnMount(el: HTMLElement): void {
    el.focus();
  }

  function submit(e: SubmitEvent): void {
    e.preventDefault();
    if (matches) onConfirm(typed);
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    } else if (e.key === "Tab") {
      // Keep focus inside the dialog.
      const items = Array.from(
        (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(
          "input, button:not([disabled])",
        ),
      );
      const first = items[0];
      const last = items[items.length - 1];
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
    aria-labelledby="logo-confirm-title"
    aria-describedby="logo-confirm-text"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="logo-confirm-title">
      {action === "upload" ? "Upload this startup logo?" : "Remove the startup logo?"}
    </h2>
    <p id="logo-confirm-text">
      {action === "upload"
        ? "The logo is written to the meter's flash and read back to check it."
        : "The logo area on the meter's flash is cleared and read back to check it."}
      The write cannot be stopped once it starts.
    </p>
    <p data-testid="logo-confirm-duration">{durationText}</p>
    <form onsubmit={submit}>
      <label>
        <span>Type <strong>{want}</strong> to confirm</span>
        <input
          type="text"
          bind:value={typed}
          autocomplete="off"
          spellcheck="false"
          use:focusOnMount
        />
      </label>
      <div class="actions">
        <button type="button" onclick={onCancel}>Cancel</button>
        <button type="submit" class="primary danger" disabled={!matches}>
          {action === "upload" ? "Upload" : "Remove"}
        </button>
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
    overflow-wrap: anywhere;
  }
  .confirm p {
    margin: 0 0 1rem;
    font-size: 0.875rem;
  }
  label {
    display: grid;
    gap: 0.35rem;
    margin-bottom: 1rem;
    font-size: 0.875rem;
  }
  strong {
    font-family: ui-monospace, monospace;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
