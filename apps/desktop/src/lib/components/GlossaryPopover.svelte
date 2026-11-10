<script lang="ts" module>
  /** Closers of the popovers now open, newest last. */
  // Plain bookkeeping read only by the Escape listener, never rendered.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const openPopovers = new Set<() => void>();

  /**
   * Registered when this module loads, before any component mounts, so it is the first
   * window capture listener: an Escape that closes a popover is spent there and reaches
   * no dialog, panel or shortcut behind it.
   */
  if (typeof window !== "undefined") {
    window.addEventListener(
      "keydown",
      (e) => {
        if (e.key !== "Escape" || openPopovers.size === 0) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        for (const close of [...openPopovers]) close();
      },
      true,
    );
  }
</script>

<script lang="ts">
  import { GLOSSARY, type GlossaryKey } from "../glossary";
  import { infoPopover } from "../infoPopover.svelte";
  import { placePopover } from "../popoverPlacement";
  import type { Snippet } from "svelte";

  /**
   * The popover of one glossary entry (FR CW66T1), shared by InfoButton and Term. It is
   * moved to the end of <body>, so its text never joins the text of the label, heading or
   * value that holds its trigger, and it is placed beside `anchor` while open.
   */
  let {
    key,
    id,
    open,
    anchor,
    children,
  }: {
    key: GlossaryKey;
    id: string;
    /** Whether the shared store has this popover open. */
    open: boolean;
    /** The trigger the popover sits beside. */
    anchor: HTMLElement | undefined;
    /** Extra content after the entry's text, rendered only while the popover is open. */
    children?: Snippet;
  } = $props();

  const entry = $derived(GLOSSARY[key]);

  let node = $state<HTMLElement>();
  let left = $state(0);
  let top = $state(0);

  /** Moves the popover to the end of <body> while it is mounted. */
  function portal(el: HTMLElement): { destroy: () => void } {
    document.body.appendChild(el);
    return { destroy: () => el.remove() };
  }

  /** Beside the trigger, off it when there is room, inside the window (placePopover). */
  function place(): void {
    if (!anchor || !node) return;
    const a = anchor.getBoundingClientRect();
    const p = placePopover(
      { left: a.left, top: a.top, width: a.width, height: a.height },
      { width: node.offsetWidth, height: node.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
    );
    left = p.left;
    top = p.top;
  }

  $effect(() => {
    if (open) place();
  });

  /** Closes the popover and puts focus back on its trigger. */
  function close(): void {
    if (infoPopover.openKey === id) infoPopover.close();
    anchor?.focus();
  }

  $effect(() => {
    if (!open) return;
    openPopovers.add(close);
    return () => openPopovers.delete(close);
  });

  /** A click outside both the popover and its trigger closes it. Focus goes back to the
   * trigger unless the click put it on another control. */
  function onOutsideClick(e: MouseEvent): void {
    if (!open) return;
    const t = e.target;
    if (t instanceof Node && (node?.contains(t) || anchor?.contains(t))) return;
    if (infoPopover.openKey === id) infoPopover.close();
    const active = document.activeElement;
    if (active === null || active === document.body) anchor?.focus();
  }
</script>

<svelte:window onresize={() => open && place()} />
<svelte:document onclickcapture={onOutsideClick} />

<span
  bind:this={node}
  use:portal
  {id}
  class="popover"
  role="dialog"
  aria-label={entry.term}
  hidden={!open}
  style:left="{left}px"
  style:top="{top}px"
>
  <span class="short">{entry.short}</span>
  {#if entry.long}
    <span class="long">{entry.long}</span>
  {/if}
  {#if children && open}
    {@render children()}
  {/if}
</span>

<style>
  .popover {
    position: fixed;
    z-index: 1000;
    display: block;
    width: max-content;
    max-width: min(20rem, calc(100vw - 2rem));
    padding: 0.5rem 0.75rem;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    font-size: 0.875rem;
    font-weight: normal;
    font-style: normal;
    text-align: left;
    text-decoration: none;
    text-transform: none;
    letter-spacing: normal;
  }
  .popover[hidden] {
    display: none;
  }
  .short,
  .long {
    display: block;
  }
  .long {
    margin-top: 0.375rem;
  }
</style>
