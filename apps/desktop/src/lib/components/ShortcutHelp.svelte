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
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 0.75rem;
    margin: 0.75rem 0 0;
  }
  dt,
  dd {
    margin: 0;
  }
  kbd {
    font-family: inherit;
    font-weight: 600;
  }
</style>
