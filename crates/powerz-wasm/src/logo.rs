//! The startup logo in the browser (FR W8SHXB): the preview of a picture or
//! a built-in logo, one upload or Remove as the native `Session::logo_upload`
//! / `Session::logo_remove` send it, the logo read and the settings-saved
//! check. Sans-IO: the page moves the frames, these state machines build and
//! check them through the protocol core's logo paths and guards. Errors carry
//! the desktop app's wording.

use powerz_protocol::decode::records::MEMORY_READ_MAX_LEN;
use powerz_protocol::decode::settings::{
    CalibrationSnapshot, MENU_SAVE_STEPS, Settings, StartupLogo,
};
use powerz_protocol::frame::{self, Attribute, Reply};
use powerz_protocol::logo::builtin::BuiltinLogo;
use powerz_protocol::logo::{self, CHUNK, HEADER_LEN, LogoBlob, LogoMode};
use powerz_protocol::session::{
    self, LogoRefusal, LogoRemoveStep, SettingsSave, logo_chunk_plaintext, logo_flag_request,
    settings_save,
};
use powerz_protocol::transport::guard::{
    self, LOGO_REGION_START, LOGO_WINDOW_END, LOGO_WINDOW_START,
};
use powerz_protocol::transport::logo_chunk_frame;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::BindError;
use crate::decoders::to_js;
use crate::meter_reads::{Chunk, section_payload, settings_request};

/// `value` for JS with every `None` as `null`, as the desktop app's views
/// reach the UI.
fn to_js_view<T: Serialize>(value: &T) -> Result<JsValue, BindError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .map_err(|e| BindError::new("internal", e.to_string()))
}

const REFUSED_HINT: &str = "Replug the meter so the app reads it again, then retry.";
const CHUNK_HINT: &str =
    "The startup duration was not changed, so the meter keeps its old screen. Try again.";
const UPLOAD_HINT: &str = "Try the upload again; nothing was retried.";
const REMOVE_HINT: &str = "Try Remove again; nothing was retried.";
const REREAD: &str = "The logo was written, but the settings could not be read again.";

/// Startup-duration value written after an upload: code 2 = 3 s. \[device\]
const DURATION_UPLOAD: u32 = 2;
/// Startup-duration value written by a Remove: code 0 = off. \[device\]
const DURATION_REMOVE: u32 = 0;
/// Bytes of 0xFF a Remove writes at the region start. \[static\]
const REMOVE_LEN: usize = 16;

fn usage(message: impl Into<String>) -> BindError {
    BindError::new("usage", message)
}

fn out_of_order(what: &str) -> BindError {
    BindError::invalid(format!("{what} is out of order in a logo write"))
}

fn mode_name(mode: LogoMode) -> &'static str {
    match mode {
        LogoMode::Opaque => "opaque",
        LogoMode::Alpha => "alpha",
    }
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digit = |v: u32| char::from(A[(v & 63) as usize]);
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let b = [
            group.first().copied().unwrap_or(0),
            group.get(1).copied().unwrap_or(0),
            group.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(digit(n >> 18));
        out.push(digit(n >> 12));
        out.push(if group.len() > 1 { digit(n >> 6) } else { '=' });
        out.push(if group.len() > 2 { digit(n) } else { '=' });
    }
    out
}

// ---- preview (AC-W8SHXB.1) ----

/// What the preview shows, as the desktop app's `LogoPreviewView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoPreviewView {
    /// Width in pixels of the logo as written.
    pub width: u32,
    /// Height in pixels of the logo as written.
    pub height: u32,
    /// `"opaque"` or `"alpha"`.
    pub mode: String,
    /// Unpadded blob size in bytes.
    pub blob_size: usize,
    /// Padded (written) size in bytes.
    pub padded_size: usize,
    /// Number of 256-byte chunks written.
    pub chunks: usize,
    /// Either side is below 48.
    pub small_side_warning: bool,
    /// The blob decoded back to RGBA8888, base64.
    pub rgba: String,
}

/// The blob a picture file encodes to, as the desktop `blob_from_file`: a
/// raw `.bin` logo file when `is_bin` (the name's extension), else PNG,
/// JPEG or BMP; opaque, or transparent with `alpha`.
fn blob_from_bytes(bytes: &[u8], is_bin: bool, alpha: bool) -> Result<LogoBlob, BindError> {
    let img = if is_bin {
        logo::decode(bytes)
    } else {
        logo::load_image_bytes(bytes)
    }
    .map_err(|e| usage(e.to_string()))?;
    let mode = if alpha {
        LogoMode::Alpha
    } else {
        LogoMode::Opaque
    };
    logo::encode(&img, mode).map_err(|e| usage(e.to_string()))
}

/// The blob of built-in logo `name` (always transparent).
fn blob_from_builtin(name: &str) -> Result<LogoBlob, BindError> {
    let b = name
        .parse::<BuiltinLogo>()
        .map_err(|e| usage(e.to_string()))?;
    b.encode().map_err(|e| usage(format!("{b}: {e}")))
}

fn preview_blob(blob: &LogoBlob) -> Result<LogoPreviewView, BindError> {
    let p = blob.preview();
    let pixels = logo::decode(&blob.bytes).map_err(|e| usage(e.to_string()))?;
    Ok(LogoPreviewView {
        width: p.width,
        height: p.height,
        mode: mode_name(p.mode).to_owned(),
        blob_size: p.blob_size,
        padded_size: p.padded_size,
        chunks: p.chunks,
        small_side_warning: p.small_side_warning,
        rgba: base64(&pixels.px),
    })
}

/// The preview of a picture file's `bytes` (a raw logo file when `is_bin`),
/// opaque or transparent with `alpha`: the desktop app's preview of the same
/// file. A file that is no picture is a `usage` error.
pub fn logo_preview(bytes: &[u8], is_bin: bool, alpha: bool) -> Result<LogoPreviewView, BindError> {
    preview_blob(&blob_from_bytes(bytes, is_bin, alpha)?)
}

/// The preview of built-in logo `name`, as on the desktop.
pub fn logo_preview_builtin(name: &str) -> Result<LogoPreviewView, BindError> {
    preview_blob(&blob_from_builtin(name)?).map_err(|e| usage(format!("{name}: {}", e.message)))
}

/// [`logo_preview`] for JS.
#[wasm_bindgen(js_name = logoPreview)]
pub fn logo_preview_js(bytes: &[u8], is_bin: bool, alpha: bool) -> Result<JsValue, BindError> {
    to_js(&logo_preview(bytes, is_bin, alpha)?)
}

/// [`logo_preview_builtin`] for JS.
#[wasm_bindgen(js_name = logoPreviewBuiltin)]
pub fn logo_preview_builtin_js(name: &str) -> Result<JsValue, BindError> {
    to_js(&logo_preview_builtin(name)?)
}

// ---- views ----

/// What an upload or Remove left on the meter, as the desktop `LogoWrittenView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoWrittenView {
    pub duration_before: String,
    pub duration_set: String,
    /// `"saved"`, `"needs_menu_save"` or `"calibration_changed"`.
    pub settings: String,
    /// The fresh read's duration when `settings` is `"saved"`, else null.
    pub startup_duration: Option<String>,
}

/// One check of the settings after a logo write, as the desktop
/// `LogoSettingsCheckView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoSettingsCheckView {
    pub settings: String,
    pub startup_duration: Option<String>,
}

/// What the logo area holds, as the desktop `LogoReadView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoReadView {
    /// `"empty"`, `"logo"` or `"unrecognised"`.
    pub status: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub mode: Option<String>,
    /// The stored logo as RGBA8888, base64, after a full read.
    pub image: Option<String>,
}

fn settings_state(save: SettingsSave) -> (String, Option<String>) {
    match save {
        SettingsSave::Saved(d) => ("saved".to_owned(), Some(d.label().to_owned())),
        SettingsSave::NeedsMenuSave => ("needs_menu_save".to_owned(), None),
        SettingsSave::CalibrationChanged => ("calibration_changed".to_owned(), None),
    }
}

/// The SETTINGS block of a SETTINGS reply.
fn settings_block(reply: &[u8]) -> Result<Settings, BindError> {
    Ok(Settings::decode(section_payload(
        reply,
        Attribute::SETTINGS,
        "SETTINGS",
    )?)?)
}

/// One read-only check of a SETTINGS reply against the calibration
/// `snapshot` (64 bytes) a logo write kept: the desktop settings check.
pub fn settings_save_check(
    snapshot: &[u8],
    settings_reply: &[u8],
) -> Result<LogoSettingsCheckView, BindError> {
    let snapshot: [u8; 64] = snapshot
        .try_into()
        .map_err(|_| BindError::invalid("a calibration snapshot is 64 bytes"))?;
    let block = settings_block(settings_reply)?;
    let save = settings_save(&block, &CalibrationSnapshot(snapshot))
        .map_err(|e| BindError::new("protocol", e.to_string()))?;
    let (settings, startup_duration) = settings_state(save);
    Ok(LogoSettingsCheckView {
        settings,
        startup_duration,
    })
}

/// [`settings_save_check`] for JS.
#[wasm_bindgen(js_name = settingsSaveCheck)]
pub fn settings_save_check_js(
    snapshot: &[u8],
    settings_reply: &[u8],
) -> Result<JsValue, BindError> {
    to_js_view(&settings_save_check(snapshot, settings_reply)?)
}

// ---- reads inside the logo window ----

/// Refuse a read outside the logo window, as the native `logo_memory_read`.
fn check_window_read(addr: u32, len: u32) -> Result<(), BindError> {
    let inside = len > 0
        && len <= MEMORY_READ_MAX_LEN
        && addr >= LOGO_WINDOW_START
        && addr
            .checked_add(len)
            .is_some_and(|end| end <= LOGO_WINDOW_END);
    if inside {
        Ok(())
    } else {
        Err(BindError::new(
            "forbidden",
            format!("the read of {len} bytes at {addr:#x} lies outside the logo window"),
        ))
    }
}

/// The reads of `total` bytes from the window start, skipping the first
/// `done`: at most 256 bytes each, as the native `logo_read_paced`.
fn window_plan(done: usize, total: usize) -> Vec<Chunk> {
    let step = MEMORY_READ_MAX_LEN as usize;
    (done..total)
        .step_by(step)
        .map(|off| Chunk {
            addr: LOGO_WINDOW_START + off as u32,
            len: step.min(total - off) as u32,
        })
        .collect()
}

/// Take a read at `addr` into `buf`, which must be the next planned one.
fn take_window_read(
    buf: &mut Vec<u8>,
    total: usize,
    addr: u32,
    data: &[u8],
    what: &str,
) -> Result<(), BindError> {
    let Some(next) = window_plan(buf.len(), total).into_iter().next() else {
        return Err(out_of_order(what));
    };
    if next.addr != addr {
        return Err(out_of_order(what));
    }
    let n = next.len as usize;
    let Some(data) = data.get(..n) else {
        // A short read can never fill the plan: stop it here.
        return Err(BindError::new(
            "malformed",
            format!(
                "the read at {addr:#x} returned {} bytes, expected {n}",
                data.len()
            ),
        ));
    };
    buf.extend_from_slice(data);
    Ok(())
}

// ---- one upload or Remove (AC-W8SHXB.2, .3, .6) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Upload,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The preconditions were not checked yet.
    New,
    /// Chunk `next` is due; `sent` once its frame was built.
    Chunks { next: u32, sent: bool },
    /// The startup-duration write is due.
    Flag { sent: bool },
    /// The read-back reads.
    ReadBack,
    /// The one SETTINGS read after the write.
    Settings { sent: bool },
    /// Written and checked.
    Done,
    /// Refused or failed: nothing more is built.
    Over,
}

/// One logo upload or Remove: `preconditions`, then `chunkRequest` /
/// `takeChunkReply` for each chunk, `flagRequest` / `takeFlagReply`, the
/// `readPlan` reads (`readRequest` / `takeRead`), and `settingsRequest` /
/// `takeSettings`. Out of order calls are `invalid`; a failed step ends it.
#[wasm_bindgen]
#[derive(Debug)]
pub struct LogoWrite {
    kind: Kind,
    /// What is written from the region start: the padded blob, or 16 × 0xFF.
    data: Vec<u8>,
    step: Step,
    /// The duration and calibration of the pre-write SETTINGS read.
    before: Option<(StartupLogo, CalibrationSnapshot)>,
    /// The read-back so far.
    read: Vec<u8>,
    /// The calibration snapshot kept while the settings wait for a menu save.
    pending: Option<CalibrationSnapshot>,
}

impl LogoWrite {
    fn new(kind: Kind, data: Vec<u8>) -> LogoWrite {
        LogoWrite {
            kind,
            data,
            step: Step::New,
            before: None,
            read: Vec::new(),
            pending: None,
        }
    }

    /// Check the native preconditions: authenticated (`auth_level` > 0),
    /// SETTINGS read in this session (`settings_reply`, which must decode),
    /// the model read and a KM003C. A refusal has the desktop wording;
    /// nothing is built until the preconditions hold.
    pub fn preconditions(
        &mut self,
        auth_level: u8,
        settings_reply: Option<&[u8]>,
        model: Option<&str>,
    ) -> Result<(), BindError> {
        if self.step != Step::New {
            return Err(out_of_order("the preconditions"));
        }
        let refuse = |r: LogoRefusal| {
            BindError::with_hint(
                "usage",
                format!("The logo was not written: {r}."),
                REFUSED_HINT,
            )
        };
        if auth_level == 0 {
            return Err(refuse(LogoRefusal::NotAuthenticated));
        }
        let Some(block) = settings_reply.and_then(|r| settings_block(r).ok()) else {
            return Err(refuse(LogoRefusal::SettingsNotRead));
        };
        match model {
            None => return Err(refuse(LogoRefusal::ModelNotRead)),
            Some(m) if !m.trim().eq_ignore_ascii_case("KM003C") => {
                return Err(refuse(LogoRefusal::WrongModel(m.to_owned())));
            }
            Some(_) => {}
        }
        // The duration is decoded before any write, as the native `logo_before`.
        let duration = block.meter_settings()?.startup_logo.value;
        self.before = Some((duration, block.calibration_snapshot()));
        self.step = Step::Chunks {
            next: 0,
            sent: false,
        };
        Ok(())
    }

    /// The number of chunk writes.
    pub fn chunk_count(&self) -> u32 {
        self.data.len().div_ceil(CHUNK) as u32
    }

    /// The encrypted 0x4A frame of chunk `k` with request id `id`, checked by
    /// the native guard; only the next chunk, after the preconditions.
    pub fn chunk_request(&mut self, id: u8, k: u32) -> Result<Vec<u8>, BindError> {
        let Step::Chunks { next, sent: false } = self.step else {
            return Err(out_of_order("a chunk"));
        };
        if k != next {
            return Err(out_of_order("that chunk"));
        }
        let off = k as usize * CHUNK;
        let data = self
            .data
            .get(off..(off + CHUNK).min(self.data.len()))
            .ok_or_else(|| out_of_order("that chunk"))?;
        let plain = logo_chunk_plaintext(id, LOGO_REGION_START + off as u32, data);
        let frame =
            logo_chunk_frame(&plain).map_err(|e| BindError::new("forbidden", e.to_string()))?;
        self.step = Step::Chunks { next, sent: true };
        Ok(frame)
    }

    /// The error of a chunk that failed with `reason`, in the desktop wording.
    fn chunk_error(&self, k: u32, reason: &str) -> BindError {
        match self.kind {
            Kind::Upload => BindError::with_hint(
                "protocol",
                format!("Writing the logo stopped at chunk {k}: {reason}."),
                CHUNK_HINT,
            ),
            Kind::Remove => remove_error(LogoRemoveStep::Clear, reason),
        }
    }

    /// Take chunk `k`'s reply: FINISHED goes on; anything else stops the
    /// write with no retry.
    pub fn take_chunk_reply(&mut self, k: u32, reply: &[u8]) -> Result<(), BindError> {
        let Step::Chunks { next, sent: true } = self.step else {
            return Err(out_of_order("a chunk reply"));
        };
        if k != next {
            return Err(out_of_order("that chunk reply"));
        }
        if let Err(reason) = write_outcome(reply, false) {
            self.step = Step::Over;
            return Err(self.chunk_error(k, &reason));
        }
        self.step = if next + 1 >= self.chunk_count() {
            Step::Flag { sent: false }
        } else {
            Step::Chunks {
                next: next + 1,
                sent: false,
            }
        };
        Ok(())
    }

    /// The startup-duration write (SET_DATA 0x48 att 0x1C) with request id
    /// `id`: 2 after an upload, 0 for a Remove, checked by the native guard.
    pub fn flag_request(&mut self, id: u8) -> Result<Vec<u8>, BindError> {
        if self.step != (Step::Flag { sent: false }) {
            return Err(out_of_order("the startup-duration write"));
        }
        let value = match self.kind {
            Kind::Upload => DURATION_UPLOAD,
            Kind::Remove => DURATION_REMOVE,
        };
        let request = logo_flag_request(id, value);
        guard::check_logo_flag_write(&request)
            .map_err(|e| BindError::new("forbidden", e.to_string()))?;
        self.step = Step::Flag { sent: true };
        Ok(request)
    }

    fn flag_error(&self, reason: &str) -> BindError {
        match self.kind {
            Kind::Upload => BindError::with_hint(
                "protocol",
                format!("The logo was written but its flag write failed: {reason}."),
                UPLOAD_HINT,
            ),
            Kind::Remove => remove_error(LogoRemoveStep::Flag, reason),
        }
    }

    /// Take the startup-duration write's reply: FINISHED or ACCEPT goes on
    /// to the read-back.
    pub fn take_flag_reply(&mut self, reply: &[u8]) -> Result<(), BindError> {
        if self.step != (Step::Flag { sent: true }) {
            return Err(out_of_order("the startup-duration reply"));
        }
        if let Err(reason) = write_outcome(reply, true) {
            self.step = Step::Over;
            return Err(self.flag_error(&reason));
        }
        self.step = Step::ReadBack;
        Ok(())
    }

    /// The read-back reads still due, from the window start: none outside
    /// the read-back.
    pub fn read_plan(&self) -> Vec<Chunk> {
        if self.step == Step::ReadBack {
            window_plan(self.read.len(), self.data.len())
        } else {
            Vec::new()
        }
    }

    /// A memory read of `len` bytes at `addr` with request id `id`:
    /// `forbidden` outside the logo window, `invalid` unless it is the next
    /// planned read-back read.
    pub fn read_request(&mut self, id: u8, addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
        check_window_read(addr, len)?;
        let next = self.read_plan().into_iter().next();
        if next != Some(Chunk { addr, len }) {
            return Err(out_of_order("that read"));
        }
        Ok(session::memory_read_request(id, addr, len))
    }

    /// Take the read-back data read at `addr`; after the last read the
    /// read-back is compared byte for byte with what was written.
    pub fn take_read(&mut self, addr: u32, data: &[u8]) -> Result<(), BindError> {
        if self.step != Step::ReadBack {
            return Err(out_of_order("a read-back read"));
        }
        if let Err(e) = take_window_read(&mut self.read, self.data.len(), addr, data, "that read") {
            self.step = Step::Over;
            return Err(e);
        }
        if self.read.len() < self.data.len() {
            return Ok(());
        }
        self.step = Step::Over;
        let read = &self.read;
        match self.kind {
            Kind::Upload => {
                if let Some(offset) = read.iter().zip(&self.data).position(|(a, b)| a != b) {
                    return Err(mismatch(offset));
                }
                if read.len() != self.data.len() {
                    return Err(mismatch(read.len().min(self.data.len())));
                }
            }
            Kind::Remove => {
                if read.len() != REMOVE_LEN || read.iter().any(|&b| b != 0xFF) {
                    return Err(remove_error(
                        LogoRemoveStep::ReadBack,
                        &format!("expected {REMOVE_LEN} bytes of 0xFF, read {read:02X?}"),
                    ));
                }
            }
        }
        self.step = Step::Settings { sent: false };
        Ok(())
    }

    /// The step that failed with `reason` off the meter (no reply, a
    /// transfer error) ends the write; `Err` with the desktop wording when
    /// the desktop words that step's failure itself, `Ok` when the caller's
    /// own error stands.
    pub fn fail(&mut self, reason: &str) -> Result<(), BindError> {
        let step = std::mem::replace(&mut self.step, Step::Over);
        match (step, self.kind) {
            (Step::Chunks { next, .. }, _) => Err(self.chunk_error(next, reason)),
            (Step::Flag { .. }, _) => Err(self.flag_error(reason)),
            (Step::ReadBack, Kind::Remove) => Err(remove_error(LogoRemoveStep::ReadBack, reason)),
            (Step::Settings { .. }, _) => Err(reread_error()),
            _ => Ok(()),
        }
    }

    /// GET_DATA SETTINGS with request id `id`: the one read after a
    /// verified write.
    pub fn settings_request(&mut self, id: u8) -> Result<Vec<u8>, BindError> {
        if self.step != (Step::Settings { sent: false }) {
            return Err(out_of_order("the SETTINGS read"));
        }
        self.step = Step::Settings { sent: true };
        Ok(settings_request(id))
    }

    /// Take the SETTINGS reply after the write: what the write left on the
    /// meter. A failed re-read is the desktop re-read error.
    pub fn take_settings(&mut self, reply: &[u8]) -> Result<LogoWrittenView, BindError> {
        if self.step != (Step::Settings { sent: true }) {
            return Err(out_of_order("the SETTINGS reply"));
        }
        self.step = Step::Over;
        let Some((duration_before, calibration)) = self.before else {
            return Err(out_of_order("the SETTINGS reply"));
        };
        let save = settings_block(reply)
            .ok()
            .and_then(|block| settings_save(&block, &calibration).ok())
            .ok_or_else(reread_error)?;
        self.pending = (save == SettingsSave::NeedsMenuSave).then_some(calibration);
        self.step = Step::Done;
        let (settings, startup_duration) = settings_state(save);
        let duration_set = match self.kind {
            Kind::Upload => StartupLogo::Secs3,
            Kind::Remove => StartupLogo::Off,
        };
        Ok(LogoWrittenView {
            duration_before: duration_before.label().to_owned(),
            duration_set: duration_set.label().to_owned(),
            settings,
            startup_duration,
        })
    }

    /// The calibration snapshot to keep for the settings check while the
    /// written settings wait for a menu save.
    pub fn pending_snapshot(&self) -> Option<Vec<u8>> {
        self.pending.map(|c| c.0.to_vec())
    }
}

#[wasm_bindgen]
impl LogoWrite {
    /// An upload of a picture file's `bytes` (a raw logo file when
    /// `is_bin`), opaque or transparent with `alpha`; no picture is `usage`.
    pub fn upload(bytes: &[u8], is_bin: bool, alpha: bool) -> Result<LogoWrite, BindError> {
        let blob = blob_from_bytes(bytes, is_bin, alpha)?;
        Ok(LogoWrite::new(Kind::Upload, blob.padded()))
    }

    /// An upload of built-in logo `name`.
    #[wasm_bindgen(js_name = uploadBuiltin)]
    pub fn upload_builtin(name: &str) -> Result<LogoWrite, BindError> {
        let blob = blob_from_builtin(name)?;
        Ok(LogoWrite::new(Kind::Upload, blob.padded()))
    }

    /// A Remove: 16 bytes of 0xFF at the region start.
    pub fn remove() -> LogoWrite {
        LogoWrite::new(Kind::Remove, vec![0xFF; REMOVE_LEN])
    }

    /// [`preconditions`](Self::preconditions) for JS.
    #[wasm_bindgen(js_name = preconditions)]
    pub fn preconditions_js(
        &mut self,
        auth_level: u8,
        settings_reply: Option<Vec<u8>>,
        model: Option<String>,
    ) -> Result<(), BindError> {
        self.preconditions(auth_level, settings_reply.as_deref(), model.as_deref())
    }

    /// [`chunk_count`](Self::chunk_count) for JS.
    #[wasm_bindgen(js_name = chunkCount)]
    pub fn chunk_count_js(&self) -> u32 {
        self.chunk_count()
    }

    /// [`chunk_request`](Self::chunk_request) for JS.
    #[wasm_bindgen(js_name = chunkRequest)]
    pub fn chunk_request_js(&mut self, id: u8, k: u32) -> Result<Vec<u8>, BindError> {
        self.chunk_request(id, k)
    }

    /// [`take_chunk_reply`](Self::take_chunk_reply) for JS.
    #[wasm_bindgen(js_name = takeChunkReply)]
    pub fn take_chunk_reply_js(&mut self, k: u32, reply: &[u8]) -> Result<(), BindError> {
        self.take_chunk_reply(k, reply)
    }

    /// [`flag_request`](Self::flag_request) for JS.
    #[wasm_bindgen(js_name = flagRequest)]
    pub fn flag_request_js(&mut self, id: u8) -> Result<Vec<u8>, BindError> {
        self.flag_request(id)
    }

    /// [`take_flag_reply`](Self::take_flag_reply) for JS.
    #[wasm_bindgen(js_name = takeFlagReply)]
    pub fn take_flag_reply_js(&mut self, reply: &[u8]) -> Result<(), BindError> {
        self.take_flag_reply(reply)
    }

    /// [`read_plan`](Self::read_plan) for JS: `[{addr, len}]`.
    #[wasm_bindgen(js_name = readPlan)]
    pub fn read_plan_js(&self) -> Result<JsValue, BindError> {
        to_js(&self.read_plan())
    }

    /// [`read_request`](Self::read_request) for JS.
    #[wasm_bindgen(js_name = readRequest)]
    pub fn read_request_js(&mut self, id: u8, addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
        self.read_request(id, addr, len)
    }

    /// [`take_read`](Self::take_read) for JS.
    #[wasm_bindgen(js_name = takeRead)]
    pub fn take_read_js(&mut self, addr: u32, data: &[u8]) -> Result<(), BindError> {
        self.take_read(addr, data)
    }

    /// [`fail`](Self::fail) for JS.
    #[wasm_bindgen(js_name = fail)]
    pub fn fail_js(&mut self, reason: &str) -> Result<(), BindError> {
        self.fail(reason)
    }

    /// [`settings_request`](Self::settings_request) for JS.
    #[wasm_bindgen(js_name = settingsRequest)]
    pub fn settings_request_js(&mut self, id: u8) -> Result<Vec<u8>, BindError> {
        self.settings_request(id)
    }

    /// [`take_settings`](Self::take_settings) for JS.
    #[wasm_bindgen(js_name = takeSettings)]
    pub fn take_settings_js(&mut self, reply: &[u8]) -> Result<JsValue, BindError> {
        to_js_view(&self.take_settings(reply)?)
    }

    /// [`pending_snapshot`](Self::pending_snapshot) for JS.
    #[wasm_bindgen(js_name = pendingSnapshot)]
    pub fn pending_snapshot_js(&self) -> Option<Vec<u8>> {
        self.pending_snapshot()
    }
}

fn remove_error(step: LogoRemoveStep, reason: &str) -> BindError {
    BindError::with_hint(
        "protocol",
        format!("Removing the logo failed at the {step} step: {reason}."),
        REMOVE_HINT,
    )
}

fn mismatch(offset: usize) -> BindError {
    BindError::with_hint(
        "protocol",
        format!(
            "The logo read back from the meter differs from the one written, at offset {offset:#x}."
        ),
        UPLOAD_HINT,
    )
}

fn reread_error() -> BindError {
    BindError::with_hint("protocol", REREAD, MENU_SAVE_STEPS)
}

/// `Ok` when `reply` is FINISHED (or ACCEPT, if `accept_ok`), else a short
/// reason, as the native logo write words it.
fn write_outcome(reply: &[u8], accept_ok: bool) -> Result<(), String> {
    match frame::classify_reply(reply) {
        Ok(Reply::Finished(_)) => Ok(()),
        Ok(Reply::Accept(_)) if accept_ok => Ok(()),
        Ok(Reply::Reject(_)) => Err("REJECT".into()),
        Ok(other) => Err(format!("unexpected reply {other:?}")),
        Err(e) => Err(e.to_string()),
    }
}

// ---- reading the logo (AC-W8SHXB.5) ----

/// What the logo area holds: the head read (16 bytes at the window start),
/// then, for a full read of a stored logo, the whole padded logo from the
/// window start (the head is not read again).
#[wasm_bindgen]
#[derive(Debug)]
pub struct LogoReader {
    full: bool,
    head: Option<Vec<u8>>,
    /// Bytes of the full read, once the head shows a logo.
    total: usize,
    data: Vec<u8>,
}

impl LogoReader {
    /// The reads still due.
    pub fn read_plan(&self) -> Vec<Chunk> {
        match &self.head {
            None => window_plan(0, HEADER_LEN),
            Some(_) => window_plan(self.data.len(), self.total),
        }
    }

    /// A memory read of `len` bytes at `addr`: `forbidden` outside the logo
    /// window, `invalid` unless it is the next planned read.
    pub fn read_request(&mut self, id: u8, addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
        check_window_read(addr, len)?;
        if self.read_plan().into_iter().next() != Some(Chunk { addr, len }) {
            return Err(BindError::invalid(
                "that read is out of order in a logo read",
            ));
        }
        Ok(session::memory_read_request(id, addr, len))
    }

    /// Take the data read at `addr`.
    pub fn take_read(&mut self, addr: u32, data: &[u8]) -> Result<(), BindError> {
        const WHAT: &str = "that read";
        if self.head.is_none() {
            let mut head = Vec::new();
            take_window_read(&mut head, HEADER_LEN, addr, data, WHAT)?;
            if self.full
                && let Ok(h) = logo::parse_header(&head)
                && !is_erased(&head)
            {
                self.total = logo::padded_len(HEADER_LEN + h.byte_count as usize);
            }
            self.head = Some(head);
            return Ok(());
        }
        take_window_read(&mut self.data, self.total, addr, data, WHAT)
    }

    /// What the logo area holds; after every planned read.
    pub fn result(&self) -> Result<LogoReadView, BindError> {
        let Some(head) = &self.head else {
            return Err(BindError::invalid(
                "the logo read has not read the head yet",
            ));
        };
        if self.data.len() < self.total {
            return Err(BindError::invalid("the logo read is not complete yet"));
        }
        let empty = |status: &str| LogoReadView {
            status: status.to_owned(),
            width: None,
            height: None,
            mode: None,
            image: None,
        };
        if is_erased(head) {
            return Ok(empty("empty"));
        }
        let Ok(h) = logo::parse_header(head) else {
            return Ok(empty("unrecognised"));
        };
        let image = if self.full {
            let px = logo::decode(&self.data)
                .map_err(|e| BindError::new("protocol", format!("logo decode: {e}")))?;
            Some(base64(&px.px))
        } else {
            None
        };
        Ok(LogoReadView {
            status: "logo".to_owned(),
            width: Some(h.width),
            height: Some(h.height),
            mode: Some(mode_name(h.mode).to_owned()),
            image,
        })
    }
}

/// All 0xFF: erased flash. \[inferred\]
fn is_erased(head: &[u8]) -> bool {
    !head.is_empty() && head.iter().all(|&b| b == 0xFF)
}

#[wasm_bindgen]
impl LogoReader {
    /// A read of the logo area; with `full`, the stored picture too.
    #[wasm_bindgen(constructor)]
    pub fn new(full: bool) -> LogoReader {
        LogoReader {
            full,
            head: None,
            total: 0,
            data: Vec::new(),
        }
    }

    /// [`read_plan`](Self::read_plan) for JS: `[{addr, len}]`.
    #[wasm_bindgen(js_name = readPlan)]
    pub fn read_plan_js(&self) -> Result<JsValue, BindError> {
        to_js(&self.read_plan())
    }

    /// [`read_request`](Self::read_request) for JS.
    #[wasm_bindgen(js_name = readRequest)]
    pub fn read_request_js(&mut self, id: u8, addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
        self.read_request(id, addr, len)
    }

    /// [`take_read`](Self::take_read) for JS.
    #[wasm_bindgen(js_name = takeRead)]
    pub fn take_read_js(&mut self, addr: u32, data: &[u8]) -> Result<(), BindError> {
        self.take_read(addr, data)
    }

    /// [`result`](Self::result) for JS.
    #[wasm_bindgen(js_name = result)]
    pub fn result_js(&self) -> Result<JsValue, BindError> {
        to_js_view(&self.result()?)
    }
}
