<script lang="ts">
  import {
    attach,
    backendAvailable,
    clearHistory,
    connect,
    disconnect,
    exportSession,
    pdClear,
    pdDetails,
    pdImport,
    pdOpen,
    pdSave,
    pdStart,
    pdStop,
    resetStats as resetStatsCommand,
    revealPath,
    setRate,
    startRecording,
    stopRecording,
    toUiError,
    triggerBackTo5v,
    triggerClose,
    triggerDetect,
    triggerList,
    triggerOpen,
    triggerRequest,
    triggerRisk,
    type BackendEvent,
    type ExportResult,
    type PdDetails,
    type PdOpened,
    type UiError,
  } from "./lib/backend";
  import {
    defaultPdCaptureName,
    pdImportNoticeText,
    pdOpenDamageText,
    pdSaveNoticeText,
  } from "./lib/pdSave";
  import { attachSession } from "./lib/attach";
  import { AutoDismiss, type HoldReason } from "./lib/autoDismiss";
  import About from "./lib/components/About.svelte";
  import Chart from "./lib/components/Chart.svelte";
  import ErrorBanner from "./lib/components/ErrorBanner.svelte";
  import Notice from "./lib/components/Notice.svelte";
  import PdAnalyzer from "./lib/components/PdAnalyzer.svelte";
  import { PdCapture } from "./lib/pdCapture.svelte";
  import { PdSelection } from "./lib/pdSelection.svelte";
  import Readout from "./lib/components/Readout.svelte";
  import ShortcutHelp from "./lib/components/ShortcutHelp.svelte";
  import StatsPanel from "./lib/components/StatsPanel.svelte";
  import TopBar from "./lib/components/TopBar.svelte";
  import TriggerView from "./lib/components/TriggerView.svelte";
  import { TriggerPanel } from "./lib/trigger.svelte";
  import { onMount, setContext, untrack } from "svelte";
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
  /** The PD analyzer's capture (FR TGE83C). */
  const pd = new PdCapture();
  /** The PD record selected in the chart or the packet table (FR TGE83D). */
  const pdSelection = new PdSelection(pd);
  // A row selected in the packet table moves the chart's playhead to its session time
  // and brings it into view, also while the Meter tab is hidden (AC-TGE83D.3).
  $effect(() => {
    void pdSelection.seq;
    const index = pdSelection.index;
    if (pdSelection.origin !== "table" || index === null) return;
    untrack(() => {
      const at = pd.store.positionOf(index);
      const ms = at < 0 ? NaN : pd.store.sessionMs(at);
      if (Number.isFinite(ms)) chart?.revealPlayheadAt(ms);
    });
  });
  /** The view the tabs show. */
  let view: "meter" | "pd" | "trigger" = $state("meter");
  const VIEWS = ["meter", "pd", "trigger"] as const;
  const VIEW_TABS = [
    { id: "meter", label: "Meter" },
    { id: "pd", label: "PD analyzer" },
    { id: "trigger", label: "Trigger" },
  ] as const;
  /** The trigger view's state (FR DGSQ6S). */
  const trigger = new TriggerPanel({
    open: triggerOpen,
    detect: triggerDetect,
    list: triggerList,
    risk: triggerRisk,
    request: triggerRequest,
    backTo5v: triggerBackTo5v,
    close: triggerClose,
  });

  // Leaving the Trigger tab returns to 5 V and closes the trigger session.
  let lastView: (typeof VIEWS)[number] = "meter";
  $effect(() => {
    const now = view;
    untrack(() => {
      if (lastView === "trigger" && now !== "trigger") void trigger.leave();
      lastView = now;
    });
  });

  /** Arrow keys, Home and End move between the view tabs. */
  function onTabKeydown(e: KeyboardEvent): void {
    const i = VIEWS.indexOf(view);
    let next: number | null = null;
    if (e.key === "Enter" || e.key === " ") {
      // Space must not also start a recording.
      e.preventDefault();
      e.stopPropagation();
      const id = (e.currentTarget as HTMLElement).id.replace(/^tab-/, "");
      view = VIEWS.find((v) => v === id) ?? "meter";
      return;
    }
    if (e.key === "ArrowRight") next = (i + 1) % VIEWS.length;
    else if (e.key === "ArrowLeft") next = (i + VIEWS.length - 1) % VIEWS.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = VIEWS.length - 1;
    if (next === null) return;
    e.preventDefault();
    view = VIEWS[next] ?? "meter";
    document.getElementById(`tab-${view}`)?.focus();
  }

  /** A capture opened from a file leaves the view first: the live one starts empty. */
  function startPd(): void {
    const cleared = pd.opened ? clearPdNow() : Promise.resolve();
    void cleared.then(() => pdStart()).catch((err: unknown) => session.reportError(toUiError(err)));
  }

  function stopPd(): void {
    void pdStop().catch((err: unknown) => session.reportError(toUiError(err)));
  }

  /** The table empties at once and the backend drops its records; rows that arrive
   * meanwhile wait for its answer, and those numbered below the index it answers (sent
   * before it cleared) are dropped. A failed clear keeps them all. */
  function clearPd(): void {
    void clearPdNow().catch((err: unknown) => session.reportError(toUiError(err)));
  }

  function clearPdNow(): Promise<void> {
    const ticket = pd.beginClear();
    return pdClear().then(
      (from) => pd.endClear(from, ticket),
      (err: unknown) => {
        pd.endClear(0, ticket);
        throw err;
      },
    );
  }

  /** The last PD capture save (FR TGE83E), while its notice shows. */
  let pdSaved = $state<ExportResult | null>(null);
  let pdSaveError = $state<UiError | null>(null);
  let pdSaving = false;

  /** Save capture: save dialog, then the PD capture so far as a pdcap file. */
  async function savePd(): Promise<void> {
    if (pdSaving) return;
    pdSaving = true;
    pdSaveError = null;
    try {
      const result = await pdSave(defaultPdCaptureName(new Date()));
      if (result !== null) pdSaved = result;
    } catch (err) {
      pdSaveError = toUiError(err);
    } finally {
      pdSaving = false;
    }
  }

  let pdOpening = false;
  /** Why the last opened PD capture file stopped loading early, while its notice shows. */
  let pdOpenDamage = $state<string | null>(null);
  /** The last vendor PD log import (FR TGE83E, AC .6), while its notice shows. */
  let pdImported = $state<PdOpened | null>(null);

  /** Open capture: open dialog, then the chosen pdcap file in place of the live capture
   * (FR TGE83E); needs no meter. */
  function openPd(): Promise<void> {
    return showPdFile(pdOpen, false);
  }

  /** Import vendor log: open dialog, then the chosen vendor PD log database, read-only,
   * in place of the live capture (FR TGE83E, AC .6); needs no meter. */
  function importPd(): Promise<void> {
    return showPdFile(pdImport, true);
  }

  /** Shows the capture `load` resolves to in place of the live one; `imported` marks a
   * vendor log, which gets its own notice. */
  async function showPdFile(
    load: () => Promise<PdOpened | null>,
    imported: boolean,
  ): Promise<void> {
    // A running capture's rows would arrive unseen behind the opened file.
    if (pdOpening || pd.capturing) return;
    pdOpening = true;
    pdSaveError = null;
    pdOpenDamage = null;
    try {
      const opened = await load();
      if (opened !== null) {
        pd.open(opened.rows, opened.status);
        pdImported = imported ? opened : null;
        pdOpenDamage = pdOpenDamageText(opened);
      }
    } catch (err) {
      pdSaveError = toUiError(err);
    } finally {
      pdOpening = false;
    }
  }

  function loadPdDetails(index: number): Promise<PdDetails | null> {
    return pdDetails(index, pd.opened).catch((err: unknown) => {
      session.reportError(toUiError(err));
      return null;
    });
  }
  // Information notices leave by themselves; errors stay until dismissed (AC-AKNVND.8).
  const recordingNoticeTimer = new AutoDismiss(() => recording.dismiss());
  const exportNoticeTimer = new AutoDismiss(() => exportJob.dismissNotice());
  const pdSaveNoticeTimer = new AutoDismiss(() => (pdSaved = null));
  const pdImportNoticeTimer = new AutoDismiss(() => (pdImported = null));
  $effect(() => {
    if (pdSaved !== null) pdSaveNoticeTimer.start();
    else pdSaveNoticeTimer.stop();
    return () => pdSaveNoticeTimer.stop();
  });
  $effect(() => {
    if (pdImported !== null) pdImportNoticeTimer.start();
    else pdImportNoticeTimer.stop();
    return () => pdImportNoticeTimer.stop();
  });
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
    for (const t of [
      recordingNoticeTimer,
      exportNoticeTimer,
      pdSaveNoticeTimer,
      pdImportNoticeTimer,
    ]) {
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
    pd.handleEvent(event);
    trigger.handleEvent(event);
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
              role: target.getAttribute("role"),
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
  >
    {#snippet views()}
      <div class="view-tabs" role="tablist" aria-label="Views">
        {#each VIEW_TABS as tab (tab.id)}
          <!-- Not <button>: the top bar's one-row layout counts its buttons. -->
          <div
            role="tab"
            id="tab-{tab.id}"
            aria-selected={view === tab.id}
            aria-controls="view-{tab.id}"
            tabindex={view === tab.id ? 0 : -1}
            onclick={() => (view = tab.id)}
            onkeydown={onTabKeydown}
          >
            {tab.label}
          </div>
        {/each}
      </div>
    {/snippet}
  </TopBar>
  <ErrorBanner {session} />
  <main class="views">
    <div
      class="content"
      role="tabpanel"
      id="view-meter"
      aria-labelledby="tab-meter"
      hidden={view !== "meter"}
    >
      <div class="main-area">
        <Readout {session} />
        <section class="chart panel" aria-label="Chart">
          <Chart
            bind:this={chart}
            theme={resolvedTheme}
            rateSps={session.rateSps}
            active={session.active}
            {pd}
            onPdMarker={(index) => {
              pdSelection.select(index, "chart");
              view = "pd";
            }}
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
    </div>
    {#if view === "trigger"}
      <div class="trigger-view" role="tabpanel" id="view-trigger" aria-labelledby="tab-trigger">
        <TriggerView panel={trigger} connected={canRecord} />
      </div>
    {/if}
    {#if view === "pd"}
      <div class="pd-view" role="tabpanel" id="view-pd" aria-labelledby="tab-pd">
        <PdAnalyzer
          {pd}
          connected={canRecord}
          onStart={startPd}
          onStop={stopPd}
          onClear={clearPd}
          onSave={() => void savePd()}
          onOpen={() => void openPd()}
          onImport={() => void importPd()}
          loadDetails={loadPdDetails}
          selection={pdSelection}
        />
        <div
          class="notice-stack pd-notices"
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
          {#if pdSaved !== null}
            {@const path = pdSaved.path}
            <Notice tone="info" onDismiss={() => (pdSaved = null)}>
              <p>{pdSaveNoticeText(pdSaved)}</p>
              {@render fileActions(path)}
            </Notice>
          {/if}
          {#if pdImported !== null}
            <Notice
              tone="info"
              message={pdImportNoticeText(pdImported)}
              onDismiss={() => (pdImported = null)}
            />
          {/if}
          {#if pdSaveError !== null}
            <Notice
              tone="error"
              message={pdSaveError.message}
              onDismiss={() => (pdSaveError = null)}
            />
          {/if}
          {#if pdOpenDamage !== null}
            <Notice tone="error" message={pdOpenDamage} onDismiss={() => (pdOpenDamage = null)} />
          {/if}
        </div>
      </div>
    {/if}
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
  /* Room for the view tabs, which hang under the sticky top bar. */
  .views {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding-top: 2.25rem;
  }
  .view-tabs {
    display: flex;
    gap: 0.25rem;
  }
  .view-tabs [role="tab"] {
    font-size: 0.875rem;
    padding: 0.3rem 0.8rem;
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: 0.375rem 0.375rem 0 0;
    margin-bottom: -1px;
    cursor: pointer;
    color: var(--muted);
  }
  .view-tabs [role="tab"][aria-selected="true"] {
    background: var(--surface);
    border-color: var(--border);
    color: var(--text);
    font-weight: 600;
  }
  .content[hidden] {
    display: none;
  }
  .pd-view {
    /* A definite height, so the packet grid scrolls itself instead of the page. */
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 1rem;
    height: calc(100vh - 8rem);
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
    .notice-stack:not(.pd-notices) {
      position: static;
      width: auto;
      max-width: none;
      max-height: none;
      overflow: visible;
    }
  }
</style>
