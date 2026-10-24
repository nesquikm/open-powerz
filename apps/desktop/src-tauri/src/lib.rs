pub mod activity;
pub mod batch;
pub mod devices;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod history;
pub mod pd_capture;
pub mod pd_import;
pub mod recording;
pub mod stats;
pub mod store;
pub mod trigger;
pub mod ui_error;
pub mod worker;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use powerz_protocol::session::SystemClock;
use powerz_protocol::transport::discovery;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

use crate::devices::{AppOpener, AttachedSession, DeviceListing, DeviceManager, WatchHandle};
use crate::recording::{ExportProgress, RecordingSummary};
use crate::ui_error::UiError;
use crate::worker::BackendEvent;

/// The app and protocol-library versions shown in the About dialog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AppInfo {
    pub app_version: String,
    pub protocol_version: String,
}

/// Returns the app and protocol-library version for the About screen.
#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol_version: powerz_protocol::VERSION.to_owned(),
    }
}

type AppManager = DeviceManager<AppOpener, SystemClock>;

/// The app's device manager and hotplug watcher, shared by the commands.
/// Commands hold the locks only to take what they need; every blocking wait
/// (a worker thread joining, a rate change answer, USB enumeration) runs on
/// a blocking thread, never on the async runtime.
struct AppState {
    manager: Mutex<AppManager>,
    watch: Mutex<Option<WatchHandle>>,
    /// The PD capture opened from a file (FR TGE83E): outside the
    /// per-connection worker, so it opens without a meter and never mixes
    /// with a live capture.
    opened_pd: Mutex<Option<pd_capture::PdCapture>>,
}

impl AppState {
    fn new() -> AppState {
        AppState::with_manager(DeviceManager::new(
            discovery::list_devices,
            AppOpener,
            SystemClock,
        ))
    }

    fn with_manager(manager: AppManager) -> AppState {
        AppState {
            manager: Mutex::new(manager),
            watch: Mutex::new(None),
            opened_pd: Mutex::new(None),
        }
    }

    /// Connect to `device_id` at `rate_sps`; events go to `sink`. The UI
    /// drops a capture opened from a file when a session connects, so its
    /// memory is released here too.
    fn connect_session(
        &self,
        device_id: &str,
        rate_sps: u32,
        sink: impl worker::EventSink,
        stop: Arc<AtomicBool>,
    ) -> Result<(), UiError> {
        self.manager()
            .connect_with_stop(device_id, rate_sps, sink, stop)?;
        *self.opened_pd() = None;
        Ok(())
    }

    fn opened_pd(&self) -> MutexGuard<'_, Option<pd_capture::PdCapture>> {
        self.opened_pd
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Put `pd`, opened from the file at `path`, in the opened-capture slot
    /// and answer its rows and status samples for the UI.
    fn show_opened(
        &self,
        path: &Path,
        mut pd: pd_capture::PdCapture,
        damaged_line: Option<usize>,
        left_out: LeftOut,
    ) -> PdOpened {
        let opened = PdOpened {
            path: path.to_string_lossy().into_owned(),
            rows: pd.take_all().unwrap_or_default(),
            status: pd.take_all_status().unwrap_or_default(),
            damaged_line,
            oversized: left_out.oversized,
            truncated: left_out.truncated,
        };
        *self.opened_pd() = Some(pd);
        opened
    }

    fn manager(&self) -> MutexGuard<'_, AppManager> {
        self.manager.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Quit or main-window close (FR X2SPDW): stop the queue, release the
    /// interface and end the hotplug watcher. Safe to call more than once.
    fn shutdown(&self) {
        self.manager().shutdown();
        let watch = self
            .watch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(watch);
    }
}

/// Shut the app state down, if it is managed.
fn shutdown_app<R: tauri::Runtime>(app: &impl tauri::Manager<R>) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        state.shutdown();
    }
}

type Shared<'a> = tauri::State<'a, Arc<AppState>>;

/// Run `work` on a blocking thread and hand back its result.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, UiError> + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| UiError::internal())?
}

/// Every attached meter plus the demo device entries; never opens a device.
#[tauri::command]
async fn list_devices(state: Shared<'_>) -> Result<Vec<DeviceListing>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || Ok(state.manager().list_devices())).await
}

/// Send the device list over `on_change` now and on every plug or unplug.
#[tauri::command]
async fn watch_devices(
    state: Shared<'_>,
    on_change: Channel<Vec<DeviceListing>>,
) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let stop = Arc::new(AtomicBool::new(false));
        let gone = Arc::clone(&stop);
        // Weak: the state owns the watcher, which must not keep it alive.
        let weak = Arc::downgrade(&state);
        let handle = state.manager().watch_devices_with_stop(
            move |list| {
                // A meter the app waits for that is listed again reconnects
                // (FR TGE839).
                if let Some(state) = weak.upgrade() {
                    state.manager().devices_changed(&list);
                }
                if on_change.send(list).is_err() {
                    gone.store(true, Ordering::SeqCst);
                }
            },
            stop,
        );
        let old = state
            .watch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(handle);
        if let Some(old) = old {
            old.stop();
        }
        Ok(())
    })
    .await
}

/// Connect to the picked device; every session event goes over `on_event`.
/// The session ends by itself once that channel is gone (window closed);
/// a reloaded page takes the session over with `attach` instead.
#[tauri::command]
async fn connect(
    state: Shared<'_>,
    device_id: String,
    rate_sps: u32,
    on_event: Channel<BackendEvent>,
) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let stop = Arc::new(AtomicBool::new(false));
        let sink = channel_sink(on_event, Arc::clone(&stop));
        state.connect_session(&device_id, rate_sps, sink, stop)
    })
    .await
}

/// Pick up the running session from a new page (after a reload): its later
/// events go over `on_event`. `null` when no session is running.
#[tauri::command]
async fn attach(
    state: Shared<'_>,
    on_event: Channel<BackendEvent>,
) -> Result<Option<AttachedSession>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // A reloaded page (e.g. after the web content process ended) starts
        // without a trigger session, so one the old page left open returns
        // to 5 V and closes now, outside the manager lock (FR DGSQ6S).
        let leftover = state.manager().take_trigger();
        if let Some(trigger) = leftover {
            let _ = trigger.close();
        }
        // Taken under the lock; the handover waits for the worker without it.
        let Some(attacher) = state.manager().attacher() else {
            return Ok(None);
        };
        // The new page's channel ends the session once it is gone, as the
        // connecting page's did.
        let sink = channel_sink(on_event, attacher.stop());
        Ok(attacher.attach(sink))
    })
    .await
}

/// Stop the queue and close the open session.
#[tauri::command]
async fn disconnect(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // Take the sessions under the lock; wait for them without it.
        let (trigger, session) = {
            let mut manager = state.manager();
            (manager.take_trigger(), manager.take_session())
        };
        // Back to 5 V and `pdm close` before the queue stops (FR DGSQ6S).
        if let Some(trigger) = trigger {
            let _ = trigger.close();
        }
        session?.disconnect();
        Ok(())
    })
    .await
}

/// Stop waiting for an unplugged meter; the user picked another device.
#[tauri::command]
async fn cancel_wait(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        state.manager().cancel_wait();
        Ok(())
    })
    .await
}

/// Restart the open session's queue at `rate_sps`.
#[tauri::command]
async fn set_rate(state: Shared<'_>, rate_sps: u32) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // Take the control under the lock; wait for the answer without it.
        let (control, rate) = state.manager().rate_control(rate_sps)?;
        control.set_rate(rate)
    })
    .await
}

/// Start the PD capture on the open session (FR TGE83C); rows and the
/// capture state arrive on the session channel.
#[tauri::command]
async fn pd_start(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = {
            let manager = state.manager();
            // The PD capture waits while triggering (FR DGSQ6S). [inferred]
            if manager.trigger_open() {
                return Err(UiError::usage(
                    "Close the trigger session to capture PD traffic.",
                ));
            }
            manager.control()?
        };
        control.start_pd()
    })
    .await
}

/// Open the trigger session on the connected meter (FR DGSQ6S).
#[tauri::command]
async fn trigger_open(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // The port opens and the PDM handshake runs without the manager
        // lock, so a slow meter never blocks disconnect or attach.
        let opener = state.manager().trigger_opener()?;
        let worker = opener.open()?;
        state.manager().install_trigger(worker)
    })
    .await
}

/// BC 1.2, Apple and the automatic list; nothing above 5 V is asked.
#[tauri::command]
async fn trigger_detect(state: Shared<'_>) -> Result<trigger::Detection, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // The handle is taken off the manager, so its lock is not held while
        // the session thread answers.
        let control = state.manager().trigger_control()?;
        control.detect()
    })
    .await
}

/// One protocol's section; a protocol the charger lacks is `supported: false`.
#[tauri::command]
async fn trigger_list(
    state: Shared<'_>,
    protocol: trigger::TriggerProtocol,
) -> Result<trigger::ProtocolSection, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.list(protocol)
    })
    .await
}

/// The library risk class of a request; `above_5v` needs the user's
/// confirmation.
#[tauri::command]
async fn trigger_risk(
    state: Shared<'_>,
    request: trigger::TriggerRequest,
) -> Result<trigger::RiskClass, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.risk(request)
    })
    .await
}

/// Send a request; above 5 V it is refused unless `confirmed` (the backend
/// is the final gate).
#[tauri::command]
async fn trigger_request(
    state: Shared<'_>,
    request: trigger::TriggerRequest,
    confirmed: bool,
) -> Result<trigger::TriggerReply, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.request(request, confirmed)
    })
    .await
}

/// The library risk class of a built PD message (FR DGSQ6T); GoodCRC and
/// malformed messages are a usage error with the reason.
#[tauri::command]
async fn trigger_message_risk(
    state: Shared<'_>,
    message: trigger::PdMessageRequest,
) -> Result<trigger::RiskClass, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.message_risk(message)
    })
    .await
}

/// Send a built PD message; above 5 V it is refused unless `confirmed`.
#[tauri::command]
async fn trigger_message_send(
    state: Shared<'_>,
    message: trigger::PdMessageRequest,
    confirmed: bool,
) -> Result<trigger::PdSendReply, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.send_message(message, confirmed)
    })
    .await
}

/// The library risk class of a raw console line (FR DGSQ6T).
#[tauri::command]
async fn trigger_console_risk(
    state: Shared<'_>,
    line: String,
) -> Result<trigger::RiskClass, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.console_risk(line)
    })
    .await
}

/// Send a raw console line; above 5 V (any unparsable line too) it is
/// refused unless `confirmed`; GoodCRC is always refused.
#[tauri::command]
async fn trigger_console_send(
    state: Shared<'_>,
    line: String,
    confirmed: bool,
) -> Result<Vec<trigger::ConsoleLine>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.console_send(line, confirmed)
    })
    .await
}

/// Back to 5 V: PD `pd req=1` (Hard Reset as the fallback), the other
/// protocols `reset`. The session stays open.
#[tauri::command]
async fn trigger_back_to_5v(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().trigger_control()?;
        control.back_to_5v()
    })
    .await
}

/// Back to 5 V and close the trigger session, if one is open.
#[tauri::command]
async fn trigger_close(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // Take the session under the lock; close it without it.
        let trigger = state.manager().take_trigger();
        trigger.map_or(Ok(()), trigger::TriggerWorker::close)
    })
    .await
}

/// Stop the PD capture; the captured records stay.
#[tauri::command]
async fn pd_stop(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().control()?;
        control.stop_pd()
    })
    .await
}

/// The raw bytes and field tree of PD record `index`: of the capture
/// opened from a file when `opened` is true, of the live capture otherwise.
#[tauri::command]
async fn pd_details(
    state: Shared<'_>,
    index: u64,
    opened: Option<bool>,
) -> Result<pd_capture::PdDetails, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        if opened == Some(true) {
            return state
                .opened_pd()
                .as_ref()
                .and_then(|pd| pd.details(index))
                .ok_or_else(|| UiError::usage("That PD record is not in the opened capture."));
        }
        let control = state.manager().control()?;
        control.pd_details(index)
    })
    .await
}

/// Drop every captured PD record; the answer is the index of the next one.
/// A capture opened from a file is dropped too; with only that one and no
/// meter, the answer is 0.
#[tauri::command]
async fn pd_clear(state: Shared<'_>) -> Result<u64, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let had_opened = state.opened_pd().take().is_some();
        let control = match state.manager().control() {
            Ok(control) => control,
            Err(_) if had_opened => return Ok(0),
            Err(e) => return Err(e),
        };
        control.clear_pd()
    })
    .await
}

/// A PD capture opened from a pdcap file (FR TGE83E): its rows and status
/// samples, numbered from 0.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PdOpened {
    /// The file opened.
    pub path: String,
    /// Every row of the capture.
    pub rows: Vec<pd_capture::PdRow>,
    /// Every status sample of the capture.
    pub status: Vec<pd_capture::PdStatusSample>,
    /// The 1-based line where reading stopped when the file is truncated or
    /// corrupt (AC-TGE83E.3); every valid line before it was loaded.
    pub damaged_line: Option<usize>,
    /// Vendor log rows skipped because their record bytes are oversized.
    pub oversized: u64,
    /// The vendor log has more rows than the import takes; only the first
    /// ones were imported.
    pub truncated: bool,
}

/// What an import left out of the opened capture (none for a pdcap file).
#[derive(Debug, Clone, Copy, Default)]
struct LeftOut {
    oversized: u64,
    truncated: bool,
}

/// Ask which file of type `name` (`extensions`) to open; `None` when the
/// dialog is cancelled. Blocks: call it on a blocking thread.
fn choose_open_path(
    app: &tauri::AppHandle,
    name: &str,
    extensions: &[&str],
) -> Result<Option<PathBuf>, UiError> {
    let Some(choice) = app
        .dialog()
        .file()
        .add_filter(name, extensions)
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    choice.into_path().map(Some).map_err(|_| {
        UiError::usage(
            "The open dialog did not return a file on disk; pick a file on this computer.",
        )
    })
}

/// Open a pdcap file into the PD analyzer (FR TGE83E); needs no meter.
/// Resolves to its rows and status samples, or `null` when the dialog was
/// cancelled. Record details then come from `pd_details` with `opened`.
#[tauri::command]
async fn pd_open(app: tauri::AppHandle, state: Shared<'_>) -> Result<Option<PdOpened>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let Some(path) = choose_open_path(&app, "PD capture", &["pdcap"])? else {
            return Ok(None);
        };
        let file = std::fs::File::open(&path)
            .map_err(|e| UiError::usage(format!("Could not open the PD capture: {e}")))?;
        let loaded = powerz_protocol::pdcap::load(std::io::BufReader::new(file));
        if let (Some(e), true) = (&loaded.error, loaded.entries.is_empty()) {
            return Err(UiError::usage(format!(
                "Could not open the PD capture: {e}"
            )));
        }
        let pd = pd_capture::PdCapture::from_pdcap(&loaded.entries);
        let damaged_line = loaded.error.as_ref().map(|e| e.line());
        Ok(Some(state.show_opened(
            &path,
            pd,
            damaged_line,
            LeftOut::default(),
        )))
    })
    .await
}

/// Import a vendor PD log database into the PD analyzer, read-only (FR
/// TGE83E, AC .6); needs no meter. Resolves like `pd_open`: the capture lands
/// in the opened-capture slot, or `null` when the dialog was cancelled.
#[tauri::command]
async fn pd_import(app: tauri::AppHandle, state: Shared<'_>) -> Result<Option<PdOpened>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let Some(path) = choose_open_path(&app, "Vendor PD log", &["sqlite"])? else {
            return Ok(None);
        };
        let imported =
            pd_import::import_vendor_pd_log_with(&path, &pd_import::ImportLimits::DEFAULT)
                .map_err(|e| UiError::usage(format!("Could not import the vendor PD log: {e}")))?;
        let left_out = LeftOut {
            oversized: imported.oversized,
            truncated: imported.truncated,
        };
        Ok(Some(state.show_opened(
            &path,
            imported.capture,
            None,
            left_out,
        )))
    })
    .await
}

/// The save dialog's file type for a proposed file name: a PD capture for
/// `.pdcap`, CSV otherwise.
fn save_filter(suggested_name: &str) -> (&'static str, &'static [&'static str]) {
    if suggested_name.ends_with(".pdcap") {
        ("PD capture", &["pdcap"])
    } else {
        ("CSV", &["csv"])
    }
}

/// Ask where to save a file, proposing `suggested_name`; `None` when the
/// dialog is cancelled. Blocks: call it on a blocking thread.
fn ask_save_path(
    app: &tauri::AppHandle,
    suggested_name: &str,
) -> Option<tauri_plugin_dialog::FilePath> {
    let (name, extensions) = save_filter(suggested_name);
    app.dialog()
        .file()
        .set_file_name(suggested_name)
        .add_filter(name, extensions)
        .blocking_save_file()
}

/// Where to save a file proposed as `suggested_name`: the dialog's choice,
/// or `None` when it was cancelled. The e2e build saves into
/// `$OPEN_POWERZ_E2E_SAVE_DIR` without asking when that is set.
fn choose_save_path(
    app: &tauri::AppHandle,
    suggested_name: &str,
) -> Result<Option<PathBuf>, UiError> {
    #[cfg(feature = "e2e")]
    if let Some(path) = e2e::save_path(
        std::env::var_os(e2e::SAVE_DIR_ENV).as_deref(),
        suggested_name,
    ) {
        return Ok(Some(path));
    }
    let Some(choice) = ask_save_path(app, suggested_name) else {
        return Ok(None);
    };
    // A save dialog that answers with something other than a file path is a
    // fault, not a cancel.
    choice.into_path().map(Some).map_err(|_| {
        UiError::usage(
            "The save dialog did not return a file on disk; pick a folder on this computer.",
        )
    })
}

/// Refuse before the dialog when `prepare` does, ask where to save, then
/// run `then` with what `prepare` gives and the chosen path; `None` when the
/// dialog was cancelled (nothing runs). `prepare` runs again after the
/// dialog, since the session may have ended meanwhile.
fn with_save_path<P, R>(
    state: &AppState,
    app: &tauri::AppHandle,
    suggested_name: &str,
    prepare: impl Fn(&AppManager) -> Result<P, UiError>,
    then: impl FnOnce(P, &Path) -> Result<R, UiError>,
) -> Result<Option<R>, UiError> {
    prepare(&state.manager())?;
    let Some(path) = choose_save_path(app, suggested_name)? else {
        return Ok(None);
    };
    let prepared = prepare(&state.manager())?;
    then(prepared, &path).map(Some)
}

/// Open the save dialog with `suggested_name` and record the stream to the
/// chosen file. Resolves to the path, or `null` when the dialog was
/// cancelled (nothing starts).
#[tauri::command]
async fn start_recording(
    app: tauri::AppHandle,
    state: Shared<'_>,
    suggested_name: String,
) -> Result<Option<String>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        with_save_path(
            &state,
            &app,
            &suggested_name,
            AppManager::control,
            |control, path| {
                control.record_to(path)?;
                Ok(path.to_string_lossy().into_owned())
            },
        )
    })
    .await
}

/// Stop the recording and return its row count and gaps.
#[tauri::command]
async fn stop_recording(state: Shared<'_>) -> Result<RecordingSummary, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = state.manager().control()?;
        control.stop_recording()
    })
    .await
}

/// The session's store, taken under the lock; queries run without it.
fn session_store(state: &AppState) -> Result<store::SessionStore, UiError> {
    state.manager().session_store()
}

/// Statistics of the session (open or last) since its start or the last
/// reset.
#[tauri::command]
async fn stats(state: Shared<'_>) -> Result<stats::StatsSnapshot, UiError> {
    let state = Arc::clone(&state);
    blocking(move || Ok(session_store(&state)?.stats())).await
}

/// Zero the session's statistics; they count again from the next sample.
#[tauri::command]
async fn reset_stats(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        session_store(&state)?.reset_stats();
        Ok(())
    })
    .await
}

/// Discard the session's history (the stream and any recording go on).
#[tauri::command]
async fn clear_history(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        session_store(&state)?.clear_history();
        Ok(())
    })
    .await
}

/// Min / max buckets of the session's history (open or last) over `t0_ms..=t1_ms`,
/// at most `buckets` per series, plus the record just outside each end that falls
/// between records, so lines run to the view's edges at any zoom (AC-9CFMD3.4).
#[tauri::command]
async fn history_range(
    state: Shared<'_>,
    series: Vec<history::Series>,
    t0_ms: u64,
    t1_ms: u64,
    buckets: usize,
) -> Result<Vec<history::SeriesBuckets>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        Ok(session_store(&state)?.history_range_to_edges(&series, t0_ms, t1_ms, buckets))
    })
    .await
}

/// The gaps between measuring runs of the session history over
/// `t0_ms..=t1_ms`, so the chart breaks its lines there (AC-272WSN.3).
#[tauri::command]
async fn history_gaps(
    state: Shared<'_>,
    t0_ms: u64,
    t1_ms: u64,
) -> Result<Vec<history::HistoryGap>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || Ok(session_store(&state)?.history_gaps(t0_ms, t1_ms))).await
}

/// Where an export went and how many rows it wrote.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExportResult {
    pub path: String,
    pub rows: u64,
}

/// Open the save dialog with `suggested_name` and export the session so far
/// to the chosen file, with progress over `on_progress`. Resolves to `null`
/// when the dialog was cancelled.
#[tauri::command]
async fn export_session(
    app: tauri::AppHandle,
    state: Shared<'_>,
    suggested_name: String,
    on_progress: Channel<ExportProgress>,
) -> Result<Option<ExportResult>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        with_save_path(
            &state,
            &app,
            &suggested_name,
            AppManager::exporter,
            |exporter, path| {
                let job = exporter.export_to(path, move |p| {
                    let _ = on_progress.send(p);
                })?;
                let rows = job
                    .join()
                    .map_err(|_| UiError::internal())
                    .and_then(|done| done)
                    .inspect_err(|_| {
                        // A half-written export is worse than none.
                        let _ = std::fs::remove_file(path);
                    })?;
                Ok(ExportResult {
                    path: path.to_string_lossy().into_owned(),
                    rows,
                })
            },
        )
    })
    .await
}

/// Open the save dialog with `suggested_name` and save the PD capture so
/// far to the chosen file as a pdcap file (FR TGE83E). Resolves to the path
/// and the number of records, or `null` when the dialog was cancelled.
#[tauri::command]
async fn pd_save(
    app: tauri::AppHandle,
    state: Shared<'_>,
    suggested_name: String,
) -> Result<Option<ExportResult>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        with_save_path(
            &state,
            &app,
            &suggested_name,
            AppManager::control,
            |control, path| {
                let rows = control.save_pd(path)?;
                Ok(ExportResult {
                    path: path.to_string_lossy().into_owned(),
                    rows,
                })
            },
        )
    })
    .await
}

/// The worker's event sink over one `tauri::ipc::Channel`: batches, gaps,
/// status and errors all reach the UI through it. A failed send means the
/// window is gone: the event is dropped and `stop` is raised so the worker
/// closes the meter instead of polling it for nobody.
pub fn channel_sink(
    channel: Channel<BackendEvent>,
    stop: Arc<AtomicBool>,
) -> impl worker::EventSink {
    move |event: BackendEvent| {
        if channel.send(event).is_err() {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(AppState::new()))
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_devices,
            watch_devices,
            connect,
            attach,
            disconnect,
            cancel_wait,
            set_rate,
            start_recording,
            stop_recording,
            export_session,
            history_range,
            history_gaps,
            stats,
            reset_stats,
            clear_history,
            pd_start,
            pd_stop,
            pd_details,
            pd_clear,
            pd_save,
            pd_open,
            pd_import,
            trigger_open,
            trigger_detect,
            trigger_list,
            trigger_risk,
            trigger_request,
            trigger_back_to_5v,
            trigger_message_risk,
            trigger_message_send,
            trigger_console_risk,
            trigger_console_send,
            trigger_close
        ])
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(
                    event,
                    tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                )
            {
                shutdown_app(window);
            }
        });
    // macOS can end the WebKit content process (memory pressure, a crash),
    // leaving a blank window (FR X2SPDW). Reload the page: the backend session
    // keeps running, and the new page re-attaches to it on load (FR VEDDFD).
    // A reload fires no CloseRequested/Destroyed, so it never shuts down.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = builder.on_web_content_process_terminate(|webview| {
        if let Err(e) = webview.reload() {
            eprintln!("open-powerz: reload after web content process end failed: {e}");
        }
    });
    builder
        .build(tauri::generate_context!())
        .expect("error while building the open-powerz desktop app")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                shutdown_app(app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_save_dialog_offers_the_file_type_of_the_proposed_name() {
        assert_eq!(save_filter("powerz-pd-20261022-101500.pdcap").1, ["pdcap"]);
        assert_eq!(save_filter("powerz-20261022-101500.csv").1, ["csv"]);
    }

    #[test]
    fn app_info_reports_the_protocol_library_version() {
        // The About info reports the protocol library's own version.
        assert_eq!(app_info().protocol_version, powerz_protocol::VERSION);
    }

    fn is_semver(v: &str) -> bool {
        let parts: Vec<&str> = v.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    }

    #[test]
    fn app_info_reports_both_versions_as_semver() {
        let info = app_info();
        assert!(is_semver(&info.app_version), "{info:?}");
        assert!(is_semver(&info.protocol_version), "{info:?}");
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("app_version").is_some() && json.get("protocol_version").is_some());
    }

    #[test]
    fn a_closed_window_channel_raises_the_stop_flag() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let channel: Channel<BackendEvent> = Channel::new(|_| Err(tauri::Error::WebviewNotFound));
        let mut sink = channel_sink(channel, std::sync::Arc::clone(&stop));
        sink(BackendEvent::Status(worker::Status::Disconnected {
            error: None,
        }));
        assert!(
            stop.load(Ordering::SeqCst),
            "a dead channel left the worker running"
        );
    }

    #[test]
    fn app_state_lists_the_demo_device_without_opening_anything() {
        let state = AppState::new();
        let list = state.manager().list_devices();
        assert!(list.iter().any(|d| d.id == "demo:idle"), "{list:?}");
    }

    #[test]
    fn connecting_a_meter_releases_the_opened_capture() {
        // No USB discovery: only the demo device is listed.
        let state = AppState::with_manager(DeviceManager::new(
            || Ok(Vec::new()),
            AppOpener,
            SystemClock,
        ));
        *state.opened_pd() = Some(pd_capture::PdCapture::new());
        state
            .connect_session(
                "demo:idle",
                1000,
                |_: BackendEvent| {},
                Arc::new(AtomicBool::new(false)),
            )
            .expect("the demo device connects");
        let still_opened = state.opened_pd().is_some();
        if let Ok(session) = state.manager().take_session() {
            session.disconnect();
        }
        assert!(!still_opened, "the opened capture outlived the new session");
    }
}
