<script lang="ts">
  import { aboutLines } from "../about";
  import { appInfo, type AppInfo } from "../backend";
  import Panel from "./Panel.svelte";

  let {
    onClose,
    opener,
  }: {
    onClose: () => void;
    /** The control that opened About; focus returns there on close (or to ⋯ when it is gone). */
    opener?: HTMLElement | null;
  } = $props();

  /** The opener if it is still on screen, else the ⋯ button (e.g. the menu has closed). */
  function returnFocus(): HTMLElement | null {
    if (opener?.isConnected && opener.getClientRects().length > 0) return opener;
    // The window may have widened since: ⋯ is hidden and About is inline again.
    const more = document.getElementById("top-bar-more");
    if (more && more.getClientRects().length > 0) return more;
    return document.getElementById("top-bar-about") ?? more;
  }

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

<Panel id="about" title="About" closeLabel="Close About" placement="top" {onClose} {returnFocus}>
  {#each lines as line (line)}
    <p>{line}</p>
  {/each}
</Panel>

<style>
  p {
    margin: 0.5rem 0 0;
  }
</style>
