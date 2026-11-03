<script lang="ts">
  import { MARK_STEPS, type MarkKind } from "../chartExtras";
  import InfoButton from "./InfoButton.svelte";

  /** The "Segment marks" and "Mark step" selects of a Chart options group (AC-AXHK2T.4). */
  let {
    marks,
    step,
    onChange,
  }: {
    marks: MarkKind;
    step: number;
    onChange: (marks: MarkKind, step: number) => void;
  } = $props();

  const KINDS: readonly MarkKind[] = ["off", "charge", "energy"];
  /** Each chart has its own pair of selects. */
  const uid = $props.id();
</script>

<span
  class="mark-option"
  title="Marks where the run's charge or energy reached each share of its total"
>
  <label for="{uid}-marks">Segment marks</label>
  <select
    id="{uid}-marks"
    value={marks}
    onchange={(e) => onChange(e.currentTarget.value as MarkKind, step)}
  >
    {#each KINDS as k (k)}
      <option value={k}>{k}</option>
    {/each}
  </select>
  <InfoButton key="section.segmentMarks" />
</span>
<span class="mark-option">
  <label for="{uid}-step">Mark step</label>
  <select
    id="{uid}-step"
    value={String(step)}
    onchange={(e) => onChange(marks, Number(e.currentTarget.value))}
  >
    {#each MARK_STEPS as s (s)}
      <option value={String(s)}>{s}</option>
    {/each}
  </select>
</span>

<style>
  .mark-option {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }
</style>
