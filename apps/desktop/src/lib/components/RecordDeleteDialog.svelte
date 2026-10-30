<script lang="ts">
  /**
   * The Records view's delete confirmation (FR JKWT3V, AC .3): an alert dialog that
   * asks for the record name (or `all`) to be typed; Delete stays disabled until it
   * matches exactly. Keeps Tab inside and closes on Escape.
   */
  let {
    name,
    onCancel,
    onConfirm,
  }: {
    /** The record to delete, or null for every record. */
    name: string | null;
    onCancel: () => void;
    /** Called with the typed confirmation once it matches. */
    onConfirm: (typed: string) => void;
  } = $props();

  let typed = $state("");
  const want = $derived(name ?? "all");
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
    aria-labelledby="record-delete-title"
    aria-describedby="record-delete-text"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="record-delete-title">
      {name === null ? "Delete all records?" : `Delete record “${name}”?`}
    </h2>
    <p id="record-delete-text">
      {name === null
        ? "Every offline record is removed from the meter."
        : "The record is removed from the meter."}
      A CSV backup is written first; nothing is deleted when the backup fails.
    </p>
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
        <button type="submit" class="primary danger" disabled={!matches}>Delete</button>
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
