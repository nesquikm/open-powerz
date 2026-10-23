<script lang="ts">
  import type { TriggerOffer } from "../backend";
  import type { TriggerPanel } from "../trigger.svelte";
  import TriggerConfirmDialog from "./TriggerConfirmDialog.svelte";

  /**
   * The trigger view (FR DGSQ6S): Detect, the detection result, and one section per
   * protocol, with the protocols the charger does not support marked as such.
   */
  let {
    panel,
    connected,
  }: {
    panel: TriggerPanel;
    /** A meter (or the demo device) is connected and streaming. */
    connected: boolean;
  } = $props();

  let busy = $state(false);

  const reason = $derived(connected ? null : "Connect a meter to detect the charger's protocols.");

  async function detect(): Promise<void> {
    busy = true;
    try {
      await panel.detect();
    } finally {
      busy = false;
    }
  }

  /** `9 V` or `3.3 V`, from millivolts. */
  function volts(mv: number): string {
    return `${Number((mv / 1000).toFixed(2))} V`;
  }

  /** `3 A` from milliamps, or null. */
  function amps(ma: number | null): string | null {
    return ma === null ? null : `${Number((ma / 1000).toFixed(2))} A`;
  }

  function offerLabel(o: TriggerOffer): string {
    const current = amps(o.max_current_ma);
    return current === null ? volts(o.max_voltage_mv) : `${volts(o.max_voltage_mv)} ${current}`;
  }

  /** A PPS / AVS offer, shown as a slider. */
  function isRange(o: TriggerOffer): boolean {
    return o.kind === "pps" || o.kind === "spr-avs" || o.kind === "epr-avs";
  }

  function rangeName(o: TriggerOffer): string {
    return o.kind === "pps" ? "PPS" : o.kind === "epr-avs" ? "EPR AVS" : "SPR AVS";
  }

  /** PPS moves in 20 mV steps, AVS in 100 mV steps. */
  function rangeStep(o: TriggerOffer): number {
    return o.kind === "pps" ? 20 : 100;
  }

  /** The current a range request asks for, mA: the offer's, or its power at `mv`. */
  function rangeCurrent(o: TriggerOffer, mv: number): number | null {
    if (o.max_current_ma !== null) return o.max_current_ma;
    if (o.max_power_mw !== null && mv > 0) return Math.floor((o.max_power_mw * 1000) / mv);
    return null;
  }

  /** The slider value of each range offer, mV, by position. */
  let rangeMv = $state<Record<number, number>>({});

  /** The slider's value, kept inside the offer's range (a new detection or
   *  charger can shrink it). */
  function sliderMv(o: TriggerOffer): number {
    const mv = rangeMv[o.position] ?? o.min_voltage_mv;
    return Math.min(o.max_voltage_mv, Math.max(o.min_voltage_mv, mv));
  }

  const SUPPORT_TEXT: Record<string, string> = {
    supported: "Supported",
    "not-supported": "Not supported",
    unknown: "Unknown",
  };
</script>

<div class="trigger">
  <div class="toolbar" role="toolbar" aria-label="Trigger">
    <button
      type="button"
      class="primary"
      disabled={!connected || busy}
      aria-describedby={reason ? "trigger-reason" : undefined}
      onclick={() => void detect()}>Detect</button
    >
    {#if panel.showBackTo5v}
      <button type="button" onclick={() => void panel.backTo5v()}>Back to 5 V</button>
    {/if}
    {#if reason}
      <p id="trigger-reason" class="reason">{reason}</p>
    {:else if busy}
      <p class="reason" aria-live="polite">Detecting…</p>
    {/if}
  </div>

  <p class="reason">
    Keep the meter's output empty: detection lets the meter probe protocols, which can briefly raise
    VBUS above 5 V on some chargers.
  </p>

  {#if panel.error}
    <p class="error" role="alert">{panel.error.message}</p>
  {/if}

  <section class="card" aria-labelledby="trigger-detection-title">
    <h2 id="trigger-detection-title">Detection</h2>
    {#if panel.detection}
      <dl class="facts">
        <dt>BC1.2</dt>
        <dd>{SUPPORT_TEXT[panel.detection.bc12] ?? panel.detection.bc12}</dd>
        <dt>Apple</dt>
        <dd>{SUPPORT_TEXT[panel.detection.apple] ?? panel.detection.apple}</dd>
      </dl>
      {#if panel.detection.entries.length > 0}
        <ul class="entries">
          {#each panel.detection.entries as entry, i (i)}
            <li>
              <span class="name">{entry.name}</span>
              {SUPPORT_TEXT[entry.support] ?? entry.support}
              {#if entry.vbus_mv !== null}<span class="muted">{volts(entry.vbus_mv)}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if !panel.detection.complete}
        <p class="muted">The automatic list ended early (stalled or cut off).</p>
      {/if}
    {:else}
      <p class="muted">Not detected yet.</p>
    {/if}
  </section>

  <div class="sections">
    {#each panel.sections as s (s.protocol)}
      <section class="card" aria-labelledby="trigger-{s.protocol}-title">
        <h2 id="trigger-{s.protocol}-title">{s.title}</h2>
        {#if s.state === "pending"}
          <p class="muted">Not listed yet.</p>
        {:else if s.state === "not-supported"}
          <p class="muted">Not supported by this charger.</p>
        {:else if s.section}
          {#if s.section.offers.length > 0}
            <div class="offers">
              {#each s.section.offers.filter((o) => o.kind === "fixed") as o (o.position)}
                <button
                  type="button"
                  onclick={() =>
                    void panel.ask({
                      protocol: "pd",
                      position: o.position,
                      voltage_mv: null,
                      current_ma: null,
                    })}>{offerLabel(o)}</button
                >
              {/each}
            </div>
          {/if}
          {#each s.section.offers.filter(isRange) as o (o.position)}
            {@const mv = sliderMv(o)}
            <div class="range">
              <label for="trigger-range-{o.position}"
                >{rangeName(o)} #{o.position}, {volts(o.min_voltage_mv)}–{volts(
                  o.max_voltage_mv,
                )}</label
              >
              <div class="range-row">
                <input
                  id="trigger-range-{o.position}"
                  type="range"
                  min={o.min_voltage_mv}
                  max={o.max_voltage_mv}
                  step={rangeStep(o)}
                  value={mv}
                  aria-valuetext={volts(mv)}
                  oninput={(e) =>
                    (rangeMv[o.position] = Number((e.currentTarget as HTMLInputElement).value))}
                />
                <output for="trigger-range-{o.position}">{volts(mv)}</output>
                <button
                  type="button"
                  onclick={() =>
                    void panel.ask({
                      protocol: "pd",
                      position: o.position,
                      voltage_mv: mv,
                      current_ma: rangeCurrent(o, mv),
                    })}>Request {rangeName(o)} {volts(mv)}</button
                >
              </div>
            </div>
          {/each}
          {#if s.section.settings}
            <h3>PD emulation</h3>
            <dl class="facts">
              <dt>Type</dt>
              <dd>{s.section.settings.pd_type}</dd>
              <dt>E-marker</dt>
              <dd>{s.section.settings.emarker}</dd>
              <dt>Sink</dt>
              <dd>{s.section.settings.sink}</dd>
            </dl>
          {/if}
          {#if s.section.limits.length > 0}
            <ul class="entries">
              {#each s.section.limits as l, i (i)}
                <li>
                  {#if l.index !== null}<span class="name">#{l.index}</span>{/if}
                  {volts(l.min_voltage_mv)}–{volts(l.max_voltage_mv)}, up to {amps(
                    l.max_current_ma,
                  )}
                </li>
              {/each}
            </ul>
          {/if}
          <p class="muted">Supported.</p>
        {/if}
      </section>
    {/each}
  </div>

  <section class="card" aria-labelledby="trigger-result-title">
    <h2 id="trigger-result-title">Request result</h2>
    {#if panel.reply}
      <dl class="facts">
        <dt>Command</dt>
        <dd><code>{panel.reply.command}</code></dd>
        {#if panel.requestedMv !== null}
          <dt>Requested</dt>
          <dd>
            {volts(panel.requestedMv)}{#if panel.reply.requested_ma !== null}, {amps(
                panel.reply.requested_ma,
              )}{/if}
          </dd>
        {/if}
        {#if panel.reply.delivered_mv !== null}
          <dt>Delivered</dt>
          <dd>
            {volts(panel.reply.delivered_mv)}{#if panel.reply.delivered_ma !== null}, {amps(
                panel.reply.delivered_ma,
              )}{/if}
          </dd>
        {/if}
        {#if panel.reply.reply !== null}
          <dt>Reply</dt>
          <dd>{panel.reply.reply}</dd>
        {/if}
      </dl>
    {:else}
      <p class="muted">No request yet.</p>
    {/if}
    <p class="live">
      <span id="trigger-live-vbus-label">Live VBUS</span>
      <output aria-labelledby="trigger-live-vbus-label"
        >{panel.vbusV === null ? "—" : `${panel.vbusV.toFixed(2)} V`}</output
      >
    </p>
    {#if panel.deviation}
      <p class="warning" role="status">VBUS differs from the request by more than 5 %.</p>
    {/if}
  </section>

  {#if panel.pending}
    <TriggerConfirmDialog
      voltageMv={panel.pending.voltageMv}
      text={panel.pending.text}
      onCancel={() => panel.cancel()}
      onConfirm={() => void panel.confirm()}
    />
  {/if}
</div>

<style>
  .trigger {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1rem;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }
  .reason,
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .error {
    margin: 0;
  }
  .live {
    margin: 0;
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
  }
  .live output {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .warning {
    margin: 0;
    font-weight: 600;
  }
  .sections {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
    gap: 1rem;
  }
  .card {
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface);
    padding: 0.75rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  h3 {
    margin: 0;
    font-size: 0.875rem;
  }
  .range {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.875rem;
  }
  .range-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }
  .range-row input {
    flex: 1 1 8rem;
  }
  .facts {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 1rem;
    margin: 0;
  }
  .facts dd {
    margin: 0;
  }
  .entries {
    margin: 0;
    padding-left: 1.25rem;
    font-size: 0.875rem;
  }
  .name {
    font-weight: 600;
  }
  .offers {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
</style>
