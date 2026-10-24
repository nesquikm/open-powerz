<script lang="ts">
  import { aboutLines } from "../about";
  import { appInfo, type AppInfo, type ConnectedInfo } from "../backend";
  import { deviceDetailsRows } from "../deviceDetails";
  import Panel from "./Panel.svelte";

  let {
    onClose,
    opener,
    details = null,
  }: {
    onClose: () => void;
    /** The connected meter's details (FR V9SK52, AC .5), null while none is connected. */
    details?: ConnectedInfo | null;
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
  {#if details !== null}
    <section aria-label="Device details">
      <h3>Device details</h3>
      <dl>
        {#each deviceDetailsRows(details) as row (row.label)}
          <dt>{row.label}</dt>
          <dd>{row.value}</dd>
        {/each}
      </dl>
    </section>
  {/if}
</Panel>

<style>
  p {
    margin: 0.5rem 0 0;
  }
  h3 {
    margin: 1rem 0 0.25rem;
    font-size: 1em;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 1rem;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
