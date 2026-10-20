<script lang="ts">
  import { aboutLines } from "../about";
  import { appInfo, type AppInfo } from "../backend";
  import Panel from "./Panel.svelte";

  let { onClose }: { onClose: () => void } = $props();

  let info: AppInfo | null = $state(null);

  const lines = $derived(
    aboutLines(info ?? { appVersion: "(unknown)", protocolVersion: "(unknown)" }),
  );

  $effect(() => {
    appInfo()
      .then((i) => (info = i))
      .catch(() => (info = null));
  });
</script>

<Panel id="about" title="About" closeLabel="Close About" placement="top" {onClose}>
  {#each lines as line (line)}
    <p>{line}</p>
  {/each}
</Panel>

<style>
  p {
    margin: 0.5rem 0 0;
  }
</style>
