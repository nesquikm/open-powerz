//! FR TZK5K4: logo upload, Remove and read in the session, end to end
//! against the simulated meter through the guard.
//!
//! AC-TZK5K4.1 chunks and stop on failure; AC-TZK5K4.2 flag 2 then a paced
//! read-back; AC-TZK5K4.3 Remove; AC-TZK5K4.4 read; AC-TZK5K4.5 refusals;
//! AC-TZK5K4.6 progress, no cancel; AC-TZK5K4.7 startup duration in the
//! result; AC-TZK5K4.8 startup duration as a settings field; AC-TZK5K4.9 the
//! simulator cases (in every failure no flag write is sent).

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::settings::{StartupLogo, WritableField, settings_write_request};
use powerz_protocol::logo::{self, LogoBlob, LogoMode, Rgba};
use powerz_protocol::session::{
    Error, LogoProgress, LogoRefusal, LogoRemoveStep, LogoStatus, LogoStep, LogoWritten,
    logo_chunk_plaintext,
};
use powerz_protocol::transport::guard::LOGO_REGION_START;
use powerz_protocol::transport::{Error as TError, GuardedTransport, Transport};
use powerz_sim::{ErasePolicy, LogoFault};

const BASE: u32 = LOGO_REGION_START;
const WINDOW: u32 = 0x9830_0000;
const WINDOW_END: u32 = 0x9832_0000;

// ---- a recording pipe between the guard and the simulated meter ----

/// One frame as it went on the wire.
#[derive(Debug, Clone)]
struct Sent {
    at_ms: u64,
    frame: Vec<u8>,
    timeout: Duration,
}

type Log = Rc<RefCell<Vec<Sent>>>;

/// Records every frame (with the sim time and timeout), then hands it on.
struct Rec {
    dev: SimDevice,
    clock: SimClock,
    log: Log,
}

impl Transport for Rec {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TError> {
        self.log.borrow_mut().push(Sent {
            at_ms: self.clock.now_ms(),
            frame: request.to_vec(),
            timeout,
        });
        self.dev.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TError> {
        self.dev.read(timeout)
    }
}

type LogoSession = powerz_protocol::session::Session<GuardedTransport<Rec>, SimClock>;

/// The signature of `Session::logo_upload` on [`LogoSession`].
type UploadFn =
    fn(&mut LogoSession, &LogoBlob, &mut dyn FnMut(LogoProgress)) -> Result<LogoWritten, Error>;

struct Bench {
    s: LogoSession,
    log: Log,
    /// Log length when the action under test starts.
    mark: usize,
}

impl Bench {
    fn dev(&self) -> &SimDevice {
        &self.s.transport().inner().dev
    }

    /// Frames sent since [`Bench::start`].
    fn sent(&self) -> Vec<Sent> {
        self.log.borrow()[self.mark..].to_vec()
    }

    fn start(&mut self) {
        self.mark = self.log.borrow().len();
    }
}

fn bench_over(dev: SimDevice, clock: &SimClock) -> Bench {
    let log: Log = Rc::default();
    let rec = Rec {
        dev,
        clock: clock.clone(),
        log: Rc::clone(&log),
    };
    let s =
        powerz_protocol::session::Session::with_clock(GuardedTransport::new(rec), clock.clone());
    Bench { s, log, mark: 0 }
}

/// Which of the preconditions to satisfy.
#[derive(Clone, Copy)]
struct Steps {
    auth: bool,
    settings: bool,
    model: bool,
}

const ALL_STEPS: Steps = Steps {
    auth: true,
    settings: true,
    model: true,
};

/// Bulk meter built by `f`, CONNECT sent, then the chosen steps.
fn bench_with(f: impl FnOnce(SimDevice) -> SimDevice, steps: Steps) -> Bench {
    let clock = clock();
    let dev = f(device(Persona::Bulk, Scenario::Idle, &clock));
    let mut b = bench_over(dev, &clock);
    b.s.connect().expect("CONNECT");
    prepare(&mut b, steps);
    b.start();
    b
}

fn prepare(b: &mut Bench, steps: Steps) {
    if steps.auth {
        assert_eq!(b.s.authenticate(1), Ok(1), "level-1 auth");
    }
    if steps.settings {
        b.s.read_settings().expect("SETTINGS read");
    }
    if steps.model {
        b.s.device_info().expect("device info");
    }
}

fn ready(f: impl FnOnce(SimDevice) -> SimDevice) -> Bench {
    bench_with(f, ALL_STEPS)
}

// ---- frame classifiers ----

fn frame_ty(f: &[u8]) -> u8 {
    f[0] & 0x7F
}

/// `(addr, data)` of an encrypted 0x4A chunk frame.
fn chunk_of(f: &[u8]) -> Option<(u32, Vec<u8>)> {
    if frame_ty(f) != 0x4A {
        return None;
    }
    let plain = crypto::decrypt(&KEYS[0], &f[4..]);
    let addr = u32::from_le_bytes(plain[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(plain[4..8].try_into().unwrap()) as usize;
    Some((addr, plain[16..16 + len].to_vec()))
}

/// Value of a logo flag write (SET_DATA attribute 0x1C).
fn flag_of(f: &[u8]) -> Option<u32> {
    if frame_ty(f) != 0x48 || f.len() != 12 {
        return None;
    }
    let ext = u32::from_le_bytes(f[4..8].try_into().unwrap());
    (ext & 0x7FFF == 0x1C).then(|| u32::from_le_bytes(f[8..12].try_into().unwrap()))
}

/// `(addr, len)` of a memory read inside the logo window.
fn window_read_of(f: &[u8]) -> Option<(u32, u32)> {
    if frame_ty(f) != 0x44 || f.len() < 36 {
        return None;
    }
    let plain = crypto::decrypt(&KEYS[0], &f[4..36]);
    let addr = u32::from_le_bytes(plain[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(plain[4..8].try_into().unwrap());
    (WINDOW..WINDOW_END).contains(&addr).then_some((addr, len))
}

fn chunks_sent(sent: &[Sent]) -> Vec<(u32, Vec<u8>)> {
    sent.iter().filter_map(|s| chunk_of(&s.frame)).collect()
}

fn flags_sent(sent: &[Sent]) -> Vec<u32> {
    sent.iter().filter_map(|s| flag_of(&s.frame)).collect()
}

fn window_reads(sent: &[Sent]) -> Vec<(u64, u32, u32)> {
    sent.iter()
        .filter_map(|s| window_read_of(&s.frame).map(|(a, l)| (s.at_ms, a, l)))
        .collect()
}

// ---- logos ----

fn image(w: u32, h: u32, seed: u32) -> Rgba {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3u32 {
                px.push(((x * (3 + seed) + y * (5 + seed) + c * 17 + seed * 31) % 251) as u8);
            }
            px.push(255);
        }
    }
    Rgba { w, h, px }
}

/// 48×48 opaque: 19 chunks.
fn small_blob() -> LogoBlob {
    logo::encode(&image(48, 48, 0), LogoMode::Opaque).unwrap()
}

fn other_blob() -> LogoBlob {
    logo::encode(&image(48, 48, 7), LogoMode::Opaque).unwrap()
}

fn upload(b: &mut Bench, blob: &LogoBlob) -> (Result<LogoWritten, Error>, Vec<LogoProgress>) {
    let mut reports = Vec::new();
    let r = b.s.logo_upload(blob, &mut |p| reports.push(p));
    (r, reports)
}

fn remove(b: &mut Bench) -> (Result<LogoWritten, Error>, Vec<LogoProgress>) {
    let mut reports = Vec::new();
    let r = b.s.logo_remove(&mut |p| reports.push(p));
    (r, reports)
}

/// Every failure: no flag write went out and the meter's flag is unchanged.
fn assert_no_flag_write(b: &Bench, flag_before: u32, what: &str) {
    assert!(
        flags_sent(&b.sent()).is_empty(),
        "{what}: a flag write was sent"
    );
    assert_eq!(b.dev().logo_flag(), flag_before, "{what}: flag changed");
}

// ---- AC-TZK5K4.1 / .2 / .9: a full upload and read-back ----

#[test]
fn tzk5k4_upload_writes_every_chunk_then_the_flag_then_reads_back() {
    let mut b = ready(|d| d);
    let blob = small_blob();
    let padded = blob.padded();
    let (r, _) = upload(&mut b, &blob);
    r.expect("upload succeeds");
    let sent = b.sent();

    // AC.1: 256-byte chunks at 0x300000 + 256·k, each waiting 1000 ms.
    let chunks = chunks_sent(&sent);
    assert_eq!(chunks.len(), padded.len() / 256, "one frame per chunk");
    for (k, (addr, data)) in chunks.iter().enumerate() {
        assert_eq!(*addr, BASE + 256 * k as u32, "chunk {k} address");
        assert_eq!(data, &padded[k * 256..k * 256 + 256], "chunk {k} data");
    }
    for s in sent.iter().filter(|s| frame_ty(&s.frame) == 0x4A) {
        assert_eq!(s.timeout, Duration::from_millis(1000), "chunk timeout");
    }

    // AC.2: flag 2 only after the last chunk, before the first read-back.
    assert_eq!(flags_sent(&sent), vec![2]);
    let last_chunk = sent.iter().rposition(|s| frame_ty(&s.frame) == 0x4A);
    let flag_at = sent.iter().position(|s| flag_of(&s.frame).is_some());
    let first_read = sent.iter().position(|s| window_read_of(&s.frame).is_some());
    assert!(
        last_chunk < flag_at && flag_at < first_read,
        "chunks, flag, read-back"
    );

    // The whole padded blob from 0x98300000, reads of at most 256 bytes,
    // contiguous, at least 50 ms apart.
    let reads = window_reads(&sent);
    let mut next = WINDOW;
    for &(_, addr, len) in &reads {
        assert_eq!(addr, next, "read-back is contiguous");
        assert!((1..=256).contains(&len), "read of {len} bytes");
        next += len;
    }
    assert_eq!((next - WINDOW) as usize, padded.len(), "covers the blob");
    for pair in reads.windows(2) {
        assert!(
            pair[1].0 >= pair[0].0 + 50,
            "reads {} ms apart",
            pair[1].0 - pair[0].0
        );
    }

    assert_eq!(b.dev().logo_flash().read(BASE, padded.len()), padded);
    assert_eq!(b.dev().logo_flag(), 2);
}

#[test]
fn tzk5k4_a_read_back_difference_names_the_first_differing_offset() {
    // Without erase, the second logo ANDs into the first.
    let mut b = ready(|d| d.with_logo_erase(ErasePolicy::NoErase));
    upload(&mut b, &small_blob()).0.expect("first upload");
    let (a, c) = (small_blob().padded(), other_blob().padded());
    let expected = (0..c.len())
        .find(|&i| a[i] & c[i] != c[i])
        .expect("the two logos clash somewhere");
    let (r, _) = upload(&mut b, &other_blob());
    assert_eq!(r, Err(Error::LogoMismatch { offset: expected }));
}

// ---- AC-TZK5K4.1 / .9: stop on the first failed chunk ----

#[test]
fn tzk5k4_a_reject_at_chunk_0_stops_at_once() {
    let mut b = ready(|d| d.with_logo_fault(LogoFault::RejectAt(0)));
    let (r, _) = upload(&mut b, &small_blob());
    match r {
        Err(e @ Error::LogoChunk { index: 0, .. }) => {
            assert!(e.to_string().contains("chunk 0"), "{e}")
        }
        other => panic!("expected a chunk-0 error, got {other:?}"),
    }
    let sent = b.sent();
    assert_eq!(chunks_sent(&sent).len(), 1, "no retry");
    assert!(window_reads(&sent).is_empty(), "no read-back");
    assert_no_flag_write(&b, 0, "REJECT at chunk 0");
    assert_eq!(b.dev().logo_flash().read(BASE, 256), vec![0xFF; 256]);
}

#[test]
fn tzk5k4_no_reply_at_chunk_k_stops_there() {
    let k = 5;
    let mut b = ready(|d| d.with_logo_fault(LogoFault::NoReplyAt(k)));
    let (r, _) = upload(&mut b, &small_blob());
    assert!(
        matches!(r, Err(Error::LogoChunk { index, .. }) if index == k),
        "{r:?}"
    );
    assert_eq!(
        chunks_sent(&b.sent()).len(),
        k + 1,
        "no retry, nothing after"
    );
    assert_no_flag_write(&b, 0, "no reply at chunk k");
}

#[test]
fn tzk5k4_unplug_after_chunk_k_stops_at_the_next_chunk() {
    let k = 5;
    let mut b = ready(|d| d.with_logo_fault(LogoFault::UnplugAfter(k)));
    let (r, _) = upload(&mut b, &small_blob());
    assert!(
        matches!(r, Err(Error::LogoChunk { index, .. }) if index == k + 1),
        "{r:?}"
    );
    assert_eq!(
        chunks_sent(&b.sent()).len(),
        k + 2,
        "nothing after the failure"
    );
    assert_no_flag_write(&b, 0, "unplug after chunk k");
}

// ---- AC-TZK5K4.3 / .9: Remove ----

#[test]
fn tzk5k4_remove_after_a_logo_clears_then_flags_0_then_reads_16_bytes() {
    let mut b = ready(|d| d.with_logo_erase(ErasePolicy::SectorOnFirstWrite));
    upload(&mut b, &small_blob()).0.expect("upload");
    b.start();
    let (r, reports) = remove(&mut b);
    r.expect("remove succeeds");
    let sent = b.sent();
    assert_eq!(chunks_sent(&sent), vec![(BASE, vec![0xFF; 16])]);
    assert_eq!(flags_sent(&sent), vec![0]);
    let reads = window_reads(&sent);
    assert_eq!(
        reads.iter().map(|&(_, a, l)| (a, l)).collect::<Vec<_>>(),
        vec![(WINDOW, 16)]
    );
    let chunk_at = sent.iter().position(|s| chunk_of(&s.frame).is_some());
    let flag_at = sent.iter().position(|s| flag_of(&s.frame).is_some());
    let read_at = sent.iter().position(|s| window_read_of(&s.frame).is_some());
    assert!(
        chunk_at < flag_at && flag_at < read_at,
        "clear, flag, read-back"
    );
    assert_eq!(b.dev().logo_flag(), 0);
    assert_eq!(b.dev().logo_flash().read(BASE, 16), vec![0xFF; 16]);
    assert_eq!(b.s.logo_read(), Ok(LogoStatus::Empty));
    // AC.6: Remove reports progress for its chunk and its read.
    assert!(reports.contains(&LogoProgress {
        step: LogoStep::Chunks,
        done: 1,
        total: 1
    }));
    assert!(reports.contains(&LogoProgress {
        step: LogoStep::ReadBack,
        done: 1,
        total: 1
    }));
}

#[test]
fn tzk5k4_remove_rejected_names_the_clear_step_and_sends_no_flag() {
    let mut b = ready(|d| d.with_logo_flag(2).with_logo_fault(LogoFault::RejectAt(0)));
    let (r, _) = remove(&mut b);
    assert!(
        matches!(
            r,
            Err(Error::LogoRemove {
                step: LogoRemoveStep::Clear,
                ..
            })
        ),
        "{r:?}"
    );
    assert_no_flag_write(&b, 2, "Remove REJECTed");
}

#[test]
fn tzk5k4_remove_whose_read_back_is_not_erased_names_the_read_back_step() {
    // Without erase, 16 bytes of 0xFF change nothing over a logo.
    let mut b = ready(|d| d.with_logo_erase(ErasePolicy::NoErase));
    upload(&mut b, &small_blob()).0.expect("upload");
    let (r, _) = remove(&mut b);
    assert!(
        matches!(
            r,
            Err(Error::LogoRemove {
                step: LogoRemoveStep::ReadBack,
                ..
            })
        ),
        "{r:?}"
    );
}

// ---- AC-TZK5K4.4: read ----

#[test]
fn tzk5k4_read_reports_empty_then_the_logo_and_decodes_it_in_full() {
    let mut b = ready(|d| d);
    assert_eq!(b.s.logo_read(), Ok(LogoStatus::Empty));
    let reads = window_reads(&b.sent());
    assert_eq!(
        reads.iter().map(|&(_, a, l)| (a, l)).collect::<Vec<_>>(),
        vec![(WINDOW, 16)],
        "one 16-byte read at 0x98300000"
    );

    let blob = logo::encode(&image(60, 50, 3), LogoMode::Alpha).unwrap();
    upload(&mut b, &blob).0.expect("upload");
    assert_eq!(
        b.s.logo_read(),
        Ok(LogoStatus::Logo {
            width: 60,
            height: 50,
            mode: LogoMode::Alpha
        })
    );
    assert_eq!(b.s.logo_read_full(), Ok(logo::decode(&blob.bytes).unwrap()));
}

#[test]
fn tzk5k4_read_reports_unrecognised_bytes() {
    let clock = clock();
    let mut dev = raw_connected(Scenario::Idle, &clock);
    // A chunk of garbage at 0x300000 through the raw pipe (no guard).
    let garbage: Vec<u8> = (0..256u32).map(|i| (i * 13 + 1) as u8).collect();
    let plain = logo_chunk_plaintext(0x50, BASE, &garbage);
    let mut wire = plain[..4].to_vec();
    wire.extend(crypto::encrypt(&KEYS[0], &plain[4..]));
    let reply = dev.transact(&wire, T).expect("chunk reply");
    assert_eq!(ty(&reply), 0x07, "FINISHED: {reply:02x?}");
    let mut b = bench_over(dev, &clock);
    b.s.connect().expect("CONNECT");
    assert_eq!(b.s.logo_read(), Ok(LogoStatus::Unrecognised));
}

// ---- AC-TZK5K4.5 / .9: refusals, the auth toggle and the model ----

fn assert_refused(f: impl Fn(SimDevice) -> SimDevice, steps: Steps, want: LogoRefusal) {
    for action in ["upload", "remove"] {
        let mut b = bench_with(|d| f(d).with_logo_flag(2), steps);
        let (r, reports) = if action == "upload" {
            upload(&mut b, &small_blob())
        } else {
            remove(&mut b)
        };
        assert_eq!(r, Err(Error::LogoRefused(want.clone())), "{action}");
        assert!(b.sent().is_empty(), "{action}: nothing is sent");
        assert!(reports.is_empty(), "{action}: no progress");
        assert_no_flag_write(&b, 2, action);
        assert_eq!(b.dev().logo_flash().read(BASE, 256), vec![0xFF; 256]);
    }
}

#[test]
fn tzk5k4_refused_without_auth() {
    let steps = Steps {
        auth: false,
        ..ALL_STEPS
    };
    assert_refused(|d| d, steps, LogoRefusal::NotAuthenticated);
}

#[test]
fn tzk5k4_refused_without_a_settings_read() {
    let steps = Steps {
        settings: false,
        ..ALL_STEPS
    };
    assert_refused(|d| d, steps, LogoRefusal::SettingsNotRead);
}

#[test]
fn tzk5k4_refused_without_a_model_read() {
    let steps = Steps {
        model: false,
        ..ALL_STEPS
    };
    assert_refused(|d| d, steps, LogoRefusal::ModelNotRead);
}

#[test]
fn tzk5k4_refused_on_a_non_km003c_model() {
    let mut b = bench_with(|d| d.with_model("KM002C"), ALL_STEPS);
    assert_eq!(b.s.device_info().map(|i| i.model), Ok("KM002C".to_string()));
    assert_refused(
        |d| d.with_model("KM002C"),
        ALL_STEPS,
        LogoRefusal::WrongModel("KM002C".into()),
    );
    let text = Error::LogoRefused(LogoRefusal::WrongModel("KM002C".into())).to_string();
    assert!(text.contains("KM002C"), "{text}");
    b.start();
    assert!(b.sent().is_empty());
}

#[test]
fn tzk5k4_with_the_auth_toggle_an_authenticated_upload_goes_through() {
    let mut b = ready(|d| d.with_logo_auth_required(true));
    let blob = small_blob();
    upload(&mut b, &blob).0.expect("authenticated upload");
    let padded = blob.padded();
    assert_eq!(b.dev().logo_flash().read(BASE, padded.len()), padded);
    assert_eq!(b.dev().logo_flag(), 2);
}

#[test]
fn tzk5k4_with_the_auth_toggle_an_unauthenticated_upload_sends_nothing() {
    let steps = Steps {
        auth: false,
        ..ALL_STEPS
    };
    assert_refused(
        |d| d.with_logo_auth_required(true),
        steps,
        LogoRefusal::NotAuthenticated,
    );
}

// ---- AC-TZK5K4.6: progress, and no way to cancel ----

#[test]
fn tzk5k4_upload_reports_chunk_then_read_back_progress() {
    let mut b = ready(|d| d);
    let blob = small_blob();
    let n = blob.padded().len() / 256;
    let (r, reports) = upload(&mut b, &blob);
    r.expect("upload");
    let reads = window_reads(&b.sent()).len();
    let of = |step| -> Vec<LogoProgress> {
        reports.iter().copied().filter(|p| p.step == step).collect()
    };
    let (chunks, back) = (of(LogoStep::Chunks), of(LogoStep::ReadBack));
    for k in 1..=n {
        assert!(
            chunks.contains(&LogoProgress {
                step: LogoStep::Chunks,
                done: k,
                total: n
            }),
            "chunk {k}/{n} reported: {chunks:?}"
        );
    }
    for j in 1..=reads {
        assert!(
            back.contains(&LogoProgress {
                step: LogoStep::ReadBack,
                done: j,
                total: reads
            }),
            "read {j}/{reads} reported: {back:?}"
        );
    }
    assert!(chunks.windows(2).all(|w| w[0].done <= w[1].done));
    assert!(back.windows(2).all(|w| w[0].done <= w[1].done));
    let last_chunk = reports.iter().rposition(|p| p.step == LogoStep::Chunks);
    let first_back = reports.iter().position(|p| p.step == LogoStep::ReadBack);
    assert!(last_chunk < first_back, "chunks are reported before reads");
}

#[test]
fn tzk5k4_an_upload_runs_to_the_end_once_its_first_chunk_is_sent() {
    // The progress callback returns nothing: it has no way to stop the
    // upload. Even a caller that "wants out" after chunk 1 gets every chunk,
    // the flag and the read-back.
    let upload_fn: UploadFn = LogoSession::logo_upload;
    let mut b = ready(|d| d);
    let blob = small_blob();
    let mut wants_out = false;
    let r = upload_fn(&mut b.s, &blob, &mut |p| {
        if p.step == LogoStep::Chunks && p.done == 1 {
            wants_out = true;
        }
    });
    assert!(wants_out, "chunk 1 was reported");
    r.expect("the upload completes");
    assert_eq!(chunks_sent(&b.sent()).len(), blob.padded().len() / 256);
    assert_eq!(flags_sent(&b.sent()), vec![2]);
}

// ---- AC-TZK5K4.7: the startup-screen duration in the result ----

#[test]
fn tzk5k4_the_result_carries_the_startup_duration_from_settings() {
    let mut b = ready(|d| d);
    let duration = b.s.read_meter_settings().unwrap().startup_logo.value;
    b.start();
    let up = upload(&mut b, &small_blob()).0.expect("upload");
    assert_eq!(up.startup_duration, duration);
    assert_eq!(up.startup_screen_off, duration == StartupLogo::Off);
    let rm = remove(&mut b).0.expect("remove");
    assert_eq!(rm.startup_duration, duration);
    assert_eq!(rm.startup_screen_off, duration == StartupLogo::Off);
}

/// CDC session (the settings write goes there), authenticated, with the
/// startup duration written to `code` and the model read.
fn cdc_with_duration(code: i32) -> Bench {
    let clock = clock();
    let dev = device(Persona::Cdc, Scenario::Idle, &clock);
    let mut b = bench_over(dev, &clock);
    assert_eq!(b.s.authenticate(1), Ok(1));
    b.s.write_settings(&[(WritableField::StartupDuration, code)])
        .expect("settings write");
    b.s.device_info().expect("device info");
    b.start();
    b
}

#[test]
fn tzk5k4_with_the_startup_screen_off_the_result_says_the_logo_wont_show() {
    let mut b = cdc_with_duration(0);
    let up = upload(&mut b, &small_blob()).0.expect("upload");
    assert_eq!(up.startup_duration, StartupLogo::Off);
    assert!(up.startup_screen_off);

    let mut b = cdc_with_duration(3);
    let up = upload(&mut b, &small_blob()).0.expect("upload");
    assert_eq!(up.startup_duration, StartupLogo::Secs5);
    assert!(!up.startup_screen_off);
}

// ---- AC-TZK5K4.8: the startup duration as a settings field ----

#[test]
fn tzk5k4_startup_duration_writes_and_reads_back_every_code() {
    let clock = clock();
    let mut s = session(device(Persona::Cdc, Scenario::Idle, &clock), &clock);
    for (code, want) in [
        (0, StartupLogo::Off),
        (1, StartupLogo::Secs1),
        (2, StartupLogo::Secs3),
        (3, StartupLogo::Secs5),
    ] {
        let before = s.read_meter_settings().unwrap();
        let after = s
            .write_settings(&[(WritableField::StartupDuration, code)])
            .unwrap_or_else(|e| panic!("code {code}: {e}"));
        assert_eq!(after.startup_logo.value, want, "code {code}");
        // Nothing else moves.
        assert_eq!(after.brightness, before.brightness);
        assert_eq!(after.theme, before.theme);
        assert_eq!(after.language, before.language);
        assert_eq!(after.rotation, before.rotation);
        assert_eq!(after.dim_timeout, before.dim_timeout);
        assert_eq!(after.sample_rate, before.sample_rate);
        assert_eq!(s.read_meter_settings().unwrap().startup_logo.value, want);
    }
    assert!(matches!(
        s.write_settings(&[(WritableField::StartupDuration, 4)]),
        Err(Error::SettingsWrite(_))
    ));
}

#[test]
fn tzk5k4_startup_duration_changes_only_the_0x06_value_of_the_write() {
    let clock = clock();
    let mut s = session(device(Persona::Cdc, Scenario::Idle, &clock), &clock);
    let current = s.read_meter_settings().unwrap();
    let plain = settings_write_request(1, &current, &[]).unwrap();
    let read_code = i32::from_le_bytes(plain[32..36].try_into().unwrap());
    let other = (read_code + 1) % 4;
    let with =
        settings_write_request(1, &current, &[(WritableField::StartupDuration, other)]).unwrap();
    // Entry 3 (key 0x06): key word at 28, value at 32.
    assert_eq!(
        u32::from_le_bytes(with[28..32].try_into().unwrap()) & 0x7FFF,
        0x06
    );
    assert_eq!(i32::from_le_bytes(with[32..36].try_into().unwrap()), other);
    let mut expected = plain.clone();
    expected[32..36].copy_from_slice(&other.to_le_bytes());
    assert_eq!(with, expected, "only the 0x06 value differs");
    // Another field's write still sends 0x06 at its read value.
    let bright = settings_write_request(1, &current, &[(WritableField::Brightness, 77)]).unwrap();
    assert_eq!(&bright[28..36], &plain[28..36]);
}
