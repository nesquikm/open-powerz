pub mod batch;
pub mod devices;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod history;
pub mod recording;
pub mod stats;
pub mod store;
pub mod ui_error;
pub mod worker;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use powerz_protocol::session::SystemClock;
use powerz_protocol::transport::discovery;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

use crate::devices::{AppOpener, DeviceListing, DeviceManager, WatchHandle};
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
}

impl AppState {
    fn new() -> AppState {
        AppState {
            manager: Mutex::new(DeviceManager::new(
                discovery::list_devices,
                AppOpener,
                SystemClock,
            )),
            watch: Mutex::new(None),
        }
    }

    fn manager(&self) -> MutexGuard<'_, AppManager> {
        self.manager.lock().unwrap_or_else(PoisonError::into_inner)
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
        let handle = state.manager().watch_devices_with_stop(
            move |list| {
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
/// The session ends by itself once that channel is gone (window closed or
/// reloaded).
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
        state
            .manager()
            .connect_with_stop(&device_id, rate_sps, sink, stop)
    })
    .await
}

/// Stop the queue and close the open session.
#[tauri::command]
async fn disconnect(state: Shared<'_>) -> Result<(), UiError> {
    let state = Arc::clone(&state);
    blocking(move || {
        // Take the session under the lock; wait for the worker without it.
        let session = state.manager().take_session()?;
        session.disconnect();
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

/// Ask where to save a CSV file, proposing `suggested_name`; `None` when
/// the dialog is cancelled. Blocks: call it on a blocking thread.
fn ask_save_path(
    app: &tauri::AppHandle,
    suggested_name: &str,
) -> Option<tauri_plugin_dialog::FilePath> {
    app.dialog()
        .file()
        .set_file_name(suggested_name)
        .add_filter("CSV", &["csv"])
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
/// at most `buckets` per series.
#[tauri::command]
async fn history_range(
    state: Shared<'_>,
    series: Vec<history::Series>,
    t0_ms: u64,
    t1_ms: u64,
    buckets: usize,
) -> Result<Vec<history::SeriesBuckets>, UiError> {
    let state = Arc::clone(&state);
    blocking(move || Ok(session_store(&state)?.history_range(&series, t0_ms, t1_ms, buckets))).await
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(AppState::new()))
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_devices,
            watch_devices,
            connect,
            disconnect,
            set_rate,
            start_recording,
            stop_recording,
            export_session,
            history_range,
            stats,
            reset_stats,
            clear_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running the open-powerz desktop app");
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
