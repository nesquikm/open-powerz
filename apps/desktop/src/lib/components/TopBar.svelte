<script lang="ts">
  import { badgeLabel } from "../format";
  import { RATE_OPTIONS, ratePicker, type ExportJob, type Recording } from "../recording.svelte";
  import type { Session } from "../session.svelte";
  import type { ThemeChoice } from "../theme";
  import DevicePicker from "./DevicePicker.svelte";

  let {
    session,
    themeChoice,
    onCycleTheme,
    onToggleConnect,
    onAbout,
    onSelectRate,
    rateSps,
    recording,
    canRecord,
    onToggleRecording,
    exportJob,
    canExport,
    onExport,
  }: {
    session: Session;
    themeChoice: ThemeChoice;
    onCycleTheme: () => void;
    onToggleConnect: () => void;
    onAbout: () => void;
    onSelectRate: (sps: number) => void;
    /** The rate the next connect asks for, or the session's current rate. */
    rateSps: number;
    recording: Recording;
    /** A stream is running that can be recorded. */
    canRecord: boolean;
    onToggleRecording: () => void;
    exportJob: ExportJob;
    /** The session holds samples that can be exported. */
    canExport: boolean;
    onExport: () => void;
  } = $props();

  const THEME_LABELS: Record<ThemeChoice, string> = {
    system: "System",
    light: "Light",
    dark: "Dark",
  };

  const picker = $derived(ratePicker(session.state, session.rateSps, recording.phase !== "idle"));
  const recordDisabled = $derived(
    recording.phase === "starting" ||
      recording.phase === "stopping" ||
      (recording.phase === "idle" && !canRecord),
  );
</script>

<header class="top-bar" aria-label="Device controls">
  <div class="group">
    <span class="brand">open-powerz</span>
    <DevicePicker {session} />
    <button
      type="button"
      class:primary={!session.active}
      disabled={!session.canToggleConnect}
      onclick={onToggleConnect}
    >
      {session.active ? "Disconnect" : "Connect"}
    </button>
    <span class="badge" data-state={session.state} role="status" aria-live="polite"
      >{badgeLabel(session.state)}</span
    >
  </div>
  <div class="group end">
    <span class="rate">
      <select
        aria-label="Sample rate"
        disabled={picker.disabled}
        value={rateSps}
        onchange={(e) => onSelectRate(Number(e.currentTarget.value))}
      >
        {#each RATE_OPTIONS as sps (sps)}
          <option value={sps}>{sps} SPS</option>
        {/each}
      </select>
      {#if picker.label !== null}
        <span class="rate-note">{picker.label}</span>
      {/if}
    </span>
    {#if recording.indicator !== null}
      <span class="rec-indicator" role="timer" title="Recording to CSV"
        ><span class="rec-dot" aria-hidden="true"></span><span class="visually-hidden"
          >Recording:
        </span>{recording.indicator}</span
      >
    {/if}
    <button
      type="button"
      title="Record to CSV (Space)"
      aria-pressed={recording.phase === "recording"}
      disabled={recordDisabled}
      onclick={onToggleRecording}
    >
      {recording.phase === "recording" || recording.phase === "stopping" ? "Stop" : "Record"}
    </button>
    {#if exportJob.running}
      <span class="export-progress" role="status">
        <progress
          aria-label="Export progress"
          max={exportJob.total || 1}
          value={exportJob.total > 0 ? exportJob.done : undefined}
        ></progress>
        Exporting {exportJob.done.toLocaleString("en-US")} / {exportJob.total.toLocaleString(
          "en-US",
        )}
      </span>
    {/if}
    <button
      type="button"
      title="Export the session so far to CSV"
      disabled={exportJob.running || !canExport}
      onclick={onExport}
    >
      Export
    </button>
    <button
      type="button"
      class="theme-toggle"
      title="Cycle theme: System, Light, Dark"
      onclick={onCycleTheme}
    >
      {THEME_LABELS[themeChoice]} theme
    </button>
    <button type="button" onclick={onAbout}>About</button>
  </div>
</header>

<style>
  .top-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1.5rem;
    min-width: 0;
    padding: 0.625rem 1rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    /* Stays in view on scroll, so the recording indicator is always visible. */
    position: sticky;
    top: 0;
    z-index: 1;
  }
  /* Each group wraps as a whole, so a narrow window gets two tidy rows. */
  .group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .rate {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .rate-note {
    font-size: 0.8125rem;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .rec-indicator {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
    color: var(--danger);
  }
  /* --danger reaches 4.5:1 on --bg and --surface in both themes (theme.test.ts). */
  .rec-dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--danger);
  }
  .export-progress {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .brand {
    font-weight: 700;
    letter-spacing: 0.01em;
    margin-right: 0.5rem;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.15rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 0.8125rem;
    background: var(--bg);
  }
  .badge::before {
    content: "";
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--control-border);
  }
  .badge[data-state="connecting"]::before {
    background: var(--accent);
  }
  .badge[data-state="streaming"]::before {
    background: var(--chart-power);
  }
  .badge[data-state="rate_locked"]::before {
    background: var(--chart-d-minus);
  }
  .badge[data-state="error"]::before {
    background: var(--danger);
  }
</style>
