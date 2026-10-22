<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * A non-modal floating panel (shortcut list, About): focus moves in while it
   * is open and back on close, Escape or the Close button closes it.
   */
  let {
    id,
    title,
    closeLabel,
    placement,
    onClose,
    returnFocus,
    children,
  }: {
    /** Element id; the heading gets `<id>-title`. */
    id: string;
    title: string;
    closeLabel: string;
    /** Corner the panel floats in (right side, under the top bar or at the bottom). */
    placement: "top" | "bottom";
    onClose: () => void;
    /**
     * Where focus goes on close. Defaults to whatever had focus at open, which is
     * unreliable for a mouse opener (WebKit does not focus buttons on click).
     */
    returnFocus?: () => HTMLElement | null | undefined;
    children: Snippet;
  } = $props();

  let panel: HTMLElement | undefined = $state();

  $effect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const own = panel;
    own?.focus();
    return () => {
      // Give focus back only if it is still ours: the user may have moved on.
      const now = document.activeElement;
      if (now === null || now === document.body || own?.contains(now)) {
        (returnFocus?.() ?? previous)?.focus();
      }
    };
  });

  /** Escape closes the panel even after focus has left it (it is non-modal). */
  function onWindowKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div
  {id}
  class="panel"
  data-placement={placement}
  role="dialog"
  aria-modal="false"
  aria-labelledby={`${id}-title`}
  tabindex="-1"
  bind:this={panel}
>
  <header>
    <h2 id={`${id}-title`}>{title}</h2>
    <button type="button" onclick={onClose} aria-label={closeLabel}>Close</button>
  </header>
  {@render children()}
</div>

<style>
  .panel {
    position: fixed;
    right: 1rem;
    z-index: 10;
    max-width: min(24rem, calc(100vw - 2rem));
    padding: 0.75rem 1rem;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
  }
  .panel[data-placement="top"] {
    top: 4rem;
  }
  .panel[data-placement="bottom"] {
    bottom: 1rem;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
</style>
