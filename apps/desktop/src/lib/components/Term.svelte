<script lang="ts">
  import type { Snippet } from "svelte";
  import { type GlossaryKey } from "../glossary";
  import GlossaryPopover from "./GlossaryPopover.svelte";
  import { glossaryTrigger } from "../infoPopover.svelte";

  /**
   * An abbreviation drawn with a dotted underline (FR CW66T1): pressing it opens the same
   * popover as an (i) button. The visible text is the children, so its label reads as before.
   */
  let { key, children }: { key: GlossaryKey; children: Snippet } = $props();

  const uid = $props.id();
  const trigger = glossaryTrigger(() => key, uid);
  let button = $state<HTMLButtonElement>();
</script>

<button
  bind:this={button}
  type="button"
  class="term"
  data-glossary-key={key}
  aria-expanded={trigger.open}
  aria-controls={trigger.popoverId}
  title={trigger.entry.short}
  onclick={trigger.toggle}>{@render children()}</button
><GlossaryPopover {key} id={trigger.popoverId} open={trigger.open} anchor={button} />

<style>
  .term {
    display: inline;
    padding: 0;
    margin: 0;
    border: 0;
    border-radius: 0;
    background: none;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    text-decoration-line: underline;
    text-decoration-style: dotted;
    text-underline-offset: 0.2em;
    cursor: help;
  }
  .term:hover:not(:disabled) {
    background: none;
    border: 0;
  }
</style>
