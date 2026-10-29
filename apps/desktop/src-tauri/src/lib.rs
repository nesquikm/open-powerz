pub mod activity;
pub mod batch;
pub mod devices;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod firmware_file;
pub mod firmware_flash;
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
    #[cfg(test)]
    fn connect_session(
        &self,
        device_id: &str,
        rate_sps: u32,
        sink: impl worker::EventSink,
        stop: Arc<AtomicBool>,
    ) -> Result<(), UiError> {
        self.connect_source_session(
            device_id,
            rate_sps,
            worker::MeasurementSource::Queue,
            sink,
            stop,
        )
    }

    /// [`AppState::connect_session`] streaming from `source` (FR 1HHT02).
    fn connect_source_session(
        &self,
        device_id: &str,
        rate_sps: u32,
        source: worker::MeasurementSource,
        sink: impl worker::EventSink,
        stop: Arc<AtomicBool>,
    ) -> Result<(), UiError> {
        self.manager()
            .connect_source_with_stop(device_id, rate_sps, source, sink, stop)?;
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
/// `source` (`queue`, the default, or `cdc`) picks where the measurements
/// come from (FR 1HHT02). The session ends by itself once that channel is
/// gone (window closed); a reloaded page takes the session over with
/// `attach` instead.
#[tauri::command]
async fn connect(
    state: Shared<'_>,
    device_id: String,
    rate_sps: u32,
    source: Option<worker::MeasurementSource>,
    on_event: Channel<BackendEvent>,
) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let stop = Arc::new(AtomicBool::new(false));
        let sink = channel_sink(on_event, Arc::clone(&stop));
        state.connect_source_session(&device_id, rate_sps, source.unwrap_or_default(), sink, stop)
    })
    .await
}

/// Pick up the running session from a new page (after a reload): its later
/// events go over `on_event`. `null` when no session is running. Without
/// `device_id` it is the current meter's session; with one, that meter's
/// (FR 1HHT03: the page picks up every running meter, see `running_meters`).
#[tauri::command]
async fn attach(
    state: Shared<'_>,
    device_id: Option<String>,
    on_event: Channel<BackendEvent>,
) -> Result<Option<AttachedSession>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let id = state.manager().target(device_id.as_deref());
        // A reloaded page (e.g. after the web content process ended) starts
        // without a trigger session, so one the old page left open returns
        // to 5 V and closes now, outside the manager lock (FR DGSQ6S).
        let leftover = state.manager().take_trigger_for(&id);
        if let Some(trigger) = leftover {
            let _ = trigger.close();
        }
        // Taken under the lock; the handover waits for the worker without it.
        let Some(attacher) = state.manager().attacher_for(&id) else {
            return Ok(None);
        };
        // The new page's channel ends the session once it is gone, as the
        // connecting page's did.
        let sink = channel_sink(on_event, attacher.stop());
        Ok(attacher.attach(sink))
    })
    .await
}

/// The device ids of the meters whose session runs, as their attach
/// snapshots name them, in the order they were connected (FR 1HHT03): what a
/// reloaded page picks up with `attach`.
#[tauri::command]
async fn running_meters(state: Shared<'_>) -> Result<Vec<String>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || Ok(state.manager().running_ids())).await
}

/// Stop the queue and close the open session.
#[tauri::command]
async fn disconnect(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // Take the sessions under the lock; wait for them without it.
        let (trigger, session) = {
            let mut manager = state.manager();
            let id = manager.target(device_id.as_deref());
            (manager.take_trigger_for(&id), manager.take_session_for(&id))
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
async fn cancel_wait(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        match device_id {
            Some(id) => state.manager().cancel_wait_for(&id),
            None => state.manager().cancel_wait(),
        }
        Ok(())
    })
    .await
}

/// Restart the open session's queue at `rate_sps`.
#[tauri::command]
async fn set_rate(
    state: Shared<'_>,
    device_id: Option<String>,
    rate_sps: u32,
) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // Take the control under the lock; wait for the answer without it.
        let (control, rate) = {
            let manager = state.manager();
            manager.rate_control_for(&manager.target(device_id.as_deref()), rate_sps)?
        };
        control.set_rate(rate)
    })
    .await
}

/// Start the PD capture on the open session (FR TGE83C); rows and the
/// capture state arrive on the session channel.
#[tauri::command]
async fn pd_start(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = {
            let manager = state.manager();
            let id = manager.target(device_id.as_deref());
            // The PD capture waits while triggering (FR DGSQ6S). [inferred]
            if manager.trigger_open_for(&id) {
                return Err(UiError::usage(
                    "Close the trigger session to capture PD traffic.",
                ));
            }
            manager.control_for(&id)?
        };
        control.start_pd()
    })
    .await
}

/// Open the trigger session on the connected meter (FR DGSQ6S).
#[tauri::command]
async fn trigger_open(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // The port opens and the PDM handshake runs without the manager
        // lock, so a slow meter never blocks disconnect or attach.
        let (id, opener) = {
            let manager = state.manager();
            let id = manager.target(device_id.as_deref());
            let opener = manager.trigger_opener_for(&id)?;
            (id, opener)
        };
        let worker = opener.open()?;
        state.manager().install_trigger_for(&id, worker)
    })
    .await
}

/// BC 1.2, Apple and the automatic list; nothing above 5 V is asked.
#[tauri::command]
async fn trigger_detect(
    state: Shared<'_>,
    device_id: Option<String>,
) -> Result<trigger::Detection, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // The handle is taken off the manager, so its lock is not held while
        // the session thread answers.
        let control = trigger_control(&state, device_id.as_deref())?;
        control.detect()
    })
    .await
}

/// One protocol's section; a protocol the charger lacks is `supported: false`.
#[tauri::command]
async fn trigger_list(
    state: Shared<'_>,
    device_id: Option<String>,
    protocol: trigger::TriggerProtocol,
) -> Result<trigger::ProtocolSection, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.list(protocol)
    })
    .await
}

/// The library risk class of a request; `above_5v` needs the user's
/// confirmation.
#[tauri::command]
async fn trigger_risk(
    state: Shared<'_>,
    device_id: Option<String>,
    request: trigger::TriggerRequest,
) -> Result<trigger::RiskClass, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.risk(request)
    })
    .await
}

/// Send a request; above 5 V it is refused unless `confirmed` (the backend
/// is the final gate).
#[tauri::command]
async fn trigger_request(
    state: Shared<'_>,
    device_id: Option<String>,
    request: trigger::TriggerRequest,
    confirmed: bool,
) -> Result<trigger::TriggerReply, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.request(request, confirmed)
    })
    .await
}

/// The library risk class of a built PD message (FR DGSQ6T); GoodCRC and
/// malformed messages are a usage error with the reason.
#[tauri::command]
async fn trigger_message_risk(
    state: Shared<'_>,
    device_id: Option<String>,
    message: trigger::PdMessageRequest,
) -> Result<trigger::RiskClass, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.message_risk(message)
    })
    .await
}

/// Send a built PD message; above 5 V it is refused unless `confirmed`.
#[tauri::command]
async fn trigger_message_send(
    state: Shared<'_>,
    device_id: Option<String>,
    message: trigger::PdMessageRequest,
    confirmed: bool,
) -> Result<trigger::PdSendReply, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.send_message(message, confirmed)
    })
    .await
}

/// The library risk class of a raw console line (FR DGSQ6T).
#[tauri::command]
async fn trigger_console_risk(
    state: Shared<'_>,
    device_id: Option<String>,
    line: String,
) -> Result<trigger::RiskClass, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.console_risk(line)
    })
    .await
}

/// Send a raw console line; above 5 V (any unparsable line too) it is
/// refused unless `confirmed`; GoodCRC is always refused.
#[tauri::command]
async fn trigger_console_send(
    state: Shared<'_>,
    device_id: Option<String>,
    line: String,
    confirmed: bool,
) -> Result<Vec<trigger::ConsoleLine>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.console_send(line, confirmed)
    })
    .await
}

/// Back to 5 V: PD `pd req=1` (Hard Reset as the fallback), the other
/// protocols `reset`. The session stays open.
#[tauri::command]
async fn trigger_back_to_5v(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = trigger_control(&state, device_id.as_deref())?;
        control.back_to_5v()
    })
    .await
}

/// Back to 5 V and close the trigger session, if one is open.
#[tauri::command]
async fn trigger_close(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // Take the session under the lock; close it without it.
        let trigger = {
            let mut manager = state.manager();
            let id = manager.target(device_id.as_deref());
            manager.take_trigger_for(&id)
        };
        trigger.map_or(Ok(()), trigger::TriggerWorker::close)
    })
    .await
}

/// Stop the PD capture; the captured records stay.
#[tauri::command]
async fn pd_stop(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = control(&state, device_id.as_deref())?;
        control.stop_pd()
    })
    .await
}

/// The raw bytes and field tree of PD record `index`: of the capture
/// opened from a file when `opened` is true, of the live capture otherwise.
#[tauri::command]
async fn pd_details(
    state: Shared<'_>,
    device_id: Option<String>,
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
        let control = control(&state, device_id.as_deref())?;
        control.pd_details(index)
    })
    .await
}

/// Read the connected meter's settings (GET_DATA 0x008 only) for the
/// read-only Meter settings panel (FR 1HHT01).
#[tauri::command]
async fn meter_settings(
    state: Shared<'_>,
    device_id: Option<String>,
) -> Result<worker::MeterSettingsView, UiError> {
    let state = Arc::clone(&state);
    blocking(move || control(&state, device_id.as_deref())?.read_settings()).await
}

/// List the connected meter's offline records (GET_DATA 0x200 only) for
/// the Records view (FR 1HHT00).
#[tauri::command]
async fn records_list(
    state: Shared<'_>,
    device_id: Option<String>,
) -> Result<worker::RecordListView, UiError> {
    let state = Arc::clone(&state);
    blocking(move || control(&state, device_id.as_deref())?.list_records()).await
}

/// Download offline record `index` (1-based) for the Records view: one
/// memory read of that record's own window (FR 1HHT00).
#[tauri::command]
async fn record_download(
    state: Shared<'_>,
    device_id: Option<String>,
    index: usize,
) -> Result<worker::OfflineRecordView, UiError> {
    let state = Arc::clone(&state);
    blocking(move || control(&state, device_id.as_deref())?.download_record(index)).await
}

/// Delete offline record `index` (1-based) or, for `null`, every record
/// (FR JKWT3V). `confirm` is the typed record name (or "all"). The affected
/// records are backed up first as CSV files into a new folder under the
/// Downloads folder (`open-powerz-backups/records-<time>`); the answer names it.
#[tauri::command]
async fn records_delete(
    app: tauri::AppHandle,
    state: Shared<'_>,
    device_id: Option<String>,
    index: Option<usize>,
    confirm: String,
) -> Result<worker::RecordDeleteView, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let dir = backup_dir(&app)?;
        control(&state, device_id.as_deref())?.delete_records(index, &confirm, &dir)
    })
    .await
}

/// A new folder name for a record backup: `open-powerz-backups/records-<unix ms>`
/// under the Downloads folder (the app data folder when there is none).
fn backup_dir(app: &tauri::AppHandle) -> Result<PathBuf, UiError> {
    use tauri::Manager;
    let base = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|_| UiError::usage("No folder found to write the record backup to."))?;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Ok(base
        .join("open-powerz-backups")
        .join(format!("records-{ms}")))
}

/// Drop every captured PD record; the answer is the index of the next one.
/// A capture opened from a file is dropped too; with only that one and no
/// meter, the answer is 0.
#[tauri::command]
async fn pd_clear(state: Shared<'_>, device_id: Option<String>) -> Result<u64, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let had_opened = state.opened_pd().take().is_some();
        let control = match control(&state, device_id.as_deref()) {
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

/// Inspect the firmware container at `path` (FR V9SK53, AC .3); needs no
/// meter. Resolves to its metadata with the English release notes.
#[tauri::command]
async fn firmware_inspect(path: String) -> Result<firmware_file::FirmwareFileInfo, UiError> {
    blocking(move || firmware_file::inspect_firmware_file(std::path::Path::new(&path))).await
}

/// Ask for a firmware container to update the meter with (FR V9SK55, AC .2);
/// resolves to its path, or `null` when the dialog was cancelled.
#[tauri::command]
async fn firmware_choose(app: tauri::AppHandle) -> Result<Option<String>, UiError> {
    blocking(move || {
        let path = choose_open_path(&app, "Firmware file", &["mencrypt"])?;
        Ok(path.map(|p| p.to_string_lossy().into_owned()))
    })
    .await
}

/// Update the meter `device_id` with the firmware container at `path`
/// (FR V9SK55, AC .4): takes the meter from every other use for the
/// update, re-checks that `confirmation` is the file's version before
/// anything is sent, streams each stage over `on_progress` (the write
/// stage about every percent) and releases the meter afterwards.
#[tauri::command]
async fn firmware_flash(
    state: Shared<'_>,
    device_id: String,
    path: String,
    confirmation: String,
    on_progress: Channel<firmware_flash::FlashStage>,
) -> Result<firmware_flash::FlashDone, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let lease = state.manager().begin_flash(&device_id)?;
        let device = lease.device().clone();
        let stop = AtomicBool::new(false);
        let mut last_step = None;
        let mut progress = move |stage: firmware_flash::FlashStage| {
            if let firmware_flash::FlashStage::Writing { done, total } = stage {
                // One message per percent, plus the last write.
                let step = done.saturating_mul(100) / total.max(1);
                if last_step == Some(step) && done < total {
                    return;
                }
                last_step = Some(step);
            }
            let _ = on_progress.send(stage);
        };
        let path = Path::new(&path);
        let done = match device.demo_scenario.as_deref() {
            Some(name) => {
                let scenario = powerz_sim::Scenario::from_name(name)
                    .ok_or_else(|| UiError::usage("That meter is no longer attached."))?;
                let mut meter = powerz_sim::SimMeter::new(powerz_sim::SimDevice::new(
                    powerz_sim::Persona::Bulk,
                    scenario,
                ));
                firmware_flash::flash_file(
                    path,
                    &confirmation,
                    &mut meter,
                    SystemClock,
                    &stop,
                    &mut progress,
                )
            }
            None => {
                let mut opener = firmware_flash::UsbFlashOpener::new(device.serial.clone());
                firmware_flash::flash_file(
                    path,
                    &confirmation,
                    &mut opener,
                    SystemClock,
                    &stop,
                    &mut progress,
                )
            }
        };
        drop(lease);
        done
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
/// dialog was cancelled (nothing runs). The meter is resolved once, before
/// the dialog (the one `device_id` names, else the current one), so a meter
/// connected meanwhile never takes the save; `prepare` runs again for it
/// after the dialog, since its session may have ended meanwhile.
fn with_save_path<P, R>(
    state: &AppState,
    app: &tauri::AppHandle,
    device_id: Option<&str>,
    suggested_name: &str,
    prepare: impl Fn(&AppManager, &str) -> Result<P, UiError>,
    then: impl FnOnce(P, &Path) -> Result<R, UiError>,
) -> Result<Option<R>, UiError> {
    save_with(
        state,
        device_id,
        || choose_save_path(app, suggested_name),
        prepare,
        then,
    )
}

/// [`with_save_path`] with the dialog as `choose`.
fn save_with<P, R>(
    state: &AppState,
    device_id: Option<&str>,
    choose: impl FnOnce() -> Result<Option<PathBuf>, UiError>,
    prepare: impl Fn(&AppManager, &str) -> Result<P, UiError>,
    then: impl FnOnce(P, &Path) -> Result<R, UiError>,
) -> Result<Option<R>, UiError> {
    let id = {
        let m = state.manager();
        let id = m.target(device_id);
        prepare(&m, &id)?;
        id
    };
    let Some(path) = choose()? else {
        return Ok(None);
    };
    let prepared = prepare(&state.manager(), &id)?;
    then(prepared, &path).map(Some)
}

/// Open the save dialog with `suggested_name` and record the stream to the
/// chosen file. Resolves to the path, or `null` when the dialog was
/// cancelled (nothing starts).
#[tauri::command]
async fn start_recording(
    app: tauri::AppHandle,
    state: Shared<'_>,
    device_id: Option<String>,
    suggested_name: String,
) -> Result<Option<String>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        with_save_path(
            &state,
            &app,
            device_id.as_deref(),
            &suggested_name,
            |m: &AppManager, id| m.control_for(id),
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
async fn stop_recording(
    state: Shared<'_>,
    device_id: Option<String>,
) -> Result<RecordingSummary, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        let control = control(&state, device_id.as_deref())?;
        control.stop_recording()
    })
    .await
}

/// The store of meter `device_id` (the current session's without one),
/// taken under the lock; queries run without it.
fn session_store(
    state: &AppState,
    device_id: Option<&str>,
) -> Result<store::SessionStore, UiError> {
    let manager = state.manager();
    manager.session_store_for(&manager.target(device_id))
}

/// The worker control of meter `device_id` (the current session's without
/// one), taken under the lock; the caller waits for answers without it.
fn control(state: &AppState, device_id: Option<&str>) -> Result<worker::WorkerControl, UiError> {
    let manager = state.manager();
    manager.control_for(&manager.target(device_id))
}

/// The trigger handle of meter `device_id` (the current session's without
/// one), taken under the lock.
fn trigger_control(
    state: &AppState,
    device_id: Option<&str>,
) -> Result<trigger::TriggerControl, UiError> {
    let manager = state.manager();
    manager.trigger_control_for(&manager.target(device_id))
}

/// Statistics of the session (open or last) since its start or the last
/// reset.
#[tauri::command]
async fn stats(
    state: Shared<'_>,
    device_id: Option<String>,
) -> Result<stats::StatsSnapshot, UiError> {
    let state = Arc::clone(&state);
    blocking(move || Ok(session_store(&state, device_id.as_deref())?.stats())).await
}

/// Zero the session's statistics; they count again from the next sample.
#[tauri::command]
async fn reset_stats(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        session_store(&state, device_id.as_deref())?.reset_stats();
        Ok(())
    })
    .await
}

/// Discard the session's history (the stream and any recording go on).
#[tauri::command]
async fn clear_history(state: Shared<'_>, device_id: Option<String>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        session_store(&state, device_id.as_deref())?.clear_history();
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
    device_id: Option<String>,
    series: Vec<history::Series>,
    t0_ms: u64,
    t1_ms: u64,
    buckets: usize,
) -> Result<Vec<history::SeriesBuckets>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        Ok(session_store(&state, device_id.as_deref())?
            .history_range_to_edges(&series, t0_ms, t1_ms, buckets))
    })
    .await
}

/// The gaps between measuring runs of the session history over
/// `t0_ms..=t1_ms`, so the chart breaks its lines there (AC-272WSN.3).
#[tauri::command]
async fn history_gaps(
    state: Shared<'_>,
    device_id: Option<String>,
    t0_ms: u64,
    t1_ms: u64,
) -> Result<Vec<history::HistoryGap>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || Ok(session_store(&state, device_id.as_deref())?.history_gaps(t0_ms, t1_ms)))
        .await
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
    device_id: Option<String>,
    suggested_name: String,
    on_progress: Channel<ExportProgress>,
) -> Result<Option<ExportResult>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        with_save_path(
            &state,
            &app,
            device_id.as_deref(),
            &suggested_name,
            |m: &AppManager, id| m.exporter_for(id),
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
    device_id: Option<String>,
    suggested_name: String,
) -> Result<Option<ExportResult>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        with_save_path(
            &state,
            &app,
            device_id.as_deref(),
            &suggested_name,
            |m: &AppManager, id| m.control_for(id),
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
            running_meters,
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
            meter_settings,
            records_list,
            record_download,
            records_delete,
            pd_save,
            pd_open,
            pd_import,
            firmware_inspect,
            firmware_choose,
            firmware_flash,
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
            // A firmware update in flight must not be cut short by closing the
            // window: refuse the close until it ends.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && let Some(state) = tauri::Manager::try_state::<Arc<AppState>>(window)
                && !state.manager().close_allowed()
            {
                api.prevent_close();
                return;
            }
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
            // Quitting (Cmd+Q) is refused during a firmware update, as closing is.
            if let tauri::RunEvent::ExitRequested { api, .. } = &event
                && let Some(state) = tauri::Manager::try_state::<Arc<AppState>>(app)
                && !state.manager().close_allowed()
            {
                api.prevent_exit();
                return;
            }
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

    /// The meter a save goes to is the one named (or current) when the save
    /// was asked for, even when another meter connects while the dialog is
    /// open (FR 1HHT03).
    #[test]
    fn a_save_goes_to_the_meter_current_before_the_dialog() {
        let state = AppState::with_manager(DeviceManager::new(
            || Ok(Vec::new()),
            AppOpener,
            SystemClock,
        ));
        let stop = || Arc::new(AtomicBool::new(false));
        state
            .connect_session("demo:load-5v-1a", 50, |_: BackendEvent| {}, stop())
            .expect("the first demo connects");
        let target = save_with(
            &state,
            None,
            || {
                // Another meter connects while the dialog is open: it
                // becomes the current one.
                state
                    .connect_session("demo:pd-9v-2a", 50, |_: BackendEvent| {}, stop())
                    .expect("the second demo connects");
                Ok(Some(PathBuf::from("unused.csv")))
            },
            |_m: &AppManager, id| Ok(id.to_owned()),
            |id, _path| Ok(id),
        );
        for id in ["demo:load-5v-1a", "demo:pd-9v-2a"] {
            let _ = state.manager().disconnect_for(id);
        }
        assert_eq!(
            target.expect("the save ran"),
            Some("demo:load-5v-1a".to_owned()),
            "the save went to the meter connected during the dialog"
        );
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
