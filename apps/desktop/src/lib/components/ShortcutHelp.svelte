<script lang="ts">
  import { SHORTCUTS } from "../shortcuts";
  import Panel from "./Panel.svelte";

  let { onClose }: { onClose: () => void } = $props();
</script>

<Panel
  id="shortcut-help"
  title="Keyboard shortcuts"
  closeLabel="Close shortcut list"
  placement="bottom"
  {onClose}
>
  <dl>
    {#each SHORTCUTS as s (s.keys)}
      <dt><kbd>{s.keys}</kbd></dt>
      <dd>{s.description}</dd>
    {/each}
  </dl>
</Panel>

<style>
  /* The shared panel caps at 24rem; this list needs room for one-line rows. */
  :global(#shortcut-help) {
    max-width: calc(100vw - 2rem);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 0.75rem;
    margin: 0.75rem 0 0;
  }
  dt,
  dd {
    margin: 0;
    white-space: nowrap;
  }
  /* Below the app's minimum width (a narrow browser view), wrap rather than spill. */
  @media (max-width: 600px) {
    dd {
      white-space: normal;
    }
  }
  kbd {
    font-family: inherit;
    font-weight: 600;
  }
</style>
