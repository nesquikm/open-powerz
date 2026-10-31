<script lang="ts">
  /** One tile per connected meter (FR 1HHT03): its name, serial suffix and live readout,
   * two side by side on a wide window and stacked on a narrow one. */
  import type { Snippet } from "svelte";
  import { badgeLabel } from "../format";
  import { controlsLabel, tileTitle } from "../meterGrid";
  import type { MeterEntry, Meters } from "../meters.svelte";
  import type { ExportJob, Recording } from "../recording.svelte";
  import MeterControls from "./MeterControls.svelte";
  import Readout from "./Readout.svelte";

  let {
    meters,
    onExpand,
    onDisconnect,
    recordingOf,
    exportJobOf,
    defaultRateSps,
    onSelectRate,
    onToggleRecording,
    onExport,
    fileActions,
  }: {
    meters: Meters;
    /** "Expand": meter `id` in the full single-meter view. */
    onExpand: (id: string) => void;
    /** "Disconnect": ends meter `id`'s session only (AC-1HHT03.3). */
    onDisconnect: (id: string) => void;
    /** The recording and the export of a tile's meter. */
    recordingOf: (entry: MeterEntry) => Recording;
    exportJobOf: (entry: MeterEntry) => ExportJob;
    /** The rate shown until a meter reports its own. */
    defaultRateSps: number;
    onSelectRate: (entry: MeterEntry, sps: number) => void;
    onToggleRecording: (entry: MeterEntry) => void;
    onExport: (entry: MeterEntry) => void;
    /** What can be done with a saved file. */
    fileActions: Snippet<[string]>;
  } = $props();
</script>

<!-- Side by side only on a wide window: a media query, so the tiles stack in the same frame
     the window narrows, never after a late resize event. -->
<section class="meter-grid" class:paired={meters.entries.length >= 2} aria-label="Meters">
  {#each meters.entries as entry (entry.listing.id)}
    {@const title = tileTitle(entry.listing, entry.session.details?.serial ?? null)}
    <article class="tile" aria-label={title.name}>
      <header>
        <h2>{title.name}</h2>
        {#if title.suffix}<span class="serial">{title.suffix}</span>{/if}
        <!-- Its meter's state: an unplugged meter's tile stays and shows the wait. -->
        <span class="state" data-state={entry.session.state} role="status"
          >{badgeLabel(entry.session.state)}</span
        >
        <button type="button" class="expand" onclick={() => onExpand(entry.listing.id)}>
          Expand
        </button>
      </header>
      <MeterControls
        label={controlsLabel(title)}
        session={entry.session}
        recording={recordingOf(entry)}
        exportJob={exportJobOf(entry)}
        rateSps={entry.session.rateSps ?? defaultRateSps}
        onSelectRate={(sps) => onSelectRate(entry, sps)}
        onToggleRecording={() => onToggleRecording(entry)}
        onExport={() => onExport(entry)}
        onDisconnect={() => onDisconnect(entry.listing.id)}
        {fileActions}
      />
      <Readout session={entry.session} />
    </article>
  {/each}
</section>

<style>
  .meter-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 1rem;
    padding: 1rem;
    align-content: start;
    min-width: 0;
  }
  /* LAYOUT_BREAKPOINT_PX (lib/layout.ts): at most two columns, however many meters. */
  @media (min-width: 1200px) {
    .meter-grid.paired {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .expand {
    margin-left: auto;
  }
  .state {
    color: var(--muted);
    font-size: 0.875rem;
    white-space: nowrap;
  }
  .serial {
    color: var(--muted);
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
  }
</style>
