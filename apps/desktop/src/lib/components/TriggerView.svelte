<script lang="ts">
  import type { TriggerLimits, TriggerOffer, TriggerProtocol } from "../backend";
  import {
    LEGACY_FIXED_VOLTS,
    QC3_RANGE,
    clampToLimits,
    detectionChecks,
    isFixedVoltProtocol,
    isLimitsProtocol,
    limitsRequest,
    type TriggerPanel,
  } from "../trigger.svelte";
  import type { PdConsole } from "../console.svelte";
  import PdConsoleView from "./PdConsoleView.svelte";
  import TriggerConfirmDialog from "./TriggerConfirmDialog.svelte";

  /**
   * The trigger view (FR DGSQ6S): Detect, the detection result, and one section per
   * protocol, with the protocols the charger does not support marked as such.
   */
  let {
    panel,
    connected,
    pdConsole,
  }: {
    panel: TriggerPanel;
    /** A meter (or the demo device) is connected and streaming. */
    connected: boolean;
    /** The PD message builder and raw console (FR DGSQ6T); shown while a session is open. */
    pdConsole?: PdConsole;
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

  /** The QC 3.0 slider's value, mV. */
  let qc3Mv = $state<number>(QC3_RANGE.minMv);

  /** The voltage and current slider values of each limits block, by key. */
  let limitsMv = $state<Record<string, number>>({});
  let limitsMa = $state<Record<string, number>>({});

  function limitsKey(p: TriggerProtocol, l: TriggerLimits, i: number): string {
    // The position is part of the key, so two blocks with one number never clash.
    return `${p}-${i}-${l.index ?? "n"}`;
  }

  // A new detection starts every control afresh: stale values from another
  // charger or an earlier list are never reused.
  $effect(() => {
    void panel.detection;
    rangeMv = {};
    qc3Mv = QC3_RANGE.minMv;
    limitsMv = {};
    limitsMa = {};
  });

  /** Request controls work only in an open session, not while a confirmation is
   *  waiting, and not once the meter fell out of step (until the session is reopened). */
  const requestsOff = $derived(
    !panel.sessionOpen || panel.pending !== null || panel.requestsDisabled,
  );

  /** `SCP` or `UFCS #2`: the name of a limits block. */
  function limitsName(title: string, l: TriggerLimits): string {
    return l.index === null ? title : `${title} #${l.index}`;
  }

  /** The block's slider values, clamped to its limits (and power). */
  function limitsValue(key: string, l: TriggerLimits): { voltage_mv: number; current_ma: number } {
    return clampToLimits(l, limitsMv[key] ?? l.min_voltage_mv, limitsMa[key] ?? l.max_current_ma);
  }

  /** One slider row: its label, the range input, the value it reads out and,
   *  optionally, the button that requests it. */
  interface SliderProps {
    id: string;
    label: string;
    min: number;
    max: number;
    step: number;
    value: number;
    /** The read-out (and `aria-valuetext`) of the value. */
    text: string | null;
    onvalue: (value: number) => void;
    request?: { label: string; onclick: () => void };
  }

  const SUPPORT_TEXT: Record<string, string> = {
    supported: "Supported",
    "not-supported": "Not supported",
    unknown: "Unknown",
  };
</script>

{#snippet slider(p: SliderProps)}
  <label for={p.id}>{p.label}</label>
  <div class="range-row">
    <input
      id={p.id}
      type="range"
      min={p.min}
      max={p.max}
      step={p.step}
      value={p.value}
      aria-valuetext={p.text}
      oninput={(e) => p.onvalue(Number((e.currentTarget as HTMLInputElement).value))}
    />
    <output for={p.id}>{p.text}</output>
    {#if p.request}
      {@const r = p.request}
      <button type="button" disabled={panel.requestsDisabled} onclick={r.onclick}>{r.label}</button>
    {/if}
  </div>
{/snippet}

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
    <p class="error" role="alert">
      {panel.error.hint ? `${panel.error.message} ${panel.error.hint}` : panel.error.message}
    </p>
  {/if}

  <section class="card" aria-labelledby="trigger-detection-title">
    <h2 id="trigger-detection-title">Detection</h2>
    {#if panel.detection}
      <dl class="facts">
        {#each detectionChecks(panel.detection) as check (check.label)}
          <dt>{check.label}</dt>
          <dd>{check.text}</dd>
        {/each}
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
      {#if panel.detection.hint}
        <p class="muted">{panel.detection.hint}</p>
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
                  disabled={panel.requestsDisabled}
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
              {@render slider({
                id: `trigger-range-${o.position}`,
                label: `${rangeName(o)} #${o.position}, ${volts(o.min_voltage_mv)}–${volts(o.max_voltage_mv)}`,
                min: o.min_voltage_mv,
                max: o.max_voltage_mv,
                step: rangeStep(o),
                value: mv,
                text: volts(mv),
                onvalue: (v) => (rangeMv[o.position] = v),
                request: {
                  label: `Request ${rangeName(o)} ${volts(mv)}`,
                  onclick: () =>
                    void panel.ask({
                      protocol: "pd",
                      position: o.position,
                      voltage_mv: mv,
                      current_ma: rangeCurrent(o, mv),
                    }),
                },
              })}
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
          {#if isFixedVoltProtocol(s.protocol)}
            {@const p = s.protocol}
            <div class="offers" role="group" aria-label="{s.title} voltages">
              {#each LEGACY_FIXED_VOLTS[p] as v (v)}
                <button
                  type="button"
                  disabled={requestsOff}
                  onclick={() => void panel.ask({ protocol: p, volts: v })}>{v} V</button
                >
              {/each}
            </div>
          {/if}
          {#if s.protocol === "qc"}
            <div class="range">
              {@render slider({
                id: "trigger-qc3",
                label: `QC 3.0, ${volts(QC3_RANGE.minMv)}–${volts(QC3_RANGE.maxMv)}`,
                min: QC3_RANGE.minMv,
                max: QC3_RANGE.maxMv,
                step: QC3_RANGE.stepMv,
                value: qc3Mv,
                text: volts(qc3Mv),
                onvalue: (v) => (qc3Mv = v),
                request: {
                  label: `Request QC 3.0 ${volts(qc3Mv)}`,
                  onclick: () => void panel.ask({ protocol: "qc3", voltage_mv: qc3Mv }),
                },
              })}
            </div>
          {/if}
          {#if isLimitsProtocol(s.protocol)}
            {@const p = s.protocol}
            {#each s.section.limits as l, i (limitsKey(p, l, i))}
              {@const key = limitsKey(p, l, i)}
              {@const name = limitsName(s.title, l)}
              {@const value = limitsValue(key, l)}
              {@const id = `trigger-${key}`}
              <div class="range">
                {@render slider({
                  id: `${id}-v`,
                  label: `${name} voltage, ${volts(l.min_voltage_mv)}–${volts(l.max_voltage_mv)}`,
                  min: l.min_voltage_mv,
                  max: l.max_voltage_mv,
                  step: 10,
                  value: value.voltage_mv,
                  text: volts(value.voltage_mv),
                  onvalue: (v) => (limitsMv[key] = v),
                })}
                {@render slider({
                  id: `${id}-c`,
                  label: `${name} current, ${amps(l.min_current_ma)}–${amps(l.max_current_ma)}${
                    l.max_power_mw === null
                      ? ""
                      : `, up to ${Number((l.max_power_mw / 1000).toFixed(1))} W`
                  }`,
                  min: l.min_current_ma,
                  max: l.max_current_ma,
                  step: 10,
                  value: value.current_ma,
                  text: amps(value.current_ma),
                  // Stored clamped, so the thumb stays where the request is.
                  onvalue: (v) =>
                    (limitsMa[key] = clampToLimits(l, value.voltage_mv, v).current_ma),
                })}
                <button
                  type="button"
                  disabled={requestsOff || (p === "ufcs" && l.index === null)}
                  title={p === "ufcs" && l.index === null
                    ? "This UFCS capability has no number, so it cannot be requested."
                    : undefined}
                  onclick={() =>
                    void panel.ask(limitsRequest(p, l, value.voltage_mv, value.current_ma))}
                  >Request {name}
                  {volts(value.voltage_mv)}
                  {amps(value.current_ma)}</button
                >
              </div>
            {/each}
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
    {:else if panel.requestFailure}
      <p class="error">
        Request failed: {panel.requestFailure.message}{panel.requestFailure.hint
          ? ` ${panel.requestFailure.hint}`
          : ""}
      </p>
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

  {#if pdConsole && panel.sessionOpen}
    <PdConsoleView {pdConsole} />
  {/if}

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
