<script lang="ts">
  import { aboutLines } from "../about";
  import { appInfo, type AppInfo, type ConnectedInfo } from "../backend";
  import { deviceDetailsRows } from "../deviceDetails";
  import Panel from "./Panel.svelte";

  let {
    onClose,
    opener,
    details = null,
    advancedFirmware = false,
    advancedFirmwareLocked = false,
    onAdvancedFirmware,
  }: {
    onClose: () => void;
    /** The Advanced firmware switch (FR V9SK55, AC .1): shows the Firmware tab. */
    advancedFirmware?: boolean;
    /** An update runs: the switch cannot be turned off. */
    advancedFirmwareLocked?: boolean;
    onAdvancedFirmware?: (on: boolean) => void;
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
        {#each deviceDetailsRows(details) as row, i (`${row.label}:${i}`)}
          <dt>{row.label}</dt>
          <dd>{row.value}</dd>
        {/each}
      </dl>
    </section>
  {/if}
  <section aria-label="Settings">
    <h3>Settings</h3>
    <button
      type="button"
      role="switch"
      class="switch"
      aria-checked={advancedFirmware}
      disabled={advancedFirmwareLocked && advancedFirmware}
      onclick={() => onAdvancedFirmware?.(!advancedFirmware)}
    >
      Advanced firmware
    </button>
  </section>
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
  .switch {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font: inherit;
    color: var(--text);
    background: none;
    border: none;
    padding: 0.25rem 0;
    cursor: pointer;
  }
  .switch::before {
    content: "";
    box-sizing: border-box;
    width: 2rem;
    height: 1.1rem;
    border: 1px solid var(--control-border);
    border-radius: 999px;
    background: radial-gradient(
      circle at 0.55rem 50%,
      var(--control-border) 0.35rem,
      transparent 0.37rem
    );
  }
  .switch[aria-checked="true"]::before {
    border-color: var(--accent);
    background: radial-gradient(
        circle at calc(100% - 0.55rem) 50%,
        var(--bg) 0.35rem,
        transparent 0.37rem
      )
      var(--accent);
  }
  .switch:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
