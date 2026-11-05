<script lang="ts">
  /** The control bar of one meter's tile (AC-4PJRVM.1): named after its meter, it acts on
   * that meter only. */
  import type { Snippet } from "svelte";
  import {
    RATE_OPTIONS,
    ratePicker,
    recordButton,
    type ExportJob,
    type Recording,
  } from "../recording.svelte";
  import type { Session } from "../session.svelte";
  import type { MeterNotice } from "../meterNotices.svelte";
  import MeterNoticeList from "./MeterNoticeList.svelte";
  import RecorderButtons from "./RecorderButtons.svelte";

  let {
    label,
    session,
    held = false,
    recording,
    exportJob,
    notices,
    onDismissNotice,
    rateSps,
    onSelectRate,
    onToggleRecording,
    onTogglePause,
    onRecorderOptions,
    onExport,
    onExportDb,
    onDisconnect,
    fileActions,
  }: {
    label: string;
    session: Session;
    /** A logo write holds the meter (AC-MEHQC0.3): no rate change, no disconnect. */
    held?: boolean;
    recording: Recording;
    exportJob: ExportJob;
    /** This meter's notices. */
    notices: readonly MeterNotice[];
    onDismissNotice: (notice: MeterNotice) => void;
    rateSps: number;
    onSelectRate: (sps: number) => void;
    onToggleRecording: () => void;
    onTogglePause: () => void;
    onRecorderOptions: () => void;
    onExport: () => void;
    onExportDb: () => void;
    onDisconnect: () => void;
    /** What can be done with a saved file. */
    fileActions: Snippet<[string]>;
  } = $props();

  const canRecord = $derived(session.meterConnected);
  const picker = $derived(ratePicker(session.state, session.rateSps, recording.phase !== "idle"));
  const record = $derived(recordButton(recording.phase, canRecord));
</script>

<div class="controls" role="group" aria-label={label}>
  <select
    aria-label="Sample rate"
    title={picker.label ?? undefined}
    disabled={picker.disabled || held}
    value={rateSps}
    onchange={(e) => onSelectRate(Number(e.currentTarget.value))}
  >
    {#each RATE_OPTIONS as sps (sps)}
      <option value={sps}>{sps} SPS</option>
    {/each}
  </select>
  <button
    type="button"
    title="Record to CSV"
    aria-pressed={recording.phase === "recording"}
    disabled={record.disabled}
    onclick={onToggleRecording}
  >
    {record.label}
  </button>
  <RecorderButtons {recording} {onTogglePause} {onRecorderOptions} />
  <button
    type="button"
    title="Export the session so far to CSV"
    disabled={exportJob.running || !canRecord || session.latest === null}
    onclick={onExport}
  >
    Export
  </button>
  <button
    type="button"
    title="Export the session so far to a vendor-layout .db file"
    disabled={exportJob.running || !canRecord || session.latest === null}
    onclick={onExportDb}
  >
    Export .db
  </button>
  {#if recording.indicator !== null}
    <span class="indicator" class:paused={recording.paused} role="timer">{recording.indicator}</span
    >
  {/if}
  {#if exportJob.running}
    <progress
      aria-label="Export progress"
      max={exportJob.total || 1}
      value={exportJob.total > 0 ? exportJob.done : undefined}
    ></progress>
  {/if}
  <button
    type="button"
    class="disconnect"
    disabled={session.flashing || held}
    onclick={onDisconnect}
  >
    Disconnect
  </button>
</div>
<!-- The full view's notice stack is hidden with the grid: a tile says it itself. -->
<MeterNoticeList {notices} onDismiss={onDismissNotice} {fileActions} />

<style>
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .indicator.paused {
    color: var(--muted);
  }
  .indicator {
    color: var(--danger);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .disconnect {
    margin-left: auto;
  }
</style>
