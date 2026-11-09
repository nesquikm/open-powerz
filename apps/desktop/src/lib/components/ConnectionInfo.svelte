<script lang="ts">
  import GlossaryPopover from "./GlossaryPopover.svelte";
  import { glossaryTrigger, infoIconsShown } from "../infoPopover.svelte";
  import { CONNECTION_INFO_LINES, currentConnectionLine } from "../web/connectionInfo";
  import { RELEASES_URL } from "../web/features";
  import type { Session } from "../session.svelte";

  let { session }: { session: Session } = $props();

  /**
   * The (i) button right after the web build's device menu (FR 60QB2S): it opens a
   * popover about the connections, styled like the section (i) buttons.
   */
  const uid = $props.id();
  const trigger = glossaryTrigger(() => "section.connection", uid);
  let button = $state<HTMLButtonElement>();
  // The Show (i) icons switch (AC-60QB2S.1).
  const showInfoIcons = infoIconsShown();
  const shown = $derived(showInfoIcons());
</script>

{#if shown}<span class="info">
    <button
      bind:this={button}
      type="button"
      class="info-button"
      data-glossary-key="section.connection"
      aria-label="About {trigger.entry.term.toLowerCase()}"
      aria-expanded={trigger.open}
      aria-controls={trigger.popoverId}
      title={trigger.entry.short}
      onclick={trigger.toggle}>i</button
    >
    <GlossaryPopover
      key="section.connection"
      id={trigger.popoverId}
      open={trigger.open}
      anchor={button}
    >
      {#each CONNECTION_INFO_LINES as line (line)}
        <span class="line">{line}</span>
      {/each}
      <span class="line">{currentConnectionLine(session)}</span>
      <a class="line" href={RELEASES_URL} target="_blank" rel="noreferrer">Get the desktop app</a>
    </GlossaryPopover>
  </span>{/if}

<style>
  .line {
    display: block;
    margin-top: 0.375rem;
  }
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
