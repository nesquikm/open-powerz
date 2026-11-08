<script lang="ts">
  import { onMount } from "svelte";
  import {
    backendAvailable,
    cancelWait,
    features,
    listDevices,
    toUiError,
    watchDevices,
  } from "../backend";
  import { chooseDeviceMenu, deviceMenuActions } from "../deviceMenu";
  import { deviceOptionLabel } from "../format";
  import { WEB_FEATURES } from "../web/features";
  import type { Session } from "../session.svelte";

  let { session, hideSerial }: { session: Session; hideSerial: boolean } = $props();

  /** The WebUSB claim failed and the browser has WebHID: offer its picker
   * (AC-QFTCWZ.3). */
  const offerHid = $derived(
    features === WEB_FEATURES &&
      session.error?.kind === "device_busy" &&
      typeof navigator !== "undefined" &&
      "hid" in navigator,
  );

  /** The web build's action items, after the devices (FR AYR4W6). */
  const menuActions = $derived(
    features === WEB_FEATURES ? deviceMenuActions(session, offerHid) : [],
  );

  /** Only the demo meter is listed: a hint points to the menu's connect item and runs
   * the same action (AC-AYR4W6.4). Undefined when no hint is shown. */
  const hintAction = $derived(
    session.devices.every((d) => d.demo_scenario !== null) ? menuActions[0] : undefined,
  );

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
      if (chooseDeviceMenu(session, e.currentTarget, menuActions)) {
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
    {#each menuActions as action (action.value)}
      <option value={action.value}>{action.label}</option>
    {/each}
  </select>
  {#if hintAction}
    <button
      type="button"
      class="connect-hint"
      title="Opens the browser's meter picker, like “{hintAction.label}” in the device menu"
      disabled={session.flashing}
      onclick={() => void hintAction.run()}>Pick “{hintAction.label}”</button
    >
  {/if}
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
  /* A small link-style button next to the menu, not a full toolbar button. */
  .connect-hint {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent, inherit);
    font-size: 0.8125rem;
    text-decoration: underline;
    white-space: nowrap;
    cursor: pointer;
  }
  .connect-hint:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
