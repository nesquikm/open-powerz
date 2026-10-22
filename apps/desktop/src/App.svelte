<script lang="ts">
  import {
    attach,
    backendAvailable,
    clearHistory,
    connect,
    disconnect,
    exportSession,
    resetStats as resetStatsCommand,
    revealPath,
    setRate,
    startRecording,
    stopRecording,
    toUiError,
    type BackendEvent,
  } from "./lib/backend";
  import { attachSession } from "./lib/attach";
  import { AutoDismiss, type HoldReason } from "./lib/autoDismiss";
  import About from "./lib/components/About.svelte";
  import Chart from "./lib/components/Chart.svelte";
  import ErrorBanner from "./lib/components/ErrorBanner.svelte";
  import Notice from "./lib/components/Notice.svelte";
  import Readout from "./lib/components/Readout.svelte";
  import ShortcutHelp from "./lib/components/ShortcutHelp.svelte";
  import StatsPanel from "./lib/components/StatsPanel.svelte";
  import TopBar from "./lib/components/TopBar.svelte";
  import { onMount, setContext } from "svelte";
  import {
    ExportJob,
    exportNoticeText,
    rateChange,
    Recording,
    revealActionLabel,
  } from "./lib/recording.svelte";
  import { Session } from "./lib/session.svelte";
  import { StatsControls } from "./lib/stats.svelte";
  import { shortcutFor, type ShortcutAction } from "./lib/shortcuts";
  import {
    applyTheme,
    browserStorage,
    DARK_QUERY,
    loadTheme,
    motionSettings,
    nextTheme,
    prefersReducedMotion,
    REDUCED_MOTION_QUERY,
    resolveTheme,
    saveTheme,
    systemPrefersDark,
    watchMediaQuery,
    type MotionSettings,
    type ThemeChoice,
  } from "./lib/theme";

  const session = new Session();
  const recording = new Recording();
  /** A stream is running that can be recorded. */
  const canRecord = $derived(session.state === "streaming" || session.state === "rate_locked");
  const exportJob = new ExportJob();
  // Information notices leave by themselves; errors stay until dismissed (AC-AKNVND.8).
  const recordingNoticeTimer = new AutoDismiss(() => recording.dismiss());
  const exportNoticeTimer = new AutoDismiss(() => exportJob.dismissNotice());
  $effect(() => {
    if (recording.notice !== null && recording.error === null) recordingNoticeTimer.start();
    else recordingNoticeTimer.stop();
    return () => recordingNoticeTimer.stop();
  });
  $effect(() => {
    if (exportJob.path !== null && exportJob.rows !== null) exportNoticeTimer.start();
    else exportNoticeTimer.stop();
    return () => exportNoticeTimer.stop();
  });
  /** The pointer over the notices, or focus in them, holds both countdowns. */
  function holdNotices(reason: HoldReason, held: boolean): void {
    for (const t of [recordingNoticeTimer, exportNoticeTimer]) {
      if (held) t.hold(reason);
      else t.release(reason);
    }
  }
  /** Whether an event's related target is outside the notice stack. */
  function leftStack(e: PointerEvent | FocusEvent): boolean {
    const stack = e.currentTarget as HTMLElement;
    const to = e.relatedTarget instanceof Node ? e.relatedTarget : null;
    return to === null || !stack.contains(to);
  }
  const statsControls = new StatsControls();
  const statsDeps = {
    resetStats: async () => {
      await resetStatsCommand();
      session.statsReset();
    },
    clearHistory,
    // The backend history is gone: the chart drops what it still holds (FR 5X154V).
    clearChart: () => chart?.clearHistory(),
  };
  /** The open session holds samples to export. */
  const canExport = $derived(canRecord && session.latest !== null);

  const storage = browserStorage();
  let themeChoice: ThemeChoice = $state(loadTheme(storage));
  const matchMedia = window.matchMedia?.bind(window);
  let systemDark = $state(systemPrefersDark(matchMedia));
  // Follow OS light/dark changes (only visible while the choice is System).
  $effect(() => watchMediaQuery(DARK_QUERY, (dark) => (systemDark = dark)));

  const resolvedTheme = $derived(resolveTheme(themeChoice, systemDark));
  $effect(() => {
    applyTheme(resolvedTheme, document.documentElement);
  });

  /** The live chart, fed from the session's batches. */
  let chart: Chart | undefined = $state();

  // Reduced motion (AC-AH2AEK.7): follow the OS setting live; the chart reads
  // `getContext<() => MotionSettings>("motion")().chartScroll`.
  let reducedMotion = $state(prefersReducedMotion(matchMedia));
  const motion: MotionSettings = $derived(motionSettings(reducedMotion));
  setContext("motion", () => motion);
  $effect(() => watchMediaQuery(REDUCED_MOTION_QUERY, (reduced) => (reducedMotion = reduced)));

  function setTheme(choice: ThemeChoice): void {
    themeChoice = choice;
    saveTheme(themeChoice, storage);
  }

  function cycleTheme(): void {
    setTheme(nextTheme(themeChoice));
  }

  /** Rate requested on connect (and changed by the 1–4 shortcuts). */
  let requestedRateSps = $state(50);

  /** A disconnect is on its way; a second press must not send another. */
  let disconnecting = false;

  /** Every event of the session: to the chart, the session and the recording. */
  function onBackendEvent(event: BackendEvent): void {
    // Same guard as the session: a late batch after the end draws nothing.
    if (event.type === "batch" && session.active) chart?.push(event.data.samples);
    if (event.type === "gap" && session.active) chart?.markGap(event.data);
    session.handleEvent(event);
    if (event.type === "recording") recording.handleEvent(event.data);
    if (event.type === "status" && event.data.state === "disconnected") {
      recording.sessionEnded();
    }
  }

  // On load, ask the backend whether a session is still running (a reload);
  // with none, the app starts disconnected as on a first launch (FR VEDDFD).
  onMount(() => {
    if (!backendAvailable()) return;
    void attachSession(attach, onBackendEvent, (snapshot) => {
      session.adopt(snapshot);
      // The new page's buffer is empty: fill the live windows from the history.
      chart?.backfillFromHistory();
      if (snapshot.recording) recording.adopt(snapshot.recording);
      if (!snapshot.rate_locked && snapshot.rate_sps !== null) {
        requestedRateSps = snapshot.rate_sps;
      }
    });
  });

  async function toggleConnection(): Promise<void> {
    // A second press while a connect is still pending would race it.
    if (!session.canToggleConnect) return;
    if (session.active) {
      if (disconnecting) return;
      disconnecting = true;
      // A refused disconnect leaves the stream running: show the error, keep the state.
      await disconnect()
        .catch((err: unknown) => session.reportError(toUiError(err)))
        .finally(() => (disconnecting = false));
      return;
    }
    const id = session.selectedId;
    if (id === null) return;
    session.beginConnect();
    chart?.reset();
    try {
      await connect(id, requestedRateSps, onBackendEvent);
    } catch (err) {
      session.failConnect(toUiError(err));
    }
  }

  async function selectRate(sps: number): Promise<void> {
    // The picker is disabled while recording; the keys 1–4 follow the same lock.
    // A meter that set its own rate stays locked after the recording too: the backend's
    // own refusal says so instead.
    const change = rateChange(sps, {
      recording: recording.phase !== "idle" && session.state !== "rate_locked",
    });
    if (change.kind === "refused") {
      recording.notice = change.notice;
      return;
    }
    requestedRateSps = sps;
    if (session.state !== "streaming" && session.state !== "rate_locked") return;
    // A failed rate change leaves the stream running; just surface the error.
    await setRate(sps).catch((err: unknown) => session.reportError(toUiError(err)));
  }

  /** Export button: save dialog, then the session history so far, off the UI thread. */
  function exportHistory(): void {
    if (!canExport) return;
    void exportJob.run({ exportSession, now: () => new Date() });
  }

  /** "Reveal in Finder" on macOS, "Show in folder" elsewhere. */
  const revealLabel = revealActionLabel(navigator.platform);

  /** Shows a saved file in the system file manager. */
  function revealFile(path: string): void {
    void revealPath(path).catch((err: unknown) => session.reportError(toUiError(err)));
  }

  /** Puts a saved file's path on the clipboard. */
  function copyPath(path: string): void {
    void navigator.clipboard
      ?.writeText(path)
      .catch((err: unknown) => session.reportError(toUiError(err)));
  }

  /** Record button / Space: start (save dialog first) or stop the CSV recording. */
  function toggleRecording(): void {
    void recording.toggle({ canRecord, startRecording, stopRecording, now: () => new Date() });
  }

  /** R shortcut: zero the session statistics. */
  function resetStats(): void {
    if (!canRecord) return;
    void statsControls.reset(statsDeps);
  }

  /** End shortcut: the chart back to its live window. */
  function returnChartToLive(): void {
    chart?.returnToLive();
  }

  let helpOpen = $state(false);
  let aboutOpen = $state(false);
  /** The control that opened About (focus goes back to it on close). */
  let aboutOpener: HTMLElement | null = $state(null);

  function dispatchShortcut(action: ShortcutAction): void {
    switch (action.kind) {
      case "record":
        toggleRecording();
        break;
      case "connect":
        void toggleConnection();
        break;
      case "reset_stats":
        resetStats();
        break;
      case "rate":
        void selectRate(action.sps);
        break;
      case "live":
        returnChartToLive();
        break;
      case "home":
        chart?.goHome();
        break;
      case "shuttle":
        chart?.shuttle(action.key);
        break;
      case "zoom":
        chart?.zoomStep(action.direction);
        break;
      case "fit":
        chart?.fitSession();
        break;
      case "step":
        chart?.stepPlayheadBy(action.direction, action.coarse);
        break;
      case "theme":
        cycleTheme();
        break;
      case "help":
        helpOpen = true;
        break;
    }
  }

  function onWindowKeydown(e: KeyboardEvent): void {
    if (e.repeat) return;
    const target = e.target instanceof HTMLElement ? e.target : null;
    const action = shortcutFor({
      key: e.key,
      ctrlKey: e.ctrlKey,
      metaKey: e.metaKey,
      altKey: e.altKey,
      shiftKey: e.shiftKey,
      target:
        target === null
          ? null
          : {
              tagName: target.tagName,
              type: target instanceof HTMLInputElement ? target.type : undefined,
              isContentEditable: target.isContentEditable,
            },
    });
    if (action === null) return;
    e.preventDefault();
    dispatchShortcut(action);
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

{#snippet fileActions(path: string)}
  <div class="file-actions">
    <button type="button" onclick={() => revealFile(path)}>{revealLabel}</button>
    <button type="button" onclick={() => copyPath(path)}>Copy path</button>
  </div>
{/snippet}

<div class="shell">
  <TopBar
    {session}
    {themeChoice}
    onSetTheme={setTheme}
    onToggleConnect={() => void toggleConnection()}
    onAbout={(opener) => {
      aboutOpener = opener;
      aboutOpen = true;
    }}
    onSelectRate={(sps) => void selectRate(sps)}
    {recording}
    {canRecord}
    onToggleRecording={toggleRecording}
    {exportJob}
    {canExport}
    onExport={exportHistory}
    rateSps={session.rateSps ?? requestedRateSps}
  />
  <ErrorBanner {session} />
  <main class="content">
    <div class="main-area">
      <Readout {session} />
      <section class="chart panel" aria-label="Chart">
        <Chart
          bind:this={chart}
          theme={resolvedTheme}
          rateSps={session.rateSps}
          active={session.active}
        />
      </section>
    </div>
    <!-- Wide, notices float in one compact stack over the side panel, so any number of
         them never moves the main layout (AC-AKNVND.3); stacked, they sit in the page
         between the chart card and the Stats panel (AC-SX3EDV.3). -->
    <div
      class="notice-stack"
      role="presentation"
      onpointerover={() => holdNotices("pointer", true)}
      onpointerout={(e) => {
        if (leftStack(e)) holdNotices("pointer", false);
      }}
      onfocusin={() => holdNotices("focus", true)}
      onfocusout={(e) => {
        if (leftStack(e)) holdNotices("focus", false);
      }}
    >
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
    </div>
    <div class="side-panel">
      <StatsPanel
        stats={session.stats}
        controls={statsControls}
        deps={statsDeps}
        canReset={canRecord}
      />
    </div>
  </main>
  {#if helpOpen}
    <ShortcutHelp onClose={() => (helpOpen = false)} />
  {/if}
  {#if aboutOpen}
    <About opener={aboutOpener} onClose={() => (aboutOpen = false)} />
  {/if}
</div>

<style>
  .file-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.375rem;
  }
  .notice-stack {
    position: fixed;
    right: 1rem;
    bottom: 1rem;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    width: 18rem;
    max-width: calc(100vw - 2rem);
    max-height: calc(100vh - 6rem);
    overflow-y: auto;
    pointer-events: none;
  }
  .notice-stack:empty {
    display: none;
  }
  .notice-stack > :global(.notice) {
    margin: 0;
    pointer-events: auto;
    background: var(--surface);
    box-shadow: 0 0.25rem 1rem rgb(0 0 0 / 0.25);
    font-size: 0.875rem;
  }
  .shell {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    min-width: 0;
  }
  /* Wide (>= 1200 px, LAYOUT_BREAKPOINT_PX in lib/layout.ts): stats beside the chart. */
  .content {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 18rem;
    grid-template-areas: "main side";
    gap: 1rem;
    padding: 1rem;
    min-width: 0;
  }
  .main-area {
    grid-area: main;
    display: grid;
    grid-template-rows: auto minmax(16rem, 1fr);
    gap: 1rem;
    min-width: 0;
  }
  .side-panel {
    grid-area: side;
    min-width: 0;
  }
  .chart {
    min-width: 0;
    min-height: 0;
    padding: 0.75rem;
  }
  /* Stacked (< 1200 px): stats panel moves under the chart, notices sit in the page
     between them (AC-SX3EDV.3). A column of boxes, not grid rows: the main area keeps
     its own height and spare height goes to the side panel, so notices push Stats
     down and never take height from the chart card. That height follows the window,
     never the notices: a tall window still gets a tall chart. */
  @media (max-width: 1199.98px) {
    .content {
      display: flex;
      flex-direction: column;
    }
    .main-area {
      flex: none;
      min-height: 60vh;
    }
    .side-panel {
      flex: 1 0 auto;
    }
    .notice-stack {
      position: static;
      width: auto;
      max-width: none;
      max-height: none;
      overflow: visible;
    }
  }
</style>
