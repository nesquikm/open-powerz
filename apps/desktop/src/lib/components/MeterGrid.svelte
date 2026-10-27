<script lang="ts">
  /** One tile per connected meter (FR 1HHT03): its name, serial suffix and live readout,
   * two side by side on a wide window and stacked on a narrow one. */
  import { badgeLabel } from "../format";
  import { gridColumns, tileTitle } from "../meterGrid";
  import type { Meters } from "../meters.svelte";
  import Readout from "./Readout.svelte";

  let {
    meters,
    onExpand,
    onDisconnect,
  }: {
    meters: Meters;
    /** "Expand": meter `id` in the full single-meter view. */
    onExpand: (id: string) => void;
    /** "Disconnect": ends meter `id`'s session only (AC-1HHT03.3). */
    onDisconnect: (id: string) => void;
  } = $props();

  let width = $state(window.innerWidth);
  const columns = $derived(gridColumns(width, meters.entries.length));
</script>

<svelte:window bind:innerWidth={width} />

<section
  class="meter-grid"
  aria-label="Meters"
  style:grid-template-columns="repeat({columns}, minmax(0, 1fr))"
>
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
        <button type="button" onclick={() => onDisconnect(entry.listing.id)}>Disconnect</button>
      </header>
      <Readout session={entry.session} />
    </article>
  {/each}
</section>

<style>
  .meter-grid {
    display: grid;
    gap: 1rem;
    padding: 1rem;
    align-content: start;
    min-width: 0;
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
