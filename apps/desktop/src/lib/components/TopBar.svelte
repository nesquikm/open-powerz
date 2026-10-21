<script lang="ts">
  import { badgeLabel } from "../format";
  import {
    RATE_OPTIONS,
    formatRecordingElapsed,
    ratePicker,
    recordingRowsSuffix,
    type ExportJob,
    type Recording,
  } from "../recording.svelte";
  import type { Session } from "../session.svelte";
  import type { ThemeChoice } from "../theme";
  import DevicePicker from "./DevicePicker.svelte";
  import ThemeControl from "./ThemeControl.svelte";

  let {
    session,
    themeChoice,
    onSetTheme,
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
    onSetTheme: (choice: ThemeChoice) => void;
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

  /** The "⋯" menu that holds theme and About (and the rate-locked note) on narrow windows. */
  let menuOpen = $state(false);

  const picker = $derived(ratePicker(session.state, session.rateSps, recording.phase !== "idle"));
  /** The locked rate is news worth showing; "stop to change" is only a tooltip. */
  const rateNote = $derived(session.state === "rate_locked" ? picker.label : null);
  const recordDisabled = $derived(
    recording.phase === "starting" ||
      recording.phase === "stopping" ||
      (recording.phase === "idle" && !canRecord),
  );
</script>

<svelte:window
  onkeydown={(e) => {
    if (menuOpen && e.key === "Escape") menuOpen = false;
  }}
/>

<header class="top-bar" aria-label="Device controls">
  <div class="group">
    <span class="brand wide-only">open-powerz</span>
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
        title={picker.label ?? undefined}
        disabled={picker.disabled}
        value={rateSps}
        onchange={(e) => onSelectRate(Number(e.currentTarget.value))}
      >
        {#each RATE_OPTIONS as sps (sps)}
          <option value={sps}>{sps} SPS</option>
        {/each}
      </select>
      <!-- Narrow windows: the note moves into the "⋯" menu (the select keeps it as a tooltip). -->
      {#if rateNote !== null}
        <span class="rate-note wide-only">{rateNote}</span>
      {/if}
    </span>
    {#if recording.indicator !== null}
      <!-- Narrow windows show just the time; the row count stays for screen readers. -->
      <span class="rec-indicator" role="timer" title={`Recording to CSV: ${recording.indicator}`}
        ><span class="rec-dot" aria-hidden="true"></span><span class="visually-hidden"
          >Recording:
        </span>REC {formatRecordingElapsed(recording.elapsedMs)}<span class="narrow-hidden"
          >{recordingRowsSuffix(recording.rows)}</span
        ></span
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
        <span class="narrow-hidden"
          >Exporting {exportJob.done.toLocaleString("en-US")} / {exportJob.total.toLocaleString(
            "en-US",
          )}</span
        >
      </span>
    {/if}
    <button
      type="button"
      class="export"
      class:busy={exportJob.running}
      title="Export the session so far to CSV"
      disabled={exportJob.running || !canExport}
      onclick={onExport}
    >
      Export
    </button>
    <!-- Wide windows: theme and About inline. Narrow: behind the "⋯" menu. -->
    <span class="settings-inline">
      <ThemeControl choice={themeChoice} onChoose={onSetTheme} />
      <button type="button" onclick={onAbout}>About</button>
    </span>
    <span class="settings-menu">
      <button
        type="button"
        class="more"
        aria-label="More: theme and About"
        aria-expanded={menuOpen}
        aria-controls="top-bar-menu"
        onclick={() => (menuOpen = !menuOpen)}>⋯</button
      >
      {#if menuOpen}
        <div id="top-bar-menu" class="menu panel">
          {#if rateNote !== null}
            <p class="rate-note menu-note">{rateNote}</p>
          {/if}
          <ThemeControl choice={themeChoice} onChoose={onSetTheme} />
          <button
            type="button"
            onclick={() => {
              menuOpen = false;
              onAbout();
            }}>About</button
          >
        </div>
      {/if}
    </span>
  </div>
</header>

<style>
  .top-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 0.75rem;
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
    white-space: nowrap;
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
  .export-progress progress {
    width: 6rem;
  }
  .export-progress {
    display: inline-flex;
    white-space: nowrap;
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
  .settings-inline {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
  }
  .settings-menu {
    display: none;
    position: relative;
  }
  .more {
    min-width: 2.25rem;
    font-weight: 700;
  }
  .menu-note {
    margin: 0;
    max-width: 16rem;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 0.375rem);
    /* Size to the content, not to the small button it hangs from. */
    width: max-content;
    z-index: 10;
    display: grid;
    gap: 0.5rem;
    padding: 0.75rem;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
  }
  /* Below ~1000 px the bar keeps one row in every state: the wordmark goes (the
     window title names the app), theme, About and the rate-locked note move into
     the menu, the recording indicator shows just its time, and a running export
     shows just its progress bar in place of the (disabled) Export button; the
     hidden text stays for screen readers. */
  @media (max-width: 999.98px) {
    .top-bar {
      column-gap: 0.5rem;
    }
    .group {
      gap: 0.375rem;
    }
    .top-bar :global(button),
    .top-bar :global(select) {
      padding-inline: 0.55rem;
    }
    .settings-inline,
    .wide-only {
      display: none;
    }
    .settings-menu {
      display: inline-flex;
    }
    .export.busy {
      display: none;
    }
    .export-progress progress {
      width: 4.5rem;
    }
    .narrow-hidden {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
  }
</style>
