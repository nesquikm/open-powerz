<script lang="ts">
  import { type GlossaryKey } from "../glossary";
  import GlossaryPopover from "./GlossaryPopover.svelte";
  import { glossaryTrigger, infoIconsShown } from "../infoPopover.svelte";

  /**
   * The (i) button beside a section's title (FR CW66T1): it opens a short popover
   * with the glossary entry of `key`.
   */
  let { key }: { key: GlossaryKey } = $props();

  const uid = $props.id();
  const trigger = glossaryTrigger(() => key, uid);
  let button = $state<HTMLButtonElement>();
  // The Show (i) icons switch (AC-CW66T1.8).
  const showInfoIcons = infoIconsShown();
  const shown = $derived(showInfoIcons());
</script>

{#if shown}<span class="info">
    <button
      bind:this={button}
      type="button"
      class="info-button"
      data-glossary-key={key}
      aria-label="About {trigger.entry.term}"
      aria-expanded={trigger.open}
      aria-controls={trigger.popoverId}
      title={trigger.entry.short}
      onclick={trigger.toggle}>i</button
    >
    <GlossaryPopover {key} id={trigger.popoverId} open={trigger.open} anchor={button} />
  </span>{/if}

<style>
  .info {
    display: inline-flex;
    align-items: center;
  }
  .info-button {
    width: 1.25rem;
    height: 1.25rem;
    padding: 0;
    border-radius: 50%;
    font-size: 0.75rem;
    font-weight: 600;
    font-style: italic;
    line-height: 1;
  }
</style>
