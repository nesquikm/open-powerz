<script lang="ts">
  /** The control bar of one meter's tile (AC-4PJRVM.1): named after its meter, it acts on
   * that meter only. */
  import type { Snippet } from "svelte";
  import {
    RATE_OPTIONS,
    exportNoticeText,
    ratePicker,
    recordButton,
    type ExportJob,
    type Recording,
  } from "../recording.svelte";
  import type { Session } from "../session.svelte";
  import Notice from "./Notice.svelte";

  let {
    label,
    session,
    recording,
    exportJob,
    rateSps,
    onSelectRate,
    onToggleRecording,
    onExport,
    onDisconnect,
    fileActions,
  }: {
    label: string;
    session: Session;
    recording: Recording;
    exportJob: ExportJob;
    rateSps: number;
    onSelectRate: (sps: number) => void;
    onToggleRecording: () => void;
    onExport: () => void;
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
    disabled={picker.disabled}
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
  <button
    type="button"
    title="Export the session so far to CSV"
    disabled={exportJob.running || !canRecord || session.latest === null}
    onclick={onExport}
  >
    Export
  </button>
  {#if recording.indicator !== null}
    <span class="indicator" role="timer">{recording.indicator}</span>
  {/if}
  {#if exportJob.running}
    <progress
      aria-label="Export progress"
      max={exportJob.total || 1}
      value={exportJob.total > 0 ? exportJob.done : undefined}
    ></progress>
  {/if}
  <button type="button" class="disconnect" disabled={session.flashing} onclick={onDisconnect}>
    Disconnect
  </button>
</div>
<!-- The full view's notice stack is hidden with the grid: a tile says it itself. -->
{#if recording.notice !== null || recording.error !== null}
  <Notice
    tone={recording.error ? "error" : "info"}
    message={recording.error?.message ?? null}
    onDismiss={() => recording.dismiss()}
  >
    {#if recording.notice}<p>{recording.notice}</p>{/if}
    {#if recording.savedPath !== null}
      {@render fileActions(recording.savedPath)}
    {/if}
  </Notice>
{/if}
{#if exportJob.path !== null && exportJob.rows !== null}
  {@const path = exportJob.path}
  <Notice tone="info" onDismiss={() => exportJob.dismissNotice()}>
    <p>{exportNoticeText({ rows: exportJob.rows, path })}</p>
    {@render fileActions(path)}
  </Notice>
{/if}
{#if exportJob.error !== null}
  <Notice
    tone="error"
    message={exportJob.error.message}
    onDismiss={() => (exportJob.error = null)}
  />
{/if}

<style>
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
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
