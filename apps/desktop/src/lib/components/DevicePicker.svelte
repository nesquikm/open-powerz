<script lang="ts">
  import { onMount } from "svelte";
  import { backendAvailable, listDevices, toUiError, watchDevices } from "../backend";
  import { deviceOptionLabel } from "../format";
  import type { Session } from "../session.svelte";

  let { session }: { session: Session } = $props();

  /** The selected entry's full label, as a tooltip in case the select is cut short. */
  const selectedLabel = $derived.by(() => {
    const device = session.devices.find((d) => d.id === session.selectedId);
    return device ? deviceOptionLabel(device) : undefined;
  });

  onMount(() => {
    if (!backendAvailable()) return;
    // A listing failure is shown but does not touch the connection state.
    const fail = (err: unknown) => session.reportError(toUiError(err));
    // Hotplug: the backend pushes the full list on every change.
    let unsubscribe: (() => void) | null = null;
    let destroyed = false;
    watchDevices((devices) => session.setDevices(devices))
      .then((stop) => (destroyed ? stop() : (unsubscribe = stop)))
      .catch(fail);
    listDevices()
      .then((devices) => session.setDevices(devices))
      .catch(fail);
    return () => {
      destroyed = true;
      unsubscribe?.();
    };
  });
</script>

<span class="device-picker">
  <select
    aria-label="Device"
    title={selectedLabel}
    disabled={session.active || session.devices.length === 0}
    value={session.selectedId ?? ""}
    onchange={(e) => session.select(e.currentTarget.value)}
  >
    {#each session.devices as device (device.id)}
      <option value={device.id}>{deviceOptionLabel(device)}</option>
    {:else}
      <option value="">No device</option>
    {/each}
  </select>
</span>

<style>
  .device-picker {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    min-width: 0;
  }
  /* As wide as its longest entry where the bar has room, never so wide the bar wraps. */
  select {
    width: auto;
    min-width: 8rem;
    max-width: 18rem;
  }
  @media (max-width: 999.98px) {
    select {
      max-width: 15rem;
    }
  }
</style>
