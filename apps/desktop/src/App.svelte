<script lang="ts">
  import {
    attach,
    backendAvailable,
    clearHistory,
    connect,
    disconnect,
    exportSession,
    resetStats as resetStatsCommand,
    setRate,
    startRecording,
    stopRecording,
    toUiError,
    type BackendEvent,
  } from "./lib/backend";
  import { attachSession } from "./lib/attach";
  import About from "./lib/components/About.svelte";
  import Chart from "./lib/components/Chart.svelte";
  import ErrorBanner from "./lib/components/ErrorBanner.svelte";
  import Notice from "./lib/components/Notice.svelte";
  import Readout from "./lib/components/Readout.svelte";
  import ShortcutHelp from "./lib/components/ShortcutHelp.svelte";
  import StatsPanel from "./lib/components/StatsPanel.svelte";
  import TopBar from "./lib/components/TopBar.svelte";
  import { onMount, setContext } from "svelte";
  import { ExportJob, Recording } from "./lib/recording.svelte";
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
  const statsControls = new StatsControls();
  const statsDeps = {
    resetStats: async () => {
      await resetStatsCommand();
      session.statsReset();
    },
    clearHistory,
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

<div class="shell">
  <TopBar
    {session}
    {themeChoice}
    onSetTheme={setTheme}
    onToggleConnect={() => void toggleConnection()}
    onAbout={() => (aboutOpen = true)}
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
  {#if recording.notice !== null || recording.error !== null}
    <Notice
      tone={recording.error ? "error" : "info"}
      message={recording.error?.message ?? null}
      onDismiss={() => recording.dismiss()}
    >
      {#if recording.notice}<p>{recording.notice}</p>{/if}
    </Notice>
  {/if}
  {#if exportJob.error !== null}
    <Notice
      tone="error"
      message={exportJob.error.message}
      onDismiss={() => (exportJob.error = null)}
    />
  {/if}
  <main class="content">
    <div class="main-area">
      <Readout {session} />
      <section class="chart panel" aria-label="Chart">
        <Chart bind:this={chart} theme={resolvedTheme} />
      </section>
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
    <About onClose={() => (aboutOpen = false)} />
  {/if}
</div>

<style>
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
  /* Stacked (< 1200 px): stats panel moves under the chart. */
  @media (max-width: 1199.98px) {
    .content {
      grid-template-columns: minmax(0, 1fr);
      grid-template-areas:
        "main"
        "side";
    }
  }
</style>
