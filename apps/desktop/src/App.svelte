<script lang="ts">
  import {
    attach,
    backendAvailable,
    connect,
    connectedMeters,
    currentMeterCommands,
    meterBackend,
    firmwareCancel,
    firmwareFlashStatus,
    firmwareChoose,
    logoChoose,
    logoPreview,
    logoPreviewBuiltin,
    logoRead,
    logoRemove,
    logoSettingsCheck,
    logoUpload,
    logoUploadBuiltin,
    firmwareFlash,
    firmwareInspect,
    pdImport,
    pdOpen,
    pdReleaseOpened,
    revealPath,
    toUiError,
    type AttachedSession,
    type BackendEvent,
    type DeviceListing,
    type ExportProgress,
    type ExportResult,
    type HistorySeries,
    type MeterCommands,
    type MeasurementSource,
    type PdDetails,
    type PdOpened,
    type UiError,
    type RecordingOptions,
    type RunProgressQuery,
  } from "./lib/backend";
  import {
    defaultPdCaptureName,
    pdImportNoticeText,
    pdOpenedNotice,
    type PdOpenedNotice,
    pdOpenDamageText,
    pdSaveNoticeText,
  } from "./lib/pdSave";
  import { attachSession } from "./lib/attach";
  import { firmwareWarningText } from "./lib/deviceDetails";
  import { AutoDismiss, type HoldReason } from "./lib/autoDismiss";
  import About from "./lib/components/About.svelte";
  import {
    loadAdvancedFirmware,
    loadHideSerial,
    loadRecorderSettings,
    loadShowInfoIcons,
    saveAdvancedFirmware,
    saveHideSerial,
    saveShowInfoIcons,
    saveRecorderSettings,
    type RecorderSettings,
  } from "./lib/settings";
  import Chart from "./lib/components/Chart.svelte";
  import ErrorBanner from "./lib/components/ErrorBanner.svelte";
  import FirmwareNotice from "./lib/components/FirmwareNotice.svelte";
  import FirmwareView from "./lib/components/FirmwareView.svelte";
  import { FirmwareUpdate, flashMeter } from "./lib/firmwareUpdate.svelte";
  import Notice from "./lib/components/Notice.svelte";
  import InfoButton from "./lib/components/InfoButton.svelte";
  import PdAnalyzer from "./lib/components/PdAnalyzer.svelte";
  import { PdCapture } from "./lib/pdCapture.svelte";
  import { PdSelection } from "./lib/pdSelection.svelte";
  import Readout from "./lib/components/Readout.svelte";
  import ShortcutHelp from "./lib/components/ShortcutHelp.svelte";
  import StatsPanel from "./lib/components/StatsPanel.svelte";
  import MeterSettingsView from "./lib/components/MeterSettingsView.svelte";
  import { MeterSettingsPanel } from "./lib/meterSettings.svelte";
  import RecordingFileView from "./lib/components/RecordingFileView.svelte";
  import { RecordingFile } from "./lib/recordingFile.svelte";
  import RecordsView from "./lib/components/RecordsView.svelte";
  import StartupLogoView from "./lib/components/StartupLogoView.svelte";
  import { StartupLogoState } from "./lib/logo.svelte";
  import { RecordsPanel } from "./lib/records.svelte";
  import TopBar from "./lib/components/TopBar.svelte";
  import TriggerView from "./lib/components/TriggerView.svelte";
  import { TriggerPanel } from "./lib/trigger.svelte";
  import { PdConsole } from "./lib/console.svelte";
  import { onMount, setContext, untrack } from "svelte";
  import {
    ExportJob,
    type ExportDeps,
    dbFileName,
    exportNoticeText,
    rateChange,
    Recording,
    type RecordingDeps,
    recordingOptions,
    revealActionLabel,
  } from "./lib/recording.svelte";
  import { Session } from "./lib/session.svelte";
  import { copyLive, Meters, type MeterEntry } from "./lib/meters.svelte";
  import MeterGrid from "./lib/components/MeterGrid.svelte";
  import { controlsLabel, tileTitle } from "./lib/meterGrid";
  import MeterNoticeList from "./lib/components/MeterNoticeList.svelte";
  import type { MeterNotice } from "./lib/meterNotices.svelte";
  import AddMeterDialog from "./lib/components/AddMeterDialog.svelte";
  import RecorderOptions from "./lib/components/RecorderOptions.svelte";
  import { canConnectTo } from "./lib/deviceDetails";
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
  /** Every connected meter (FR 1HHT03): the one the full view shows, then each one added
   * beside it, which runs a session and stream of its own. */
  const meters = new Meters();
  /** The listing of a meter a reload picked up before the device list arrived. */
  let attachedListing: DeviceListing | null = null;
  // The meter of the full view joins the meters while its session runs, and stays while
  // it waits for an unplugged meter to come back.
  $effect(() => {
    const id = meters.fullViewId;
    void session.state;
    untrack(() => {
      const listing =
        session.devices.find((d) => d.id === id) ??
        (attachedListing?.id === id ? attachedListing : undefined);
      meters.syncFullView(session, listing);
    });
  });
  /** The meter the full single-meter view shows and acts on (FR 1HHT03). */
  const primaryId = $derived(session.active ? meters.fullViewId : null);
  /** The commands of meter `id`: named by its device id while several meters run, so an
   * action never reaches another meter; with one meter, the commands as before. */
  function commandsOf(id: string | null): MeterCommands {
    return id !== null && meters.entries.length > 1 ? meterBackend(id) : currentMeterCommands;
  }
  /** The commands of the meter the full view shows. */
  function act(): MeterCommands {
    return commandsOf(primaryId);
  }
  /** The chart's history, from the meter the full view shows. */
  const chartHistory = {
    historyRange: <const S extends readonly HistorySeries[]>(
      series: S,
      t0Ms: number,
      t1Ms: number,
      buckets: number,
    ) => act().historyRange(series, t0Ms, t1Ms, buckets),
    historyGaps: (t0Ms: number, t1Ms: number) => act().historyGaps(t0Ms, t1Ms),
    historyFoldBoundary: () => act().historyFoldBoundary(),
    rangeStats: (t0Ms: number, t1Ms: number) => act().rangeStats(t0Ms, t1Ms),
    runProgress: (query: RunProgressQuery) => act().runProgress(query),
  };

  /** Every event of meter `id`: to the full view's pipeline when it shows that meter
   * (followed by the id it connected with, through an unplug and the replug), to its tile's
   * store otherwise. A meter the user disconnected leaves the grid; an unplugged one stays
   * to show its wait. When the full view's meter ends, a running meter takes its place. */
  function routeEvent(id: string, event: BackendEvent): void {
    if (!meters.inFullView(id)) {
      meters.handleTileEvent(id, event);
      // A tile that left has no place to speak: the meter shown takes what it still has to say.
      if (meters.get(id) === undefined && noticesId !== null) meters.notices.move(id, noticesId);
      return;
    }
    onBackendEvent(event);
    meters.fullViewEvent(event);
    if (event.type !== "status" || event.data.state !== "disconnected") return;
    const unplugged = event.data.error !== null;
    const others = meters.entries.filter((e) => e.listing.id !== id && e.session !== session);
    // An unplugged meter gives the view to a running one and waits in its tile; one the
    // user disconnected gives it to any other meter, which may be waiting itself.
    const next = others.find((e) => e.session.active) ?? (unplugged ? undefined : others[0]);
    if (!next) return;
    if (!unplugged) {
      // It leaves no tile behind: what it still has to say goes to the meter shown next.
      meters.notices.move(id, next.listing.id);
      meters.remove(id);
    }
    showInFullView(next, unplugged);
  }

  /** Moves meter `entry` into the full single-meter view: the meter shown so far keeps
   * its own store as a tile (unless `keepShown` is false, when it ended), and the chart,
   * statistics, recording and PD views follow the meter shown. */
  function showInFullView(entry: MeterEntry, keepShown: boolean): void {
    // Armed and the cable are per meter: the panel follows the meter on screen. It also
    // drops its pending dialog and stops a running Detect, so nothing later goes to the
    // other meter.
    void trigger.meterChanged();
    if (entry.session === session) return;
    const shownId = meters.fullViewId;
    if (keepShown && shownId !== null) {
      const away = new Session();
      copyLive(session, away);
      away.selectedId = shownId;
      const rec = recording.detach();
      if (rec) meters.park(shownId, rec);
      // The PD view shows one meter's capture: the meter leaving it keeps its own
      // running, unless it left because it ended (an unplug ended its capture).
      if (pd.capturing && session.active) {
        const parked = new PdCapture();
        parked.swapWith(pd);
        meters.parkPd(shownId, parked);
      }
      meters.replace(shownId, away);
      // An expanded meter that leaves the view goes back to the grid.
      if (meters.expandedId === shownId) meters.collapse();
    }
    const id = entry.listing.id;
    meters.fullViewId = id;
    copyLive(entry.session, session);
    session.selectedId = id;
    meters.replace(id, session);
    const rec = meters.unpark(id);
    if (rec) recording.adopt(rec);
    const parkedPd = meters.unparkPd(id);
    if (parkedPd) {
      pd.swapWith(parkedPd);
    } else {
      if (entry.session.details !== null) {
        pd.handleEvent({ type: "status", data: { state: "connected", ...entry.session.details } });
      }
      pd.capturing = false;
    }
    chart?.reset();
    chart?.backfillFromHistory();
  }

  /** Names the meter the top bar's controls act on. */
  const topBarControlsLabel = $derived.by(() => {
    const id = meters.fullViewId ?? session.selectedId;
    if (id === null) return null;
    const listing = meters.get(id)?.listing ?? session.devices.find((d) => d.id === id);
    if (!listing) return null;
    const masked = { hidden: hideSerial, position: meters.position(id) };
    return controlsLabel(tileTitle(listing, session.details?.serial ?? null, masked));
  });

  /** A tile's recording and export: the full view's meter keeps the app's. */
  function recordingOf(entry: MeterEntry): Recording {
    return entry.session === session ? recording : entry.recording;
  }
  function exportJobOf(entry: MeterEntry): ExportJob {
    return entry.session === session ? exportJob : entry.exportJob;
  }

  /** Shows `err` with meter `id`'s notices. */
  function raiseError(id: string, err: unknown): void {
    const { message, hint } = toUiError(err);
    meters.notices.raise(id, { tone: "error", message, text: hint ?? "", path: null });
  }

  /** The rate picker of a tile: acts on that meter only. */
  function selectMeterRate(entry: MeterEntry, sps: number): void {
    if (meterHeld) return;
    const refused = (err: unknown): void => raiseError(entry.listing.id, err);
    if (entry.session === session) {
      void selectRate(sps, refused);
      return;
    }
    void commandsOf(entry.listing.id).setRate(sps).catch(refused);
  }

  /** What the recording of a meter outside the full view needs from the app. */
  function meterRecordingDeps(entry: MeterEntry): RecordingDeps {
    const commands = commandsOf(entry.listing.id);
    return {
      canRecord: entry.session.meterConnected,
      startRecording: (name) => commands.startRecording(name, startOptions()),
      prefix: recorderSettings.prefix,
      stopRecording: () => commands.stopRecording(),
      now: () => new Date(),
    };
  }

  /** Record on a tile: starts or stops that meter's recording only. */
  function toggleMeterRecording(entry: MeterEntry): void {
    if (entry.session === session) {
      toggleRecording();
      return;
    }
    void entry.recording.toggle({
      ...meterRecordingDeps(entry),
      ...saveAskedAtStop(commandsOf(entry.listing.id)),
    });
  }

  /** Pause on a tile: pauses or resumes that meter's recording only. */
  function toggleMeterPause(entry: MeterEntry): void {
    if (entry.session === session) {
      togglePause();
      return;
    }
    void entry.recording.togglePause(commandsOf(entry.listing.id));
  }

  /** Runs an export; how it ended goes to meter `id`'s notices, wherever that meter is by
   * then. */
  async function runExport(id: string, job: ExportJob, deps: ExportDeps): Promise<void> {
    await job.run(deps);
    const { path, rows, folded, error } = job;
    if (path !== null && rows !== null) {
      const text = exportNoticeText({ rows, path, folded });
      meters.notices.raise(id, { tone: "info", text, path });
      job.dismissNotice();
    }
    if (error !== null) {
      raiseError(id, error);
      job.error = null;
    }
  }

  /** What an export writes: `name` is the CSV name proposed for it. */
  type ExportFormat = (
    commands: MeterCommands,
    name: string,
    onProgress: (progress: ExportProgress) => void,
  ) => Promise<ExportResult | null>;
  const toCsv: ExportFormat = (commands, name, onProgress) =>
    commands.exportSession(name, onProgress);
  /** A vendor-layout database file. */
  const toDb: ExportFormat = (commands, name, onProgress) =>
    commands.exportSessionDb(dbFileName(name), onProgress);

  /** Export on a tile: that meter's session so far. */
  function exportMeter(entry: MeterEntry, format: ExportFormat): void {
    if (entry.session === session) {
      exportHistory(format);
      return;
    }
    const commands = commandsOf(entry.listing.id);
    void runExport(entry.listing.id, entry.exportJob, {
      exportSession: (name, onProgress) => format(commands, name, onProgress),
      now: () => new Date(),
    });
  }

  /** "Expand" on a tile: that meter in the full single-meter view. */
  function expandMeter(id: string): void {
    const entry = meters.get(id);
    if (!entry) return;
    showInFullView(entry, true);
    meters.expand(id);
  }

  /** "Disconnect" on a tile: ends that meter's session only. A tile whose meter is
   * unplugged (or whose session failed) stops waiting for it and leaves the grid. */
  function disconnectMeter(id: string): void {
    if (meterHeld) return;
    const entry = meters.get(id);
    if (entry && !entry.session.active) {
      void commandsOf(id)
        .cancelWait()
        .catch((err: unknown) => session.reportError(toUiError(err)));
      if (meters.inFullView(id)) {
        meters.fullViewId = null;
        session.waitingSerial = null;
        if (session.state === "waiting") session.state = "disconnected";
      }
      meters.remove(id);
      return;
    }
    void commandsOf(id)
      .disconnect()
      .catch((err: unknown) => {
        if (entry && !meters.inFullView(id)) raiseError(id, err);
        else session.reportError(toUiError(err));
      });
  }

  let addMeterOpen = $state(false);

  /** Enter and Space press a control of the tab strip, as on a button. */
  function pressOnKey(e: KeyboardEvent, press: () => void): void {
    if (e.key !== "Enter" && e.key !== " ") return;
    // Space must not also start a recording.
    e.preventDefault();
    e.stopPropagation();
    press();
  }

  /** Connects one more meter beside those connected, with a session store of its own. */
  async function addMeter(id: string): Promise<void> {
    addMeterOpen = false;
    const listing = session.devices.find((d) => d.id === id);
    if (!listing) return;
    // A firmware update took the only meter: this one takes the empty full view.
    if (meters.fullViewId === null) {
      session.selectedId = id;
      await connectSelected();
      return;
    }
    const added = meters.add(listing);
    added.session.beginConnect();
    try {
      await connect(id, requestedRateSps, (e) => routeEvent(id, e), requestedSource);
    } catch (err) {
      meters.remove(id);
      session.reportError(toUiError(err));
    }
  }
  /** The meter whose notices the full view shows: it stays named once its session ended. */
  const noticesId = $derived(meters.fullViewId ?? session.selectedId);
  const noticesShown = $derived(noticesId === null ? [] : meters.notices.of(noticesId));
  function dismissShownNotice(notice: MeterNotice): void {
    if (noticesId !== null) meters.notices.dismiss(noticesId, notice);
  }
  /** Dismisses every information notice the full view shows; errors stay. */
  function dismissInfoNotices(): void {
    noticesShown.filter((n) => n.tone === "info").forEach(dismissShownNotice);
  }
  /** The meter a start or stop of the full view's recording began on. */
  let recordingActionId: string | null = null;
  const recording = new Recording((notice) => {
    const id = recordingActionId ?? noticesId;
    if (id !== null) meters.notices.setRecording(id, notice);
  });
  /** Runs a start or stop of the full view's recording: what it goes on to say stays with
   * the meter it began on, whichever one the view shows by then. */
  async function onShownMeter<T>(action: () => Promise<T>): Promise<T> {
    if (recordingActionId !== null) return action();
    recordingActionId = noticesId;
    try {
      return await action();
    } finally {
      recordingActionId = null;
    }
  }
  /** A stream is running that can be recorded. */
  const canRecord = $derived(session.state === "streaming" || session.state === "rate_locked");
  const exportJob = new ExportJob();
  /** The Meter settings panel (FR 1HHT01): read on connect, cleared on disconnect;
   * Edit changes the writable fields (FR JKWT3W). */
  const settingsPanel = new MeterSettingsPanel(
    () => act().meterSettings(),
    (changes) => act().meterSettingsWrite(changes),
  );
  /** The Records view (FR 1HHT00): the offline records, listed on connect. */
  const recordsPanel = new RecordsPanel(
    () => act().recordsList(),
    (index) => act().recordDownload(index),
    (target, confirm) => act().recordsDelete(target, confirm),
  );
  const meterConnected = $derived(session.meterConnected);
  /** The meter the panels read from: they read again when another one takes the view. */
  const panelMeter = $derived(meterConnected ? primaryId : null);
  $effect(() => {
    const on = panelMeter !== null;
    untrack(() => {
      if (on) {
        void settingsPanel.connected();
        void recordsPanel.connected();
      } else {
        settingsPanel.disconnected();
        recordsPanel.disconnected();
      }
    });
  });
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
  /** The recording file in the File tab (FR AXHK2S). */
  const recordingFile = new RecordingFile();
  /** The view the tabs show. */
  let view: "meter" | "pd" | "trigger" | "records" | "logo" | "firmware" | "file" = $state("meter");
  /** The grid shows, so each tile carries its meter's controls (AC-4PJRVM.1). */
  const controlsInTiles = $derived(view === "meter" && meters.showGrid);
  const ALL_VIEWS = ["meter", "pd", "records", "logo", "trigger", "firmware", "file"] as const;
  const ALL_VIEW_TABS = [
    { id: "meter", label: "Meter" },
    { id: "pd", label: "PD analyzer" },
    { id: "records", label: "Records" },
    { id: "logo", label: "Startup logo" },
    { id: "trigger", label: "Trigger" },
    { id: "firmware", label: "Firmware" },
    { id: "file", label: "File" },
  ] as const;
  /** The Advanced firmware switch shows the Firmware tab (FR V9SK55, AC .1). */
  let advancedFirmware = $state(loadAdvancedFirmware(browserStorage()));
  /** The File tab exists only while a file is open (FR AXHK2S). */
  function tabShown(id: string): boolean {
    if (id === "firmware") return advancedFirmware;
    // The Startup logo tab is for a connected meter (FR MEHQC0).
    if (id === "logo") return meterConnected;
    return id !== "file" || recordingFile.summary !== null;
  }
  const VIEWS = $derived(ALL_VIEWS.filter(tabShown));
  const VIEW_TABS = $derived(ALL_VIEW_TABS.filter((t) => tabShown(t.id)));
  function setAdvancedFirmware(on: boolean): void {
    // Progress is never hidden: the switch stays on while an update runs.
    if (!on && firmware.busy) return;
    advancedFirmware = on;
    saveAdvancedFirmware(on, browserStorage());
    if (!on && view === "firmware") view = "meter";
  }
  /** The meter a logo write goes to: the one the full view shows. Named when the write
   * starts, so the write keeps going to it whatever the view or meter list does next. */
  function logoMeterId(): string {
    const id = primaryId ?? session.selectedId;
    if (id === null) throw toUiError({ kind: "usage", message: "No meter is connected." });
    return id;
  }
  /** The Startup logo view's state (FR MEHQC0); the preview needs no meter. The write
   * runs in this store, not in the view, so leaving the view does not stop it. */
  const startupLogo = new StartupLogoState(
    {
      choose: () => logoChoose(),
      preview: (path, alpha) => logoPreview(path, alpha),
      read: (full) => logoRead(logoMeterId(), full),
      upload: (path, alpha, onProgress) => logoUpload(logoMeterId(), path, alpha, onProgress),
      previewBuiltin: (name) => logoPreviewBuiltin(name),
      uploadBuiltin: (name, onProgress) => logoUploadBuiltin(logoMeterId(), name, onProgress),
      remove: (onProgress) => logoRemove(logoMeterId(), onProgress),
      writeSettings: async (changes) => {
        // The block as read back, shown by the settings panel too, as its Apply does.
        const after = await act().meterSettingsWrite(changes);
        settingsPanel.settings = after;
        return after;
      },
      checkSettings: () => logoSettingsCheck(logoMeterId()),
      refreshSettings: () => settingsPanel.refresh(),
    },
    () => ({ model: session.details?.model ?? null, settings: settingsPanel.settings }),
  );
  $effect(() => {
    if (view === "logo" && !meterConnected) untrack(() => (view = "meter"));
  });
  /** The settings check after a logo write stops when the meter goes away (FR QG6RNY). */
  $effect(() => {
    if (!meterConnected) untrack(() => startupLogo.stopChecking());
  });
  /** A logo write holds every meter (AC-MEHQC0.3): nothing else is sent to one until it
   * ends. The controls that send are disabled, and the PD analyzer, Trigger and Records
   * tabs are held as during a firmware update. The settings check after it (FR QG6RNY)
   * holds only the logo writes, not the meter. */
  const meterHeld = $derived(startupLogo.phase === "writing");
  /** The recorder's options dialog (FR AXHK2R). */
  let recorderSettings = $state(loadRecorderSettings(browserStorage()));
  let recorderOptionsOpen = $state(false);
  /** The tile whose Options opened the dialog; null for the top bar's. */
  let recorderOptionsEntry = $state<MeterEntry | null>(null);
  function openRecorderOptions(entry: MeterEntry | null): void {
    recorderOptionsEntry = entry;
    recorderOptionsOpen = true;
  }
  /** The options hold for the recording they started with: they lock while any meter
   * records. */
  const recorderLocked = $derived(
    recording.phase !== "idle" || meters.entries.some((e) => e.recording.phase !== "idle"),
  );

  /** With a folder set the file is already in place, so Stop asks nothing. The options
   * lock while any meter records: this folder is the one the recording started with. */
  function saveAskedAtStop(commands: MeterCommands): Pick<RecordingDeps, "saveRecordingAs"> {
    return recorderSettings.folder
      ? {}
      : { saveRecordingAs: (path) => commands.saveRecordingAs(path) };
  }

  /** What a recording starts with; nothing while the settings ask for nothing. */
  function startOptions(): RecordingOptions | undefined {
    const options = recordingOptions(recorderSettings);
    return Object.keys(options).length > 0 ? options : undefined;
  }

  function saveRecorderOptions(settings: RecorderSettings): void {
    recorderSettings = settings;
    saveRecorderSettings(settings, browserStorage());
    recorderOptionsOpen = false;
  }

  /** The Hide serial numbers switch (FR AXHK2Q, AC .1). */
  let hideSerial = $state(loadHideSerial(browserStorage()));
  function setHideSerial(on: boolean): void {
    hideSerial = on;
    saveHideSerial(on, browserStorage());
  }
  /** The Show (i) icons switch (FR CW66T1, AC .8); InfoButton reads it from context. */
  let showInfoIcons = $state(loadShowInfoIcons(browserStorage()));
  function setShowInfoIcons(on: boolean): void {
    showInfoIcons = on;
    saveShowInfoIcons(on, browserStorage());
  }
  setContext("showInfoIcons", () => showInfoIcons);
  /** The firmware view's state (FR V9SK55), kept across tab switches. */
  const firmware = new FirmwareUpdate({
    choose: firmwareChoose,
    inspect: firmwareInspect,
    flash: firmwareFlash,
    cancelFlash: firmwareCancel,
    flashStatus: firmwareFlashStatus,
    // A plain connect of the meter's listing, back in application mode (AC-AXHK2M.15).
    // Run after the effect that asked for it, never inside it.
    reconnect: (id) =>
      Promise.resolve().then(async () => {
        const shown = meters.fullViewId;
        if (session.active && shown !== null && shown !== id) {
          if (meters.get(id) === undefined) await addMeter(id);
          return;
        }
        if (session.active) return;
        session.selectedId = id;
        await connectSelected();
      }),
    stopRecording: async (id) => {
      if (meters.inFullView(id)) {
        if (!(await onShownMeter(() => recording.stop(recordingDeps())))) throw recording.error;
        return;
      }
      const entry = meters.get(id);
      if (entry === undefined || entry.recording.phase === "idle") {
        await commandsOf(id).stopRecording();
        return;
      }
      if (!(await entry.recording.stop(meterRecordingDeps(entry)))) throw entry.recording.error;
    },
  });
  /** Another meter can be added while one streams or a firmware update holds one
   * (AC-4PJRVM.4). */
  const canAddMeter = $derived(firmware.busy || session.meterConnected);
  /** The devices "Add meter" offers: every connectable one not connected yet, and not the
   * one a firmware update holds. */
  const addableDevices = $derived(
    session.devices.filter(
      (d) => canConnectTo(d) && meters.get(d.id) === undefined && d.id !== firmware.updatingId,
    ),
  );
  /** While an update runs the meter is held for it (AC .4): no device switching, no
   * connect, and the PD analyzer, Trigger and Records tabs are disabled. A meter that runs
   * in the view beside the update is not held (AC-4PJRVM.4). */
  $effect(() => {
    session.flashing =
      firmware.busy && (!session.active || session.selectedId === firmware.updatingId);
  });
  function tabDisabled(id: string): boolean {
    return (firmware.busy || meterHeld) && (id === "pd" || id === "trigger" || id === "records");
  }
  function selectView(id: string): void {
    if (tabDisabled(id)) return;
    view = VIEWS.find((v) => v === id) ?? "meter";
  }
  /** The meter an update would go to: the picked device, with its versions while connected. */
  const firmwareMeter = $derived(
    flashMeter(
      session.devices.find((d) => d.id === session.selectedId),
      session.details,
    ),
  );
  /** Once an update has ended the updated meter stays picked as the OS lists it again,
   * and it is connected again once back in application mode (AC-AXHK2M.14, .15). The
   * pick moves only when the listing changes and no session runs: a meter the user
   * connected meanwhile keeps the picker. */
  let followedId: string | null = null;
  /** While an update runs, and after it ended until the meter is listed in application
   * mode, every device list it sees is noted for its "Device list" line (AC-XCCQAK.1,
   * AC-PGY704.1): when it starts and on each change of the list. */
  $effect(() => {
    const devices = session.devices;
    if (firmware.notingListings) untrack(() => firmware.noteListings(devices));
  });
  $effect(() => {
    const target = firmware.followTarget(session.devices);
    untrack(() => {
      if (target !== null && target !== followedId && !session.active) {
        session.selectedId = target;
      }
      followedId = target;
    });
  });
  /** The trigger view's state (FR DGSQ6S). */
  const trigger = new TriggerPanel(
    {
      open: () => act().triggerOpen(),
      detect: () => act().triggerDetect(),
      list: (protocol) => act().triggerList(protocol),
      risk: (request) => act().triggerRisk(request),
      request: (request, confirmed, remember) => act().triggerRequest(request, confirmed, remember),
      armed: () => act().triggerArmed(),
      disarm: () => act().triggerDisarm(),
      chargerGone: () => act().triggerChargerGone(),
      pdEntered: () => act().triggerPdEntered(),
      backTo5v: (leaving) => act().triggerBackTo5v(leaving),
      close: () => act().triggerClose(),
      cableRead: () => act().triggerCableRead(),
      cableSeen: () => act().pdCableSeen(),
    },
    // Detect starts afresh: the console's dialog goes too.
    { onDetect: () => pdConsole.cancel() },
  );
  /** What the console sends can end "don't ask again" and enter or leave PD: the
   * panel's status follows it. */
  const refreshingArmed = <T,>(sent: Promise<T>): Promise<T> =>
    sent.finally(() => Promise.all([trigger.refreshArmed(), trigger.refreshPdEntered()]));
  /** The PD message builder and raw console on the trigger session (FR DGSQ6T). */
  const pdConsole = new PdConsole(
    {
      messageRisk: (message) => act().triggerMessageRisk(message),
      messageSend: (message, confirmed) =>
        refreshingArmed(act().triggerMessageSend(message, confirmed)),
      consoleRisk: (line) => act().triggerConsoleRisk(line),
      consoleSend: (line, confirmed) => refreshingArmed(act().triggerConsoleSend(line, confirmed)),
    },
    () => trigger.detecting,
  );
  // A closed trigger session takes the console's transcript, cable and error with it.
  $effect(() => {
    if (!trigger.sessionOpen) untrack(() => pdConsole.reset());
  });

  // Leaving the Trigger tab returns to 5 V and closes the trigger session.
  let lastView: (typeof ALL_VIEWS)[number] = "meter";
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
      selectView(id);
      return;
    }
    if (e.key === "ArrowRight") next = (i + 1) % VIEWS.length;
    else if (e.key === "ArrowLeft") next = (i + VIEWS.length - 1) % VIEWS.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = VIEWS.length - 1;
    if (next === null) return;
    e.preventDefault();
    if (tabDisabled(VIEWS[next] ?? "meter")) return;
    view = VIEWS[next] ?? "meter";
    document.getElementById(`tab-${view}`)?.focus();
  }

  /** Open recording: open dialog, then the chosen file in the File tab (FR AXHK2S). The
   * Trigger tab stays on screen: leaving it returns the meter to 5 V, and opening a file
   * never touches a meter. A running firmware update stays too: its progress is never
   * hidden. */
  async function openRecording(): Promise<void> {
    const opened = await recordingFile.open();
    if (opened && view !== "trigger" && !firmware.busy) view = "file";
  }

  async function closeRecording(): Promise<void> {
    if ((await recordingFile.close()) && view === "file") view = "meter";
  }

  /** A capture opened from a file leaves the view first: the live one starts empty. */
  function startPd(): void {
    const cleared = pd.opened ? clearPdNow() : Promise.resolve();
    void cleared
      .then(() =>
        act()
          .pdStart()
          .catch((err: unknown) => {
            pd.startRefused();
            throw err;
          }),
      )
      .catch((err: unknown) => session.reportError(toUiError(err)));
  }

  function stopPd(): void {
    void act()
      .pdStop()
      .catch((err: unknown) => session.reportError(toUiError(err)));
  }

  /** The table empties at once and the backend drops its records; rows that arrive
   * meanwhile wait for its answer, and those numbered below the index it answers (sent
   * before it cleared) are dropped. A failed clear keeps them all. */
  function clearPd(): void {
    void clearPdNow().catch((err: unknown) => session.reportError(toUiError(err)));
  }

  function clearPdNow(): Promise<void> {
    // `pd_clear` lets go of an opened file itself; a release beside it could win the
    // race and leave the clear, with no meter, nothing to do but fail.
    pdFileShown = false;
    const ticket = pd.beginClear();
    return act()
      .pdClear()
      .then(
        (cleared) => pd.endClear(cleared.next_index, ticket, cleared.status_floor_ms),
        (err: unknown) => {
          pd.endClear(0, ticket);
          throw err;
        },
      );
  }

  /** The connected meter's firmware warnings, worded (FR V9SK52, AC .4). */
  const firmwareWarnings = $derived.by(() => {
    const info = session.details;
    if (info === null) return [];
    return info.warnings.map((code) => firmwareWarningText(code, info));
  });

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
      const result = await act().pdSave(defaultPdCaptureName(new Date()));
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
  let pdImported = $state<PdOpenedNotice | null>(null);

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
        pd.open(opened.rows, opened.status, opened.dropped);
        // Only the counts stay for the notice; the rows live in the store.
        const notice = pdOpenedNotice(opened);
        pdImported = imported ? notice : null;
        pdOpenDamage = pdOpenDamageText(notice);
      }
    } catch (err) {
      pdSaveError = toUiError(err);
    } finally {
      pdOpening = false;
    }
  }

  /** The backend keeps an opened file until told (AC-AXHK2K.5): whatever takes it out of
   * the view (a new session, a clear, another meter's capture), the backend lets go too. */
  let pdFileShown = false;
  $effect(() => {
    const shown = pd.opened;
    if (pdFileShown && !shown) {
      void pdReleaseOpened().catch((err: unknown) => session.reportError(toUiError(err)));
    }
    pdFileShown = shown;
  });

  function loadPdDetails(index: number): Promise<PdDetails | null> {
    return act()
      .pdDetails(index, pd.opened)
      .catch((err: unknown) => {
        session.reportError(toUiError(err));
        return null;
      });
  }
  // Information notices leave by themselves; errors stay until dismissed (AC-AKNVND.8).
  const meterNoticeTimer = new AutoDismiss(dismissInfoNotices);
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
    // Only while it can be read: one raised behind another tab waits for the Meter tab.
    if (view === "meter" && noticesShown.some((n) => n.tone === "info")) {
      meterNoticeTimer.start();
    } else {
      meterNoticeTimer.stop();
    }
    return () => meterNoticeTimer.stop();
  });
  /** The pointer over the notices, or focus in them, holds the countdowns. */
  function holdNotices(reason: HoldReason, held: boolean): void {
    for (const t of [meterNoticeTimer, pdSaveNoticeTimer, pdImportNoticeTimer]) {
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
      await act().resetStats();
      session.statsReset();
    },
    clearHistory: () => act().clearHistory(),
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
  /** Where the next session's measurements come from (FR 1HHT02). */
  let requestedSource = $state<MeasurementSource>("queue");

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

  /** The meter a reload picked up, whose events route like a connected one's. */
  let attachedId: string | null = null;
  function routeAttached(event: BackendEvent): void {
    if (attachedId === null) onBackendEvent(event);
    else routeEvent(attachedId, event);
  }

  /** A picker entry for meter `snapshot` when the device list has none (yet). */
  function listingOf(snapshot: AttachedSession): DeviceListing {
    const id = snapshot.device_id;
    const listed = session.devices.find((d) => d.id === id);
    if (listed) return listed;
    const demo = id.startsWith("demo:") ? id.slice("demo:".length) : null;
    return {
      id,
      label: demo ? `Demo device — ${demo}` : snapshot.model,
      model: snapshot.model,
      serial: snapshot.serial,
      bus: null,
      address: null,
      demo_scenario: demo,
      mode: "application",
    };
  }

  /** After a reload, picks up every other running meter in a tile of its own (FR 1HHT03):
   * each one's events go to the new page, not to the old one's gone channel. */
  async function attachOtherMeters(): Promise<void> {
    let ids: string[];
    try {
      ids = await connectedMeters();
    } catch (err) {
      console.warn("open-powerz: listing the running meters failed", err);
      return;
    }
    // The tiles keep the order they had, whichever meter attaches first (AC-4PJRVM.3).
    meters.arrange(ids);
    // All at once: each meter's events go to the old page until it is attached.
    await Promise.all(
      ids
        .filter((id) => !meters.inFullView(id) && meters.get(id) === undefined)
        .map((id) =>
          attachSession(
            (onEvent) => attach(onEvent, id),
            (e) => routeEvent(id, e),
            (snapshot) => {
              const tile = new Session();
              tile.adopt(snapshot);
              meters.add(listingOf(snapshot), tile);
              if (snapshot.recording) meters.park(id, snapshot.recording);
              // The capture runs on without this page (AC-AXHK2K.7); a stopped one
              // still carries the status samples the backend dropped (AC-AXHK2W.6).
              if (snapshot.pd_capturing || (snapshot.pd_status_dropped ?? 0) > 0) {
                const parked = new PdCapture();
                parked.adopt(snapshot);
                meters.parkPd(id, parked);
              }
            },
          ),
        ),
    );
  }

  // On load, ask the backend whether a session is still running (a reload);
  // with none, the app starts disconnected as on a first launch (FR VEDDFD).
  onMount(() => {
    if (!backendAvailable()) return;
    // A reloaded page has no view of a file the last page left open.
    void recordingFile.close();
    void firmware.reattach().then(() => {
      if (firmware.busy) view = "firmware";
    });
    void attachSession(
      attach,
      (e) => routeAttached(e),
      (snapshot) => {
        attachedId = snapshot.device_id;
        attachedListing = listingOf(snapshot);
        meters.fullViewId = snapshot.device_id;
        session.adopt(snapshot);
        // The new page's buffer is empty: fill the live windows from the history.
        chart?.backfillFromHistory();
        if (snapshot.recording) recording.adopt(snapshot.recording);
        pd.adopt(snapshot);
        if (!snapshot.rate_locked && snapshot.rate_sps !== null) {
          requestedRateSps = snapshot.rate_sps;
        }
      },
    ).then(() => (attachedId === null ? undefined : attachOtherMeters()));
  });

  async function toggleConnection(): Promise<void> {
    // A second press while a connect is still pending would race it.
    if (!session.canToggleConnect || meterHeld) return;
    if (session.active) {
      if (disconnecting) return;
      disconnecting = true;
      // A refused disconnect leaves the stream running: show the error, keep the state.
      await act()
        .disconnect()
        .catch((err: unknown) => session.reportError(toUiError(err)))
        .finally(() => (disconnecting = false));
      return;
    }
    await connectSelected();
  }

  /** Connects the picked device into the full view. */
  async function connectSelected(): Promise<void> {
    const id = session.selectedId;
    if (id === null) return;
    session.beginConnect();
    meters.fullViewId = id;
    chart?.reset();
    try {
      await connect(id, requestedRateSps, (e) => routeEvent(id, e), requestedSource);
    } catch (err) {
      session.failConnect(toUiError(err));
      if (meters.fullViewId === id) meters.fullViewId = null;
    }
  }

  async function selectRate(
    sps: number,
    onFailed: (err: unknown) => void = (err) => session.reportError(toUiError(err)),
  ): Promise<void> {
    if (meterHeld) return;
    // The picker is disabled while recording; the keys 1–4 follow the same lock.
    // A meter that set its own rate stays locked after the recording too: the backend's
    // own refusal says so instead.
    const change = rateChange(sps, {
      recording: recording.phase !== "idle" && session.state !== "rate_locked",
    });
    if (change.kind === "refused") {
      if (noticesId !== null) {
        meters.notices.setRecording(noticesId, { tone: "info", text: change.notice, path: null });
      }
      return;
    }
    requestedRateSps = sps;
    if (session.state !== "streaming" && session.state !== "rate_locked") return;
    // A failed rate change leaves the stream running; just surface the error.
    await act().setRate(sps).catch(onFailed);
  }

  /** Export button: save dialog, then the session history so far, off the UI thread. */
  function exportHistory(format: ExportFormat): void {
    if (!canExport || noticesId === null) return;
    void runExport(noticesId, exportJob, {
      exportSession: (name, onProgress) => format(act(), name, onProgress),
      now: () => new Date(),
    });
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

  /** What the recording needs from the app, for the meter the full view shows. */
  function recordingDeps() {
    return {
      canRecord,
      startRecording: (name: string) => act().startRecording(name, startOptions()),
      prefix: recorderSettings.prefix,
      stopRecording: () => act().stopRecording(),
      now: () => new Date(),
    };
  }

  /** Record button / Space: start at once, or stop and ask where to save the CSV file. */
  function toggleRecording(): void {
    void onShownMeter(() =>
      recording.toggle({
        ...recordingDeps(),
        ...saveAskedAtStop(act()),
      }),
    );
  }

  /** Pause button: holds the rows back, or writes them again. */
  function togglePause(): void {
    void onShownMeter(() => recording.togglePause(act()));
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
      // In the grid no meter is the obvious one: its tiles' controls are named instead.
      case "record":
        if (!controlsInTiles) toggleRecording();
        break;
      case "connect":
        if (!controlsInTiles) void toggleConnection();
        break;
      case "reset_stats":
        resetStats();
        break;
      case "rate":
        if (!controlsInTiles) void selectRate(action.sps);
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
    {hideSerial}
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
    onTogglePause={togglePause}
    onRecorderOptions={() => openRecorderOptions(null)}
    {exportJob}
    {canExport}
    onExport={() => exportHistory(toCsv)}
    onExportDb={() => exportHistory(toDb)}
    controlsLabel={topBarControlsLabel}
    {controlsInTiles}
    held={meterHeld}
    rateSps={session.rateSps ?? requestedRateSps}
    source={requestedSource}
    onSelectSource={(source) => (requestedSource = source)}
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
            aria-disabled={tabDisabled(tab.id) ? "true" : undefined}
            tabindex={view === tab.id ? 0 : -1}
            onclick={() => selectView(tab.id)}
            onkeydown={onTabKeydown}
          >
            {tab.label}
          </div>
        {/each}
      </div>
      <!-- Not <button>, like the tabs: the top bar's one-row layout counts its buttons. -->
      <div
        role="button"
        class="strip-action open-recording"
        tabindex="0"
        onclick={() => void openRecording()}
        onkeydown={(e) => pressOnKey(e, () => void openRecording())}
      >
        Open recording
      </div>
      {#if canAddMeter}
        <div
          role="button"
          class="strip-action"
          tabindex="0"
          onclick={() => (addMeterOpen = true)}
          onkeydown={(e) => pressOnKey(e, () => (addMeterOpen = true))}
        >
          Add meter
        </div>
      {/if}
    {/snippet}
  </TopBar>
  <main class="views">
    <!-- Inside the views, under the room kept for the tabs that hang below the top bar. -->
    <ErrorBanner {session} />
    {#if recordingFile.error !== null}
      <Notice
        tone="error"
        message={recordingFile.error}
        onDismiss={() => (recordingFile.error = null)}
      />
    {/if}
    {#if recordingFile.working}
      {@const progress = recordingFile.progress}
      <div class="file-opening" role="status">
        <span>Opening recording</span>
        {#if progress !== null && progress.total > 0}
          <progress aria-label="Opening progress" max={progress.total} value={progress.done}
          ></progress>
        {:else}
          <!-- No value: a plain moving bar, for a total not known. -->
          <progress aria-label="Opening progress"></progress>
        {/if}
        <button type="button" onclick={() => void recordingFile.cancel()}>Cancel</button>
      </div>
    {/if}
    <div
      class="content"
      role="tabpanel"
      id="view-meter"
      aria-labelledby="tab-meter"
      hidden={view !== "meter" || meters.showGrid}
    >
      <div class="main-area">
        {#if meters.focused && meters.expandedId !== null}
          <!-- One tile of the grid expanded (AC-1HHT03.2): its readout, and the way back. -->
          <div class="expanded-meter">
            <button type="button" class="all-meters" onclick={() => meters.collapse()}>
              All meters
            </button>
            <Readout session={meters.focused.session} />
          </div>
        {:else}
          <Readout {session} />
        {/if}
        <section class="chart panel" aria-label="Chart">
          <Chart
            bind:this={chart}
            history={chartHistory}
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
        <MeterNoticeList notices={noticesShown} onDismiss={dismissShownNotice} {fileActions} />
      </div>
      <div class="side-panel">
        <!-- The firmware card stays for as long as the meter is connected, so it takes
           its own room at the top of the side panel instead of floating over the
           settings (AC-AXHK2V.9); stacked, it sits between the chart card and Stats. -->
        <FirmwareNotice warnings={firmwareWarnings} />
        <StatsPanel
          stats={session.stats}
          controls={statsControls}
          deps={statsDeps}
          canReset={canRecord}
        >
          <MeterSettingsView panel={settingsPanel} connected={meterConnected} held={meterHeld} />
        </StatsPanel>
      </div>
    </div>
    {#if view === "meter" && meters.showGrid}
      <MeterGrid
        {meters}
        {hideSerial}
        held={meterHeld}
        onExpand={expandMeter}
        onDisconnect={disconnectMeter}
        {recordingOf}
        {exportJobOf}
        defaultRateSps={requestedRateSps}
        onSelectRate={selectMeterRate}
        onToggleRecording={toggleMeterRecording}
        onTogglePause={toggleMeterPause}
        onRecorderOptions={openRecorderOptions}
        onExport={(entry) => exportMeter(entry, toCsv)}
        onExportDb={(entry) => exportMeter(entry, toDb)}
        {fileActions}
      />
    {/if}
    {#if view === "trigger"}
      <div class="trigger-view" role="tabpanel" id="view-trigger" aria-labelledby="tab-trigger">
        <TriggerView panel={trigger} connected={canRecord} {pdConsole} />
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
              hint={pdSaveError.hint}
              onDismiss={() => (pdSaveError = null)}
            />
          {/if}
          {#if pdOpenDamage !== null}
            <Notice tone="error" message={pdOpenDamage} onDismiss={() => (pdOpenDamage = null)} />
          {/if}
        </div>
      </div>
    {/if}
    {#if view === "records"}
      <div class="records-view" role="tabpanel" id="view-records" aria-labelledby="tab-records">
        <RecordsView panel={recordsPanel} connected={meterConnected} theme={resolvedTheme} />
      </div>
    {/if}
    {#if view === "file" && recordingFile.summary !== null}
      <div class="file-view" role="tabpanel" id="view-file" aria-labelledby="tab-file">
        <RecordingFileView
          summary={recordingFile.summary}
          theme={resolvedTheme}
          onClose={() => void closeRecording()}
          onError={(err) => session.reportError(toUiError(err))}
        />
      </div>
    {/if}
    {#if view === "logo" && meterConnected}
      <div class="logo-view" role="tabpanel" id="view-logo" aria-labelledby="tab-logo">
        <h2>Startup logo</h2>
        <StartupLogoView logo={startupLogo} />
      </div>
    {/if}
    {#if view === "firmware" && advancedFirmware}
      <div class="firmware-view" role="tabpanel" id="view-firmware" aria-labelledby="tab-firmware">
        <div class="titled">
          <h2>Firmware</h2>
          <InfoButton key="section.firmware" />
        </div>
        <FirmwareView {firmware} meter={firmwareMeter} {hideSerial} />
      </div>
    {/if}
  </main>
  {#if addMeterOpen}
    <AddMeterDialog
      devices={addableDevices}
      {hideSerial}
      onCancel={() => (addMeterOpen = false)}
      onConnect={(id) => void addMeter(id)}
    />
  {/if}
  {#if recorderOptionsOpen}
    <RecorderOptions
      settings={recorderSettings}
      locked={recorderLocked}
      rateSps={(recorderOptionsEntry?.session ?? session).rateSps}
      onCancel={() => (recorderOptionsOpen = false)}
      onSave={saveRecorderOptions}
    />
  {/if}
  {#if helpOpen}
    <ShortcutHelp onClose={() => (helpOpen = false)} />
  {/if}
  {#if aboutOpen}
    <About
      opener={aboutOpener}
      details={session.details}
      {advancedFirmware}
      advancedFirmwareLocked={firmware.busy}
      onAdvancedFirmware={setAdvancedFirmware}
      {hideSerial}
      onHideSerial={setHideSerial}
      {showInfoIcons}
      onShowInfoIcons={setShowInfoIcons}
      onClose={() => (aboutOpen = false)}
    />
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
  /* At the right end of the view tabs' strip. */
  .open-recording {
    margin-left: auto;
  }
  .strip-action {
    margin-bottom: 0.25rem;
    font-size: 0.875rem;
    padding: 0.2rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: var(--surface);
    cursor: pointer;
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
  /* A phone-wide window (FR EWQ6FQ): the strip's row is too narrow for the tabs and the
     buttons, so the tabs take the first line whole and Open recording and Add meter move
     to their own line under them. Nothing scrolls sideways; the views keep room for the
     two lines. */
  @media (max-width: 640px) {
    .views {
      padding-top: 4.75rem;
    }
    .view-tabs {
      flex: 1 0 100%;
      flex-wrap: wrap;
      min-width: 0;
    }
    .view-tabs [role="tab"] {
      flex: none;
      white-space: nowrap;
      padding: 0.3rem 0.6rem;
    }
    .strip-action {
      flex: none;
      white-space: nowrap;
    }
  }
  .view-tabs [role="tab"][aria-disabled="true"] {
    cursor: not-allowed;
    opacity: 0.6;
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
  .firmware-view,
  .logo-view,
  .records-view,
  .file-view {
    padding: 1rem;
  }
  .file-opening {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin: 0.75rem 1rem 0;
    font-size: 0.875rem;
  }
  .file-opening progress {
    flex: 1;
    min-width: 0;
    max-width: 20rem;
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
  .expanded-meter {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.5rem;
    min-width: 0;
  }
  .expanded-meter > :global(*:last-child) {
    align-self: stretch;
  }
  .main-area {
    grid-area: main;
    display: grid;
    grid-template-rows: auto minmax(16rem, 1fr);
    gap: 1rem;
    min-width: 0;
  }
  /* Wide, the side card never sets the row height: it gets the main area's height and
     scrolls inside it when the Meter settings make it taller. */
  .side-panel {
    grid-area: side;
    min-width: 0;
    contain: size;
    display: flex;
    flex-direction: column;
  }
  .side-panel > :global(.stats) {
    min-height: 0;
    overflow-y: auto;
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
      contain: none;
    }
    /* With nothing to show it stays fixed, so the page keeps no gap for it. */
    .notice-stack:not(.pd-notices):has(> :global(:not(:empty))) {
      position: static;
      width: auto;
      max-width: none;
      max-height: none;
      overflow: visible;
    }
  }
</style>
