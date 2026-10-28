<script lang="ts">
  /**
   * Add a meter (FR 1HHT03): picks one more device to connect beside the meters already
   * connected, which are not offered again. Opens with the picker focused, keeps Tab
   * inside, and closes on Escape.
   */
  import { untrack } from "svelte";
  import type { DeviceListing } from "../backend";
  import { deviceOptionLabel } from "../format";

  let {
    devices,
    onCancel,
    onConnect,
  }: {
    /** The devices that can be added. */
    devices: readonly DeviceListing[];
    onCancel: () => void;
    onConnect: (id: string) => void;
  } = $props();

  /** The device to add; the first one offered when the dialog opens. */
  let picked = $state(untrack(() => devices[0]?.id ?? ""));

  function focusOnMount(el: HTMLElement): void {
    el.focus();
  }

  function onKey(e: KeyboardEvent): void {
    // No key reaches the window shortcuts while the dialog is open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    } else if (e.key === "Tab") {
      // Keep focus inside the dialog.
      const controls = Array.from(
        (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("select, button"),
      ).filter((el) => !(el as HTMLButtonElement).disabled);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first?.focus();
      }
    }
  }
</script>

<div class="backdrop">
  <div
    class="add panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="add-meter-title"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="add-meter-title">Add a meter</h2>
    <select
      aria-label="Device"
      bind:value={picked}
      disabled={devices.length === 0}
      use:focusOnMount
    >
      {#each devices as device (device.id)}
        <option value={device.id}>{deviceOptionLabel(device)}</option>
      {:else}
        <option value="">No other device</option>
      {/each}
    </select>
    <div class="actions">
      <button type="button" onclick={onCancel}>Cancel</button>
      <button
        type="button"
        class="primary"
        disabled={picked === ""}
        onclick={() => onConnect(picked)}>Connect</button
      >
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
  }
  .add {
    display: grid;
    gap: 0.75rem;
    width: min(24rem, calc(100vw - 2rem));
    padding: 1rem;
  }
  .add h2 {
    margin: 0;
    font-size: 1rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
