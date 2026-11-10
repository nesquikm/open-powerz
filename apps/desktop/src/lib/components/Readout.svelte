<script lang="ts">
  import InfoButton from "./InfoButton.svelte";
  import Term from "./Term.svelte";
  import { DASH, currentDirectionText, readoutView } from "../format";
  import type { Session } from "../session.svelte";
  import { STALL_NOTICE } from "../stallNotice";

  let { session }: { session: Session } = $props();

  const view = $derived(readoutView(session.latest, session.tempC, session.rateSps));
  const direction = $derived(currentDirectionText(session.latest?.ibus_a));
  /** The data and CC lines below the tiles: [name, value]. */
  const secondary = $derived<[string, string][]>([
    ["D+", view.dp],
    ["D−", view.dm],
    ["CC1", view.cc1],
    ["CC2", view.cc2],
  ]);
</script>

<section class="readout panel" aria-label="Live readout">
  <span class="readout-info"><InfoButton key="section.meter" /></span>
  <div class="primary">
    <div class="cell">
      <span class="label"><Term key="term.vbus">VBUS</Term></span>
      <span class="value" aria-label="VBUS"
        ><span class="digits" class:empty={view.vbus === DASH}>{view.vbus}</span><span class="unit"
          >V</span
        ></span
      >
    </div>
    <div class="cell">
      <span class="label"><Term key="term.ibus">IBUS</Term></span>
      <span class="value" aria-label="IBUS"
        ><span class="digits" class:empty={view.ibus === DASH}>{view.ibus}</span><span class="unit"
          >A</span
        ><span class="arrow" aria-hidden="true" title={direction ?? undefined}
          >{view.ibusArrow}</span
        ></span
      >
      {#if direction}
        <span class="visually-hidden">{direction}</span>
      {/if}
    </div>
    <div class="cell">
      <span class="label">Power</span>
      <span class="value" aria-label="Power"
        ><span class="digits" class:empty={view.power === DASH}>{view.power}</span><span
          class="unit">W</span
        ></span
      >
    </div>
    <div class="cell" title={view.tempTooltip ?? undefined}>
      <span class="label">Temp</span>
      <span
        class="value"
        aria-label="Temperature"
        aria-describedby={view.tempTooltip ? "readout-temp-note" : undefined}
        ><span class="digits" class:empty={view.temp === DASH}>{view.temp}</span><span class="unit"
          >°C</span
        ></span
      >
      {#if view.tempTooltip}
        <span id="readout-temp-note" class="visually-hidden">{view.tempTooltip}</span>
      {/if}
    </div>
  </div>
  <div class="secondary">
    <!-- Only "CC" is the term, so the readout adds no second button named CC1/CC2 beside
         the chart legend's series toggles. -->
    {#each secondary as [name, value] (name)}
      <span class="line" aria-label={name}>
        <span class="label"
          >{#if name === "D+" || name === "D−"}<Term key="term.dataLines">{name}</Term>{:else}<Term
              key="term.cc">CC</Term
            >{name.slice("CC".length)}{/if}</span
        >
        <span class="small-value"
          ><span class="digits" class:empty={value === DASH}>{value}</span><span class="unit"
            >V</span
          ></span
        >
      </span>
    {/each}
  </div>
  {#if session.stalled}
    <p class="stall-notice" role="status">{STALL_NOTICE}</p>
  {/if}
</section>

<style>
  .stall-notice {
    margin: 0;
    padding-top: 0.75rem;
    border-top: 1px solid var(--border);
    color: var(--muted);
  }
  .readout {
    position: relative;
    min-width: 0;
    display: grid;
    gap: 1rem;
    padding: 1rem 1.25rem;
    font-variant-numeric: tabular-nums;
    container-type: inline-size;
  }
  .readout-info {
    position: absolute;
    top: 0.5rem;
    right: 0.5rem;
  }
  /* 2 × 2 tiles, or 4 across when the panel is wide enough for the widest
     values ("-10.000 A ←", "100.000 W") at the full type size. */
  .primary {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1rem 1.5rem;
  }
  @container (min-width: 60rem) {
    .primary {
      grid-template-columns: repeat(4, minmax(0, 1fr));
    }
  }
  /* Too narrow for two values side by side: one tile per row, at the full type size. */
  @container (max-width: 30rem) {
    .primary {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .cell {
    display: grid;
    min-width: 0;
  }
  .label {
    font-size: 0.85rem;
    color: var(--muted);
  }
  .value {
    /* >= 40 px, fixed-width box so changing digits never shift the layout. */
    font-size: 3rem;
    line-height: 1.1;
    min-width: 7ch;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    font-size: 0.5em;
    margin-left: 0.2em;
  }
  .arrow {
    display: inline-block;
    width: 1em;
    font-size: 0.5em;
    text-align: center;
  }
  /* Same four columns as the readout above, so the values line up. */
  .secondary {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 1rem 1.5rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--border);
  }
  /* The "—" of a missing value: smaller and quieter than a reading. */
  .digits.empty {
    color: var(--muted);
    font-size: 0.6em;
    vertical-align: middle;
  }
  .line {
    display: grid;
    gap: 0.1rem;
    min-width: 0;
  }
  .small-value .unit {
    font-size: 0.75em;
    font-weight: 400;
  }
  .small-value {
    font-size: 1.125rem;
    font-weight: 600;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
</style>
