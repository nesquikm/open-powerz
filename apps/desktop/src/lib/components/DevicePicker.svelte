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

  /** Every menu entry's label, in menu order, for the width sizer. */
  const entryLabels = $derived([
    ...(session.devices.length > 0
      ? session.devices.map((d) => deviceOptionLabel(d, hideSerial, session.devices))
      : ["No device"]),
    ...menuActions.map((a) => a.label),
  ]);

  /** The sizer is drawn after mount only. The app itself never renders on a server, but
   * the component tests render it with `svelte/server` and expect the menu's labels
   * nowhere outside the select (e.g. no "Connect a meter…" beside it). */
  let mounted = $state(false);

  onMount(() => {
    mounted = true;
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
  <!-- The select and a hidden sizer share one grid cell: the sizer holds every entry's
       label with the select's padding plus room for the chevron, so the picker is as
       wide as its longest entry needs (FR 350H0S), yet may still shrink under fit-shrink. -->
  <span class="picker-box">
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
    {#if mounted}
      <span class="sizer" aria-hidden="true">
        {#each entryLabels as label, i (i)}
          <span>{label}</span>
        {/each}
      </span>
    {/if}
  </span>
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
  .picker-box {
    display: inline-grid;
    grid-template-columns: minmax(0, max-content);
    align-items: center;
    min-width: 7rem;
    max-width: 22rem;
    flex: 0 1 auto;
  }
  select,
  .sizer {
    grid-area: 1 / 1;
  }
  select {
    width: 100%;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Invisible and zero-height; only its width counts. Its box matches the select's
     (font size, padding, border) plus 12 px for the chevron. */
  .sizer {
    display: flex;
    flex-direction: column;
    height: 0;
    overflow: hidden;
    visibility: hidden;
    font-size: 0.875rem;
    white-space: nowrap;
    padding: 0 calc(1.75rem + 12px) 0 0.7rem;
    border: 1px solid transparent;
    border-block-width: 0;
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
