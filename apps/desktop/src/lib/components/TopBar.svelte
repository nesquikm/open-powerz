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
  import {
    ALL_FIT_CLASSES,
    chooseFit,
    EXPORT_IN_MENU_LEVEL,
    fitClasses,
    moreMenuLabel,
    RATE_IN_MENU_LEVEL,
    SETTINGS_IN_MENU_LEVEL,
    startLevel,
    type Fit,
  } from "../topBarFit";
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

  let bar: HTMLElement | undefined = $state();
  /** The overflow level the bar is at (lib/topBarFit.ts); the menu shows what it moved. */
  let level = $state(0);

  function applyFit(el: HTMLElement, fit: Fit): void {
    el.classList.remove(...ALL_FIT_CLASSES);
    el.classList.add(...fitClasses(fit));
  }

  /** The bar is one row: nothing sticks out of it or out of its groups. */
  function fitsOneRow(el: HTMLElement): boolean {
    if (el.scrollWidth > el.clientWidth) return false;
    for (const group of el.querySelectorAll<HTMLElement>(".group")) {
      if (group.scrollWidth > group.clientWidth) return false;
    }
    return true;
  }

  /** Measure the bar at each level in turn (classes only, no re-render) and keep the first fit. */
  function fit(): void {
    const el = bar;
    if (!el) return;
    const chosen = chooseFit(startLevel(window.innerWidth), (f) => {
      applyFit(el, f);
      return fitsOneRow(el);
    });
    applyFit(el, chosen);
    level = chosen.level;
    if (level < SETTINGS_IN_MENU_LEVEL) menuOpen = false;
  }

  // Refit when the window or anything in the bar changes size (a label, the recording
  // timer, export progress, a font that loads late).
  $effect(() => {
    const el = bar;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => fit());
    observer.observe(el);
    for (const group of el.querySelectorAll(".group")) observer.observe(group);
    fit();
    return () => observer.disconnect();
  });

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

{#snippet rateSelect()}
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
{/snippet}

{#snippet exportButton(inMenu: boolean)}
  <button
    type="button"
    class={inMenu ? undefined : "export"}
    class:busy={exportJob.running}
    title="Export the session so far to CSV"
    disabled={exportJob.running || !canExport}
    onclick={() => {
      if (inMenu) menuOpen = false;
      onExport();
    }}
  >
    Export
  </button>
{/snippet}

<header class="top-bar" aria-label="Device controls" bind:this={bar}>
  <div class="group start">
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
      <span class="rate-inline">{@render rateSelect()}</span>
      <!-- Short of room: the note moves into the "⋯" menu (the select keeps it as a tooltip). -->
      {#if rateNote !== null}
        <span class="rate-note inline-note">{rateNote}</span>
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
    {@render exportButton(false)}
    <!-- Room to spare: theme and About inline. Short of room: behind the "⋯" menu. -->
    <span class="settings-inline">
      <ThemeControl choice={themeChoice} onChoose={onSetTheme} />
      <button type="button" onclick={onAbout}>About</button>
    </span>
    <span class="settings-menu">
      <button
        type="button"
        class="more"
        aria-label={moreMenuLabel(level)}
        aria-expanded={menuOpen}
        aria-controls="top-bar-menu"
        onclick={() => (menuOpen = !menuOpen)}>⋯</button
      >
      {#if menuOpen}
        <div id="top-bar-menu" class="menu panel">
          {#if level >= RATE_IN_MENU_LEVEL}
            {@render rateSelect()}
          {/if}
          {#if rateNote !== null}
            <p class="rate-note menu-note">{rateNote}</p>
          {/if}
          {#if level >= EXPORT_IN_MENU_LEVEL}
            {@render exportButton(true)}
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
  /* One row in every state (lib/topBarFit.ts): the bar never wraps on its own; `fit()`
     adds level classes (hide-brand … export-in-menu) until the row fits, then lets the
     device picker shrink (fit-shrink), and only then lets the bar wrap (fit-wrap). */
  .top-bar {
    display: flex;
    flex-wrap: nowrap;
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
  .group {
    display: flex;
    flex-wrap: nowrap;
    flex-shrink: 0;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  /* Items keep their width; an overflow shows up as overflow, which `fit()` measures. */
  .group > * {
    flex-shrink: 0;
  }
  /* The picker shrinks (ellipsis, full label in its tooltip) only once every level is used. */
  .top-bar:global(.fit-shrink) .group.start,
  .top-bar:global(.fit-shrink) .group.start > :global(.device-picker) {
    flex-shrink: 1;
  }
  /* Last resort (a very narrow window): each group wraps as a whole. */
  .top-bar:global(.fit-wrap),
  .top-bar:global(.fit-wrap) .group,
  .top-bar:global(.fit-wrap) .rate {
    flex-wrap: wrap;
  }
  .top-bar:global(.fit-wrap) .group,
  .top-bar:global(.fit-wrap) .group.start > :global(.device-picker) {
    flex-shrink: 1;
  }
  .rate {
    display: inline-flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .rate-inline {
    display: contents;
  }
  .rate-note {
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .inline-note {
    white-space: nowrap;
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
    white-space: nowrap;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.15rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 0.8125rem;
    white-space: nowrap;
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
  .top-bar :global(button) {
    white-space: nowrap;
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
    overflow-wrap: anywhere;
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
  /* Level 1: the wordmark goes (the window title names the app). */
  .top-bar:global(.hide-brand) .brand {
    display: none;
  }
  /* Level 2: theme, About and the rate-locked note move into the menu. */
  .top-bar:global(.settings-in-menu) .settings-inline,
  .top-bar:global(.settings-in-menu) .inline-note {
    display: none;
  }
  .top-bar:global(.settings-in-menu) .settings-menu {
    display: inline-flex;
  }
  /* Level 3 (always below 1000 px): tighter spacing; the recording indicator shows just
     its time, and a running export just its progress bar in place of the (disabled)
     Export button; the hidden text stays for screen readers. */
  .top-bar:global(.compact) {
    column-gap: 0.5rem;
  }
  .top-bar:global(.compact) .group {
    gap: 0.375rem;
  }
  .top-bar:global(.compact) :global(button),
  .top-bar:global(.compact) :global(select) {
    padding-inline: 0.55rem;
  }
  .top-bar:global(.compact) .export.busy {
    display: none;
  }
  .top-bar:global(.compact) .export-progress progress {
    width: 4.5rem;
  }
  .top-bar:global(.compact) .narrow-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  /* Levels 4 and 5: the sample rate, then Export, move into the menu. */
  .top-bar:global(.rate-in-menu) .rate-inline,
  .top-bar:global(.export-in-menu) .export {
    display: none;
  }
</style>
