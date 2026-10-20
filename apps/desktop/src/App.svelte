<script lang="ts">
  import {
    connect,
    disconnect,
    exportSession,
    setRate,
    startRecording,
    stopRecording,
    toUiError,
  } from "./lib/backend";
  import About from "./lib/components/About.svelte";
  import ErrorBanner from "./lib/components/ErrorBanner.svelte";
  import Notice from "./lib/components/Notice.svelte";
  import Readout from "./lib/components/Readout.svelte";
  import ShortcutHelp from "./lib/components/ShortcutHelp.svelte";
  import StatsPanel from "./lib/components/StatsPanel.svelte";
  import TopBar from "./lib/components/TopBar.svelte";
  import { setContext } from "svelte";
  import { ExportJob, Recording } from "./lib/recording.svelte";
  import { Session } from "./lib/session.svelte";
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
  /** The open session holds samples to export. */
  const canExport = $derived(canRecord && session.latest !== null);

  const storage = browserStorage();
  let themeChoice: ThemeChoice = $state(loadTheme(storage));
  const matchMedia = window.matchMedia?.bind(window);
  let systemDark = $state(systemPrefersDark(matchMedia));
  // Follow OS light/dark changes (only visible while the choice is System).
  $effect(() => watchMediaQuery(DARK_QUERY, (dark) => (systemDark = dark)));

  $effect(() => {
    applyTheme(resolveTheme(themeChoice, systemDark), document.documentElement);
  });

  // Reduced motion (AC-AH2AEK.7): follow the OS setting live; the chart reads
  // `getContext<() => MotionSettings>("motion")().chartScroll`.
  let reducedMotion = $state(prefersReducedMotion(matchMedia));
  const motion: MotionSettings = $derived(motionSettings(reducedMotion));
  setContext("motion", () => motion);
  $effect(() => watchMediaQuery(REDUCED_MOTION_QUERY, (reduced) => (reducedMotion = reduced)));

  function cycleTheme(): void {
    themeChoice = nextTheme(themeChoice);
    saveTheme(themeChoice, storage);
  }

  /** Rate requested on connect (and changed by the 1–4 shortcuts). */
  let requestedRateSps = $state(50);

  /** A disconnect is on its way; a second press must not send another. */
  let disconnecting = false;

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
    try {
      await connect(id, requestedRateSps, (event) => {
        session.handleEvent(event);
        if (event.type === "recording") recording.handleEvent(event.data);
        if (event.type === "status" && event.data.state === "disconnected") {
          recording.sessionEnded();
        }
      });
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

  // Hooks filled in by later FRs (stats, chart).
  function resetStats(): void {}
  function returnChartToLive(): void {}

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
    onCycleTheme={cycleTheme}
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
      <section class="chart panel" aria-label="Chart"></section>
    </div>
    <div class="side-panel">
      <StatsPanel />
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
