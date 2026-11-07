<script lang="ts">
  import { onMount } from "svelte";
  import { backendAvailable, cancelWait, listDevices, toUiError, watchDevices } from "../backend";
  import { deviceOptionLabel } from "../format";
  import type { Session } from "../session.svelte";

  let { session, hideSerial }: { session: Session; hideSerial: boolean } = $props();

  /** The selected entry's full label, as a tooltip in case the select is cut short. */
  const selectedLabel = $derived.by(() => {
    const device = session.devices.find((d) => d.id === session.selectedId);
    return device ? deviceOptionLabel(device, hideSerial, session.devices) : undefined;
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
    disabled={session.active || session.flashing || session.devices.length === 0}
    value={session.selectedId ?? ""}
    onchange={(e) => {
      if (session.select(e.currentTarget.value)) {
        cancelWait().catch((err: unknown) => session.reportError(toUiError(err)));
      }
    }}
  >
    <!-- Keyed by mode too: a meter that turns to or from its bootloader under the same
         entry gets a new option, which WebKit needs to repaint the closed select. -->
    {#each session.devices as device (`${device.id}|${device.mode}`)}
      <option value={device.id}>{deviceOptionLabel(device, hideSerial, session.devices)}</option>
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
    min-width: 7rem;
  }
  /* As wide as its longest entry where the bar has room; when the top bar runs out of
     room it may shrink, cut with an ellipsis (the full label stays in the tooltip). */
  select {
    width: auto;
    min-width: 7rem;
    max-width: 22rem;
    flex: 0 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
