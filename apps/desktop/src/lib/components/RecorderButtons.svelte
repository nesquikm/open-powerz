<script lang="ts">
  /** Pause (while a recording runs) and Options, next to a Record button. They add no box
   * of their own, so they sit in the row around them. */
  import type { Recording } from "../recording.svelte";

  let {
    recording,
    onTogglePause,
    onRecorderOptions,
  }: {
    recording: Recording;
    onTogglePause: () => void;
    onRecorderOptions: () => void;
  } = $props();
</script>

{#if recording.phase === "recording"}
  <button
    type="button"
    title="Pause or resume writing rows; the meter and the chart go on"
    onclick={onTogglePause}
  >
    {recording.paused ? "Resume" : "Pause"}
  </button>
{/if}
<button
  type="button"
  aria-label="Recorder options"
  title="Recorder options"
  onclick={(e) => {
    // WebKit leaves a clicked button unfocused, and the dialog hands focus back to this one.
    e.currentTarget.focus();
    onRecorderOptions();
  }}
>
  Options
</button>
