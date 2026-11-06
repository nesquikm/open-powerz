<script lang="ts">
  /** Pause (while a recording runs) and Options, next to a Record button. They add no box
   * of their own, so they sit in the row around them. */
  import { holdTitle } from "../logoHold";
  import type { Recording } from "../recording.svelte";

  let {
    recording,
    held = false,
    onTogglePause,
    onRecorderOptions,
  }: {
    recording: Recording;
    /** A startup logo write holds both (FR 70X5YY). */
    held?: boolean;
    onTogglePause: () => void;
    onRecorderOptions: () => void;
  } = $props();
</script>

{#if recording.phase === "recording"}
  <button
    type="button"
    title={holdTitle(held, "Pause or resume writing rows; the meter and the chart go on")}
    disabled={held}
    onclick={onTogglePause}
  >
    {recording.paused ? "Resume" : "Pause"}
  </button>
{/if}
<button
  type="button"
  aria-label="Recorder options"
  title={holdTitle(held, "Recorder options")}
  disabled={held}
  onclick={(e) => {
    // WebKit leaves a clicked button unfocused, and the dialog hands focus back to this one.
    e.currentTarget.focus();
    onRecorderOptions();
  }}
>
  Options
</button>
