//! The firmware flashing engine (FR V9SK54) over a hand-written fake meter.
//!
//! No test here opens a real device. The fake meter ([`Bench`]) answers
//! like the application firmware (CONNECT, HEAD, the 0x4420 info block,
//! the UID read and a 0x4C authentication: synthetic UID, shipped keys)
//! and, in bootloader mode, replays a script of replies, recording every
//! transfer with its timeout. Every container is synthetic, built by the
//! shared test builder; no vendor firmware is read.
//!
//! The wire sequence pinned here is `[static]` (`docs/protocol.md`, research
//! notes): CONNECT (1000 ms) → ACCEPT; HEAD (field must be 0); the 0x42
//! descriptor as a raw write (1000 ms write wait), then a separate 20 ms
//! read that must time out; N−1 reads of 10 ms that must
//! time out and a 1000 ms read that must be ACCEPT; the payload in raw
//! 4080-byte writes each acked by SYNC; FINISHED within 1000 ms.

mod common;
#[path = "support/fw_container.rs"]
mod fw_container;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{FakeClock, SYNTH_UID, UID_ADDR, reply_key};
use fw_container::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::head::HeadReply;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::firmware::flash::{
    self, APP_START, BLOCK_LEN, FlashAck, FlashError, FlashGuard, FlashOutcome, FlashProgress,
    FlashTransport, JumpReply, MAX_APP_END, Meter, MeterOpener, POLL_INTERVAL, RETURN_TIMEOUT,
    SWITCH_TIMEOUT,
};
use powerz_protocol::transport::bulk::BulkTransport;
use powerz_protocol::transport::discovery::{DeviceEntry, Model};
use powerz_protocol::transport::guard::{self, FORBIDDEN_TYPES};
use powerz_protocol::transport::{Error as TransportError, GuardedTransport, Transport};
use proptest::prelude::*;

// ---------------------------------------------------------------- helpers

/// The block size of the data phase, written out independently of the
/// library `[static]`.
const B: usize = 4080;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A synthetic payload of `len` bytes. Its bytes never form a 4-byte
/// JUMP frame, so the fake can tell payload writes from jumps.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 37 + 11) as u8 | 0x80).collect()
}

/// A KM003C 2.0.7 container whose stored payload is `stored` bytes and
/// whose declared size is `size`.
fn container_bytes(stored: usize, size: usize) -> Vec<u8> {
    ContainerBuilder::new()
        .payload(&payload(stored))
        .field(SIZE, &size.to_string())
        .build()
}

fn parse(bytes: &[u8]) -> FirmwareContainer<'_> {
    FirmwareContainer::parse(bytes).expect("a synthetic container parses")
}

/// 3 whole blocks plus a 100-byte remainder: N = 3, four writes.
const P: usize = 3 * B + 100;

fn accept() -> Vec<u8> {
    vec![0x05, 0x00, 0x00, 0x00]
}

fn reject() -> Vec<u8> {
    vec![0x06, 0x00, 0x00, 0x00]
}

fn sync() -> Vec<u8> {
    vec![0x01, 0x00, 0x00, 0x00]
}

fn finished() -> Vec<u8> {
    vec![0x07, 0x00, 0x00, 0x00]
}

fn head(field: u32) -> Vec<u8> {
    let mut f = vec![0x40, 0x00, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

/// A 64-byte info block (0x4420 layout): start, size, CRC, then model,
/// version, date and an extra string at 0x10 / 0x1C / 0x28 / 0x34.
fn info_block(model: &str, version: &str) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0..4].copy_from_slice(&0x4000u32.to_le_bytes());
    b[4..8].copy_from_slice(&0x0002_0000u32.to_le_bytes());
    b[8..12].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    for (off, s) in [
        (0x10, model),
        (0x1C, version),
        (0x28, "2026.9.29"),
        (0x34, "FAKE"),
    ] {
        b[off..off + s.len()].copy_from_slice(s.as_bytes());
    }
    b
}

/// How the fake application firmware answers a 0x4C request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthAnswer {
    /// Echo the plaintext under the reply key, granting this level.
    Grant(u8),
    /// Answer REJECT.
    Reject,
}

/// One transfer the fake saw.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    /// `transact`: a framed request and the reply wait.
    Transact { req: Vec<u8>, timeout: Duration },
    /// `read` with no write.
    Read { timeout: Duration },
    /// `FlashTransport::write_raw`: bytes written with no reply awaited.
    Raw { data: Vec<u8>, timeout: Duration },
    /// `Transport::recover`: clear the halt and drain (no device command).
    Recover,
}

impl Op {
    /// The bytes written, if this op wrote any.
    fn written(&self) -> Option<&[u8]> {
        match self {
            Op::Transact { req, .. } => Some(req),
            Op::Raw { data, .. } => Some(data),
            Op::Read { .. } | Op::Recover => None,
        }
    }
}

/// One scripted bootloader reply, for `transact` or `read` in order.
#[derive(Debug, Clone)]
enum Boot {
    /// This frame; byte 1 becomes the id of the last framed request.
    Frame(Vec<u8>),
    /// This frame exactly as given (byte 1 kept, e.g. id 0).
    Exact(Vec<u8>),
    /// No reply (`Error::Timeout`).
    Timeout,
    /// This transport error.
    Fail(TransportError),
}

#[derive(Debug)]
struct State {
    /// `None` while no meter is attached (re-enumerating).
    mode: Option<DeviceMode>,
    model: Model,
    app_model: String,
    app_version: String,
    /// What the application info block reports after JUMP_APROM.
    new_version: String,
    ops: Vec<Op>,
    /// Index into `ops` where bootloader mode began.
    boot_from: Option<usize>,
    script: VecDeque<Boot>,
    app_pending: VecDeque<Vec<u8>>,
    /// What successive `open` calls see after JUMP_DFU (the last stays).
    after_dfu: Vec<Option<DeviceMode>>,
    /// What successive `open` calls see after JUMP_APROM (the last stays).
    after_aprom: Vec<Option<DeviceMode>>,
    upcoming: VecDeque<Option<DeviceMode>>,
    /// The payload write (1-based) that fails at the USB level.
    raw_fail_at: Option<usize>,
    /// The JUMP_DFU raw write reports this error. `Disconnected` is the
    /// meter resetting as it jumps: the switch still happens.
    dfu_fail: Option<TransportError>,
    /// The JUMP_APROM raw write reports this error. `Disconnected` is the
    /// meter resetting as it jumps: the return still happens.
    aprom_fail: Option<TransportError>,
    /// The raw descriptor write reports this error.
    desc_fail: Option<TransportError>,
    payload_writes: usize,
    opens: usize,
    last_id: u8,
    /// The stop flag handed to the engine (stands in for Ctrl-C).
    stop: Rc<AtomicBool>,
    /// Set the stop flag once this many payload writes have been made.
    stop_after_writes: Option<usize>,
    /// The bench clock; each `open` advances it by `open_cost_ms`.
    clock: FakeClock,
    open_cost_ms: u64,
    /// Clock time of each jump write: (frame type, now_ms).
    jump_at: Vec<(u8, u64)>,
    /// After a jump of this frame type the meter obeys, this many `open`
    /// calls fail with this error (an ambiguous device list, an I/O error)
    /// before the device list is read again; `usize::MAX`: every one.
    open_err_after: Option<(u8, TransportError, usize)>,
    /// The armed opener error and how many opens it has left.
    open_errs: Option<(TransportError, usize)>,
    /// How the application firmware answers 0x4C (default: level 1).
    auth: AuthAnswer,
}

impl State {
    /// Arm the opener error for a jump of type `ty` the meter obeyed.
    fn arm_open_err(&mut self, ty: u8) {
        if let Some((t, e, n)) = &self.open_err_after
            && *t == ty
        {
            self.open_errs = Some((e.clone(), *n));
        }
    }
}

impl State {
    fn set_mode(&mut self, mode: Option<DeviceMode>) {
        if mode == Some(DeviceMode::Bootloader) && self.boot_from.is_none() {
            self.boot_from = Some(self.ops.len());
        }
        self.mode = mode;
    }

    fn next_boot(&mut self) -> Result<Vec<u8>, TransportError> {
        match self.script.pop_front() {
            Some(Boot::Frame(mut f)) => {
                if f.len() >= 2 {
                    f[1] = self.last_id;
                }
                Ok(f)
            }
            Some(Boot::Exact(f)) => Ok(f),
            Some(Boot::Timeout) | None => Err(TransportError::Timeout),
            Some(Boot::Fail(e)) => Err(e),
        }
    }

    /// The application firmware's answer to `req`.
    fn app_reply(&mut self, req: &[u8]) -> Result<Vec<u8>, TransportError> {
        let id = req.get(1).copied().unwrap_or(0);
        let mut reply = match req.first().map(|b| b & 0x7F) {
            Some(0x02) => accept(),
            Some(0x40) => head(0),
            Some(0x44) if req.len() >= 36 => {
                let plain = crypto::decrypt(&KEYS[0], &req[4..36]);
                let word = |o: usize| u32::from_le_bytes(plain[o..o + 4].try_into().unwrap());
                let data = match (word(0), word(4)) {
                    (0x4420, 64) => Some(info_block(&self.app_model, &self.app_version)),
                    (UID_ADDR, 12) => Some(SYNTH_UID.to_vec()),
                    _ => None,
                };
                if let Some(block) = data {
                    self.app_pending
                        .push_back(crypto::encrypt(&KEYS[0], &block));
                    let mut c = vec![0xC4, 0x00, 0x01, 0x01];
                    c.extend_from_slice(&plain[..16]);
                    c
                } else {
                    reject()
                }
            }
            Some(0x4C) if req.len() == 36 => match self.auth {
                AuthAnswer::Reject => reject(),
                AuthAnswer::Grant(level) => {
                    // `4C 00 xx 02`: id 0, bit 16 set, level in bits 17..18;
                    // the meter echoes the plaintext under the reply key.
                    let plain = crypto::decrypt(&KEYS[3], &req[4..]);
                    let word: u32 = 0x4C | (1 << 16) | (u32::from(level) << 17) | (8 << 22);
                    let mut reply = word.to_le_bytes().to_vec();
                    reply.extend(crypto::encrypt(&reply_key(), &plain));
                    return Ok(reply);
                }
            },
            _ => reject(),
        };
        reply[1] = id;
        Ok(reply)
    }
}

/// The fake meter's vendor bulk pipe.
#[derive(Debug, Clone)]
struct Pipe(Rc<RefCell<State>>);

impl Transport for Pipe {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Transact {
            req: request.to_vec(),
            timeout,
        });
        if let Some(&id) = request.get(1) {
            s.last_id = id;
        }
        match s.mode {
            None => Err(TransportError::Disconnected),
            Some(DeviceMode::Bootloader) => s.next_boot(),
            Some(_) => s.app_reply(request),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Read { timeout });
        match s.mode {
            None => Err(TransportError::Disconnected),
            Some(DeviceMode::Bootloader) => s.next_boot(),
            Some(_) => s.app_pending.pop_front().ok_or(TransportError::Timeout),
        }
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        self.0.borrow_mut().ops.push(Op::Recover);
        Ok(())
    }
}

/// Whether a jump write with `fail` still switches the meter: only a clean
/// write or the reset-as-it-jumps `Disconnected`.
fn jump_happens(fail: &Option<TransportError>) -> bool {
    matches!(fail, None | Some(TransportError::Disconnected))
}

impl FlashTransport for Pipe {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Raw {
            data: data.to_vec(),
            timeout,
        });
        if let Some(&id) = data.get(1) {
            s.last_id = id;
        }
        let jump = (data.len() == 4).then(|| data[0] & 0x7F);
        let descriptor = data.len() == 4 + 64 && data[0] == 0x42;
        if let Some(ty @ (0x08 | 0x09)) = jump {
            let now = s.clock.0.borrow().now_ms;
            s.jump_at.push((ty, now));
        }
        match (s.mode, jump) {
            (None, _) => Err(TransportError::Disconnected),
            (Some(DeviceMode::Application), Some(0x09)) => {
                let fail = s.dfu_fail.clone();
                if jump_happens(&fail) {
                    s.upcoming = s.after_dfu.clone().into();
                    s.arm_open_err(0x09);
                }
                fail.map_or(Ok(()), Err)
            }
            (Some(DeviceMode::Bootloader), Some(0x08)) => {
                let fail = s.aprom_fail.clone();
                if jump_happens(&fail) {
                    s.app_version = s.new_version.clone();
                    s.upcoming = s.after_aprom.clone().into();
                    s.arm_open_err(0x08);
                }
                fail.map_or(Ok(()), Err)
            }
            (Some(DeviceMode::Bootloader), _) if descriptor => {
                s.desc_fail.clone().map_or(Ok(()), Err)
            }
            (Some(DeviceMode::Bootloader), _) => {
                s.payload_writes += 1;
                if s.stop_after_writes == Some(s.payload_writes) {
                    s.stop.store(true, Ordering::SeqCst);
                }
                if s.raw_fail_at == Some(s.payload_writes) {
                    Err(TransportError::Disconnected)
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

/// Opens the fake meter as the current device list shows it.
#[derive(Debug)]
struct Opener(Rc<RefCell<State>>);

impl MeterOpener for Opener {
    type Pipe = Pipe;

    fn open(&mut self) -> Result<Option<Meter<Pipe>>, TransportError> {
        let mut s = self.0.borrow_mut();
        s.opens += 1;
        let cost = s.open_cost_ms;
        s.clock.0.borrow_mut().now_ms += cost;
        if let Some((e, left)) = s.open_errs.take() {
            if left > 1 {
                s.open_errs = Some((e.clone(), left - 1));
            }
            return Err(e);
        }
        if let Some(next) = s.upcoming.pop_front() {
            s.set_mode(next);
        }
        let model = s.model;
        Ok(s.mode.map(|mode| Meter {
            mode,
            model,
            pipe: Pipe(self.0.clone()),
        }))
    }
}

/// A fake meter plus its clock.
struct Bench {
    s: Rc<RefCell<State>>,
    clock: FakeClock,
}

impl Bench {
    fn new(mode: DeviceMode) -> Bench {
        let clock = FakeClock::new(1_790_000_000_000);
        let mut state = State {
            mode: None,
            model: Model::Km003c,
            app_model: "KM003C".into(),
            app_version: "2.0.6".into(),
            new_version: "2.0.7".into(),
            ops: Vec::new(),
            boot_from: None,
            script: VecDeque::new(),
            app_pending: VecDeque::new(),
            after_dfu: vec![None, Some(DeviceMode::Bootloader)],
            after_aprom: vec![None, Some(DeviceMode::Application)],
            upcoming: VecDeque::new(),
            raw_fail_at: None,
            dfu_fail: None,
            aprom_fail: None,
            desc_fail: None,
            payload_writes: 0,
            opens: 0,
            last_id: 0,
            stop: Rc::new(AtomicBool::new(false)),
            stop_after_writes: None,
            clock: clock.clone(),
            open_cost_ms: 0,
            jump_at: Vec::new(),
            open_err_after: None,
            open_errs: None,
            auth: AuthAnswer::Grant(1),
        };
        state.set_mode(Some(mode));
        Bench {
            s: Rc::new(RefCell::new(state)),
            clock,
        }
    }

    /// Starts in application mode.
    fn app() -> Bench {
        Bench::new(DeviceMode::Application)
    }

    /// Starts in bootloader mode.
    fn boot() -> Bench {
        Bench::new(DeviceMode::Bootloader)
    }

    fn script(self, script: Vec<Boot>) -> Bench {
        self.s.borrow_mut().script = script.into();
        self
    }

    fn after_dfu(self, seen: Vec<Option<DeviceMode>>) -> Bench {
        self.s.borrow_mut().after_dfu = seen;
        self
    }

    fn after_aprom(self, seen: Vec<Option<DeviceMode>>) -> Bench {
        self.s.borrow_mut().after_aprom = seen;
        self
    }

    fn model(self, model: Model, app_model: &str) -> Bench {
        {
            let mut s = self.s.borrow_mut();
            s.model = model;
            s.app_model = app_model.into();
        }
        self
    }

    fn raw_fail_at(self, write: usize) -> Bench {
        self.s.borrow_mut().raw_fail_at = Some(write);
        self
    }

    /// The version the info block reports after JUMP_APROM.
    fn new_version(self, version: &str) -> Bench {
        self.s.borrow_mut().new_version = version.into();
        self
    }

    /// Make the JUMP_APROM raw write report `e`.
    fn aprom_fail(self, e: TransportError) -> Bench {
        self.s.borrow_mut().aprom_fail = Some(e);
        self
    }

    /// Make the JUMP_DFU raw write report `e`.
    fn dfu_fail(self, e: TransportError) -> Bench {
        self.s.borrow_mut().dfu_fail = Some(e);
        self
    }

    /// Make the raw descriptor write report `e`.
    fn desc_fail(self, e: TransportError) -> Bench {
        self.s.borrow_mut().desc_fail = Some(e);
        self
    }

    /// Set the stop flag (Ctrl-C) once `k` payload writes have been made.
    fn stop_after_writes(self, k: usize) -> Bench {
        self.s.borrow_mut().stop_after_writes = Some(k);
        self
    }

    /// Set the stop flag (Ctrl-C) before the flash starts.
    fn stopped(self) -> Bench {
        self.s.borrow().stop.store(true, Ordering::SeqCst);
        self
    }

    /// After the jump of type `ty`, `times` opens fail with `e`
    /// (`usize::MAX`: all of them).
    fn open_err_after(self, ty: u8, e: TransportError, times: usize) -> Bench {
        self.s.borrow_mut().open_err_after = Some((ty, e, times));
        self
    }

    /// How the application firmware answers 0x4C.
    fn auth(self, answer: AuthAnswer) -> Bench {
        self.s.borrow_mut().auth = answer;
        self
    }

    /// Every `open` advances the clock by `ms` (enumeration is slow).
    fn open_cost(self, ms: u64) -> Bench {
        self.s.borrow_mut().open_cost_ms = ms;
        self
    }

    /// Clock time of the first jump of type `ty`.
    fn jump_at(&self, ty: u8) -> u64 {
        self.s
            .borrow()
            .jump_at
            .iter()
            .find(|(t, _)| *t == ty)
            .map(|(_, at)| *at)
            .unwrap_or_else(|| panic!("no jump {ty:#04x} was sent"))
    }

    fn now_ms(&self) -> u64 {
        self.clock.0.borrow().now_ms
    }

    /// Run the flash engine on `c`; return its result and the progress.
    fn run(
        &self,
        c: &FirmwareContainer<'_>,
    ) -> (Result<FlashOutcome, FlashError>, Vec<FlashProgress>) {
        let mut opener = Opener(self.s.clone());
        let mut events = Vec::new();
        let stop = self.s.borrow().stop.clone();
        let r = flash::flash(
            FlashAck::acknowledge(),
            c,
            &mut opener,
            self.clock.clone(),
            &stop,
            &mut |p| events.push(p),
        );
        (r, events)
    }

    fn ops(&self) -> Vec<Op> {
        self.s.borrow().ops.clone()
    }

    /// Ops from the moment the meter was in bootloader mode.
    fn boot_ops(&self) -> Vec<Op> {
        let s = self.s.borrow();
        let from = s.boot_from.expect("the meter reached bootloader mode");
        s.ops[from..].to_vec()
    }

    /// The bytes of every write in bootloader mode, in order.
    fn boot_writes(&self) -> Vec<Vec<u8>> {
        self.boot_ops()
            .iter()
            .filter_map(|o| o.written().map(<[u8]>::to_vec))
            .collect()
    }

    fn mode(&self) -> Option<DeviceMode> {
        self.s.borrow().mode
    }

    fn opens(&self) -> usize {
        self.s.borrow().opens
    }
}

/// The full bootloader script for a `stored`-byte payload.
fn boot_ok(stored: usize) -> Vec<Boot> {
    let n = stored / B;
    let chunks = stored.div_ceil(B);
    let mut v = vec![Boot::Frame(accept()), Boot::Frame(head(0)), Boot::Timeout];
    v.extend((0..n.saturating_sub(1)).map(|_| Boot::Timeout));
    v.push(Boot::Frame(accept()));
    v.extend((0..chunks).map(|_| Boot::Frame(sync())));
    v.push(Boot::Frame(finished()));
    v
}

/// `boot_ok(stored)` with reply `at` replaced by `reply` and nothing after.
fn fail_at(stored: usize, at: usize, reply: Boot) -> Vec<Boot> {
    let mut v = boot_ok(stored);
    v.truncate(at);
    v.push(reply);
    v
}

/// Script indices for the `P`-byte payload (N = 3, four writes).
const AT_CONNECT: usize = 0;
const AT_HEAD: usize = 1;
const AT_DESCRIPTOR: usize = 2;
const AT_ERASE_SHORT: usize = 3;
const AT_ERASE_ACCEPT: usize = 5;
const AT_SYNC_1: usize = 6;
const AT_FINISHED: usize = 10;

/// Every memory-read address the run asked for.
fn memread_addrs(ops: &[Op]) -> Vec<(u32, u32)> {
    ops.iter()
        .filter_map(Op::written)
        .filter(|w| w.first().map(|b| b & 0x7F) == Some(0x44) && w.len() >= 36)
        .map(|w| {
            let p = crypto::decrypt(&KEYS[0], &w[4..36]);
            (
                u32::from_le_bytes(p[0..4].try_into().unwrap()),
                u32::from_le_bytes(p[4..8].try_into().unwrap()),
            )
        })
        .collect()
}

/// Whether a raw write is the 68-byte 0x42 descriptor (payload bytes all
/// have bit 7 set, so they never start with 0x42).
fn is_raw_descriptor(data: &[u8]) -> bool {
    data.len() == 4 + 64 && data[0] == 0x42
}

/// Frame types (bits 0..6) of the framed writes, i.e. `transact`s, 4-byte
/// raw writes (the jumps) and the raw descriptor.
fn framed_types(ops: &[Op]) -> Vec<u8> {
    ops.iter()
        .filter_map(|o| match o {
            Op::Transact { req, .. } => Some(req[0] & 0x7F),
            Op::Raw { data, .. } if data.len() == 4 || is_raw_descriptor(data) => {
                Some(data[0] & 0x7F)
            }
            _ => None,
        })
        .collect()
}

/// The raw payload writes (not jumps, not the descriptor).
fn payload_writes(ops: &[Op]) -> Vec<Vec<u8>> {
    ops.iter()
        .filter_map(|o| match o {
            Op::Raw { data, .. } if data.len() != 4 && !is_raw_descriptor(data) => {
                Some(data.clone())
            }
            _ => None,
        })
        .collect()
}

/// The error message tells the user how to recover: hold Back and plug the
/// meter in to reach the bootloader, then retry.
fn assert_recovery_text(e: &FlashError) {
    let msg = e.to_string();
    assert!(msg.contains("Back"), "no hold-Back hint in: {msg}");
    assert!(
        msg.to_lowercase().contains("plug"),
        "no plug-in hint in: {msg}"
    );
}

/// The progress stages with consecutive `Writing` events folded into one.
fn stages(events: &[FlashProgress]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for e in events {
        let name = match e {
            FlashProgress::Switching => "switching",
            FlashProgress::Erasing => "erasing",
            FlashProgress::Writing { .. } => "writing",
            FlashProgress::Verifying => "verifying",
            FlashProgress::Finished => "finished",
            FlashProgress::Returning => "returning",
            FlashProgress::Replug { .. } => "replug",
        };
        if out.last() != Some(&name) {
            out.push(name);
        }
    }
    out
}

/// A raw 0x42 descriptor frame with the given address fields.
fn descriptor_frame(id: u8, start: u32, end: u32, size: u32) -> Vec<u8> {
    let mut f = vec![0x42, id, 0x00, 0x04];
    let mut d = [0u8; 64];
    d[0..4].copy_from_slice(&start.to_le_bytes());
    d[4..8].copy_from_slice(&end.to_le_bytes());
    d[8..12].copy_from_slice(&size.to_le_bytes());
    d[12..16].copy_from_slice(&0xA175_E4A0u32.to_le_bytes());
    d[0x10..0x16].copy_from_slice(b"KM003C");
    d[0x1C..0x21].copy_from_slice(b"2.0.7");
    f.extend(d);
    f
}

/// A write-capable transport that records everything it is asked to do.
#[derive(Debug, Default)]
struct Recorder {
    ops: Vec<Op>,
}

impl Transport for Recorder {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.ops.push(Op::Transact {
            req: request.to_vec(),
            timeout,
        });
        Err(TransportError::Timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.ops.push(Op::Read { timeout });
        Err(TransportError::Timeout)
    }
}

impl FlashTransport for Recorder {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), TransportError> {
        self.ops.push(Op::Raw {
            data: data.to_vec(),
            timeout,
        });
        Ok(())
    }
}

fn memread(id: u8, addr: u32, len: u32) -> Vec<u8> {
    powerz_protocol::session::memory_read_request(id, addr, len)
}

/// A 0x4C request `4C id 00 02` carrying `plain` under key 3.
fn auth_frame(id: u8, plain: &[u8; 32]) -> Vec<u8> {
    let mut f = vec![0x4C, id, 0x00, 0x02];
    f.extend(crypto::encrypt(&KEYS[3], plain));
    f
}

/// A level-1 0x4C: `u64 now ‖ 12-byte UID ‖ u64 random`, then 0xFF.
fn level1_auth(id: u8) -> Vec<u8> {
    let mut p = [0xFFu8; 32];
    p[0..8].copy_from_slice(&1_790_000_000_000u64.to_le_bytes());
    p[8..20].copy_from_slice(&SYNTH_UID);
    p[20..28].copy_from_slice(&0x0123_4567_89AB_CDEFu64.to_le_bytes());
    auth_frame(id, &p)
}

/// A level-2 0x4C: a 16-byte OTP credential fills the block, no padding.
fn level2_auth(id: u8) -> Vec<u8> {
    let mut p = [0u8; 32];
    p[0..8].copy_from_slice(&1_790_000_000_000u64.to_le_bytes());
    p[8..24].copy_from_slice(b"123456 0f1e2d3c4");
    p[24..32].copy_from_slice(&0x0123_4567_89AB_CDEFu64.to_le_bytes());
    auth_frame(id, &p)
}

// ---- AC-V9SK54.1: JUMP_DFU from application mode, then bootloader ----

#[test]
fn from_application_mode_sends_jump_dfu_write_only_then_flashes_in_bootloader() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
    let ops = bench.ops();
    let dfu: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(
            |(_, o)| matches!(o, Op::Raw { data, .. } if data.len() == 4 && data[0] & 0x7F == 0x09),
        )
        .map(|(i, _)| i)
        .collect();
    assert_eq!(dfu.len(), 1, "exactly one JUMP_DFU, written raw: {ops:?}");
    match &ops[dfu[0]] {
        Op::Raw { data, .. } => assert_eq!(&data[2..], &[0x00, 0x00], "JUMP_DFU is `09 id 00 00`"),
        other => panic!("JUMP_DFU must be a write-only raw write, got {other:?}"),
    }
    assert!(
        !ops.iter()
            .any(|o| matches!(o, Op::Transact { req, .. } if req[0] & 0x7F == 0x09)),
        "JUMP_DFU awaits no reply"
    );
    // Nothing of the download goes out before the jump: only the model
    // check and the session the vendor tool opens.
    let before: Vec<u8> = framed_types(&ops[..dfu[0]]);
    assert!(
        before
            .iter()
            .all(|t| matches!(t, 0x02 | 0x40 | 0x44 | 0x4C)),
        "only read-only requests and the auth before JUMP_DFU, got {before:02x?}"
    );
    assert_eq!(payload_writes(&ops), payload_writes(&bench.boot_ops()));
}

/// The 0x4C requests the run sent, decrypted (key 3).
fn auth_plaintexts(ops: &[Op]) -> Vec<Vec<u8>> {
    ops.iter()
        .filter_map(Op::written)
        .filter(|w| w.len() == 36 && w[0] & 0x7F == 0x4C)
        .map(|w| crypto::decrypt(&KEYS[3], &w[4..]))
        .collect()
}

/// Index of the one JUMP_DFU raw write.
fn dfu_index(ops: &[Op]) -> Option<usize> {
    ops.iter().position(
        |o| matches!(o, Op::Raw { data, .. } if data.len() == 4 && data[0] & 0x7F == 0x09),
    )
}

#[test]
fn from_application_mode_the_vendor_session_comes_right_before_jump_dfu() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
    let ops = bench.ops();
    let dfu = dfu_index(&ops).expect("JUMP_DFU was sent");
    let before = &ops[..dfu];
    // The session: CONNECT, the UID read, then one 0x4C; JUMP_DFU follows.
    let types = framed_types(before);
    assert!(
        types.ends_with(&[0x02, 0x44, 0x4C]),
        "CONNECT, the UID read and 0x4C come right before JUMP_DFU, got {types:02x?}"
    );
    assert_eq!(
        memread_addrs(before).last(),
        Some(&(UID_ADDR, 12)),
        "the read before 0x4C is the 12-byte UID"
    );
    // Exactly one level-1 auth: the UID as credential, 0xFF padding.
    let auths = auth_plaintexts(before);
    assert_eq!(auths.len(), 1, "one 0x4C before JUMP_DFU");
    assert_eq!(&auths[0][8..20], &SYNTH_UID, "the UID is the credential");
    assert!(
        auths[0][28..].iter().all(|&b| b == 0xFF),
        "level-1 padding: {:02x?}",
        auths[0]
    );
    let auth_frame = before
        .iter()
        .filter_map(Op::written)
        .rfind(|w| w.len() == 36)
        .expect("the 0x4C frame");
    assert_eq!(&auth_frame[2..4], &[0x00, 0x02], "a 0x4C request header");
    // No 0x4C at all after the jump.
    assert!(auth_plaintexts(&ops[dfu..]).is_empty());
}

/// A failed authentication before JUMP_DFU: no jump, nothing written, the
/// meter stays in application mode and the error names the auth.
fn assert_auth_failure_stops_before_jump_dfu(answer: AuthAnswer) {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().auth(answer).script(boot_ok(P));
    let (r, events) = bench.run(&c);
    let e = r.expect_err("a failed authentication stops the flash");
    assert!(
        e.to_string().to_lowercase().contains("authenticat"),
        "{answer:?}: names the authentication: {e}"
    );
    let ops = bench.ops();
    assert_eq!(dfu_index(&ops), None, "{answer:?}: no JUMP_DFU: {ops:?}");
    assert!(
        !sent_write_capable(&ops),
        "{answer:?}: nothing write-capable: {ops:?}"
    );
    assert_eq!(
        auth_plaintexts(&ops).len(),
        1,
        "{answer:?}: the auth is not retried"
    );
    assert_eq!(bench.mode(), Some(DeviceMode::Application), "{answer:?}");
    assert!(
        !events
            .iter()
            .any(|p| matches!(p, FlashProgress::Replug { .. })),
        "{answer:?}: no replug prompt without a jump: {events:?}"
    );
}

#[test]
fn a_rejected_authentication_stops_the_flash_before_jump_dfu() {
    assert_auth_failure_stops_before_jump_dfu(AuthAnswer::Reject);
}

#[test]
fn an_authentication_granting_level_0_stops_the_flash_before_jump_dfu() {
    assert_auth_failure_stops_before_jump_dfu(AuthAnswer::Grant(0));
}

#[test]
fn the_switch_wait_leaves_two_minutes_for_the_replug() {
    assert_eq!(SWITCH_TIMEOUT, Duration::from_secs(120));
}

#[test]
fn jump_dfu_wait_tolerates_the_meter_vanishing_while_it_re_enumerates() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app()
        .after_dfu(vec![None, None, None, Some(DeviceMode::Bootloader)])
        .script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
    assert!(
        bench.opens() >= 5,
        "the opener was polled until bootloader mode"
    );
}

#[test]
fn still_in_application_mode_after_the_wait_stops_with_the_back_button_fallback() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().after_dfu(vec![Some(DeviceMode::Application)]);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a meter that stays in application mode stops the flash");
    assert!(matches!(e, FlashError::NotInBootloader { .. }), "got {e:?}");
    assert_recovery_text(&e);
    // It waited the whole switch time on the clock, polling the opener.
    assert!(
        bench.clock.total_slept() >= SWITCH_TIMEOUT,
        "slept {:?}",
        bench.clock.total_slept()
    );
    assert!(
        bench.opens() >= 2,
        "the opener was polled again after the jump"
    );
    // Nothing after JUMP_DFU: no descriptor, no payload, no second jump.
    let ops = bench.ops();
    let last = ops.iter().rposition(|o| o.written().is_some()).unwrap();
    assert!(
        matches!(&ops[last], Op::Raw { data, .. } if data.len() == 4 && data[0] & 0x7F == 0x09),
        "JUMP_DFU is the last write, got {:?}",
        ops[last]
    );
    assert!(!framed_types(&ops).contains(&0x42));
    assert!(payload_writes(&ops).is_empty());
}

#[test]
fn no_meter_at_all_after_jump_dfu_stops_with_the_back_button_fallback() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().after_dfu(vec![None]);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("no meter after the jump stops the flash");
    assert_recovery_text(&e);
    assert!(payload_writes(&bench.ops()).is_empty());
}

/// An opener error that is not "no device yet": the device list is
/// ambiguous after the re-enumeration.
fn ambiguous() -> TransportError {
    TransportError::Io("two meters match; pass --device SERIAL".into())
}

#[test]
fn an_opener_error_through_the_switch_wait_is_named_not_called_no_meter() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().open_err_after(0x09, ambiguous(), usize::MAX);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("no bootloader reached");
    assert!(matches!(e, FlashError::NotInBootloader { .. }), "got {e:?}");
    let msg = e.to_string();
    assert!(
        msg.contains("two meters match"),
        "names the opener's reason: {msg}"
    );
    assert!(!msg.contains("no meter"), "a meter was found: {msg}");
    assert_recovery_text(&e);
    assert!(payload_writes(&bench.ops()).is_empty());
}

#[test]
fn opener_errors_that_clear_inside_the_switch_wait_still_flash() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app()
        .open_err_after(0x09, ambiguous(), 3)
        .script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
}

#[test]
fn an_opener_error_through_the_return_wait_is_named_not_a_plain_power_cycle() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .script(boot_ok(P))
        .open_err_after(0x08, ambiguous(), usize::MAX);
    let (r, _) = bench.run(&c);
    assert!(
        !matches!(r, Ok(FlashOutcome::PowerCycleNeeded)),
        "an opener error is not \"the meter stayed in its bootloader\": got {r:?}"
    );
    let e = r.expect_err("the return failure names the opener error");
    assert!(
        e.to_string().contains("two meters match"),
        "names the opener's reason: {e}"
    );
    let ops = bench.ops();
    let aprom = ops
        .iter()
        .filter(|o| matches!(o, Op::Raw { data, .. } if data.len() == 4 && data[0] & 0x7F == 0x08))
        .count();
    assert_eq!(aprom, 1, "JUMP_APROM is not retried");
    assert!(
        memread_addrs(&ops).is_empty(),
        "no info read without the app"
    );
}

#[test]
fn an_opener_error_then_application_mode_through_the_switch_wait_names_application_mode() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app()
        .after_dfu(vec![Some(DeviceMode::Application)])
        .open_err_after(0x09, ambiguous(), 1);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a meter that stays in application mode stops the flash");
    assert!(matches!(e, FlashError::NotInBootloader { .. }), "got {e:?}");
    let msg = e.to_string();
    assert!(
        msg.contains("application mode"),
        "says the meter is still in application mode: {msg}"
    );
    assert!(
        !msg.contains("two meters match"),
        "a later successful open forgets the earlier opener error: {msg}"
    );
    assert_recovery_text(&e);
    assert!(payload_writes(&bench.ops()).is_empty());
}

#[test]
fn an_opener_error_then_the_bootloader_through_the_return_wait_asks_for_a_power_cycle() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .script(boot_ok(P))
        .after_aprom(vec![Some(DeviceMode::Bootloader)])
        .open_err_after(0x08, ambiguous(), 1);
    let (r, _) = bench.run(&c);
    assert!(
        matches!(r, Ok(FlashOutcome::PowerCycleNeeded)),
        "a later successful open forgets the earlier opener error: got {r:?}"
    );
    let ops = bench.ops();
    let aprom = framed_types(&ops).iter().filter(|t| **t == 0x08).count();
    assert_eq!(aprom, 1, "JUMP_APROM is not retried");
    assert!(
        memread_addrs(&ops).is_empty(),
        "no info read without the app"
    );
}

#[test]
fn switch_timeout_is_at_least_the_vendor_two_seconds() {
    assert!(SWITCH_TIMEOUT >= ms(2000), "{SWITCH_TIMEOUT:?}");
}

#[test]
fn a_model_mismatch_in_application_mode_sends_no_write_capable_frame() {
    let bytes = ContainerBuilder::new()
        .field(MODEL, "KM002C")
        .payload_sized(&payload(P))
        .build();
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P));
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a KM002C file is refused on a KM003C");
    assert!(matches!(e, FlashError::Product { .. }), "got {e:?}");
    assert_eq!(e.code(), -100);
    assert!(e.to_string().contains("KM002C"), "{e}");
    let ops = bench.ops();
    let types = framed_types(&ops);
    assert!(
        types.iter().all(|t| matches!(t, 0x02 | 0x40 | 0x44)),
        "only read-only requests, got {types:02x?}"
    );
    assert!(payload_writes(&ops).is_empty());
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
}

#[test]
fn application_mode_reads_only_the_application_info_block_and_the_uid() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P));
    let _ = bench.run(&c);
    let ops = bench.ops();
    let dfu = dfu_index(&ops).expect("JUMP_DFU was sent");
    let addrs = memread_addrs(&ops[..dfu]);
    assert_eq!(
        addrs,
        vec![(0x4420, 64), (UID_ADDR, 12)],
        "the model check reads 0x4420, the session reads the UID; nothing else"
    );
}

#[test]
fn a_model_mismatch_in_bootloader_mode_comes_from_the_usb_product_id_and_sends_nothing() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .model(Model::Km002c, "KM002C")
        .script(boot_ok(P));
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a KM003C file is refused on a KM002C bootloader");
    assert!(matches!(e, FlashError::Product { .. }), "got {e:?}");
    assert!(bench.ops().is_empty(), "nothing sent: {:?}", bench.ops());
}

#[test]
fn an_unknown_model_in_bootloader_mode_sends_nothing() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .model(Model::Unknown(0x1234), "")
        .script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(r.is_err(), "an unknown product ID is refused, got {r:?}");
    assert!(bench.ops().is_empty(), "nothing sent: {:?}", bench.ops());
}

// ---- AC-V9SK54.2: the bootloader sequence and its typed errors ----

#[test]
fn bootloader_sequence_is_connect_head_descriptor_erase_wait_payload_and_finished() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
    let ops = bench.boot_ops();
    let mut i = 0;
    let mut next = || {
        let op = ops
            .get(i)
            .cloned()
            .unwrap_or_else(|| panic!("op {i} missing: {ops:?}"));
        i += 1;
        op
    };
    // CONNECT `02 id 00 00`, 1000 ms.
    match next() {
        Op::Transact { req, timeout } => {
            assert_eq!(req.len(), 4);
            assert_eq!(req[0], 0x02);
            assert_eq!(&req[2..], &[0, 0]);
            assert_eq!(timeout, ms(1000), "CONNECT waits 1000 ms");
        }
        other => panic!("expected CONNECT, got {other:?}"),
    }
    // HEAD `40 id 00 01` + 16 zero bytes.
    match next() {
        Op::Transact { req, .. } => {
            assert_eq!(req.len(), 20);
            assert_eq!(req[0], 0x40);
            assert_eq!(&req[2..4], &[0x00, 0x01]);
            assert!(req[4..].iter().all(|&b| b == 0));
        }
        other => panic!("expected HEAD, got {other:?}"),
    }
    // The 0x42 descriptor as a raw write with the normal 1000 ms write
    // wait, then a separate 20 ms read (expected to time out).
    match next() {
        Op::Raw { data, timeout } => {
            assert_eq!(data.len(), 4 + 64);
            assert_eq!(data[0], 0x42);
            assert_eq!(&data[2..4], &[0x00, 0x04]);
            assert_eq!(timeout, ms(1000), "the descriptor write waits 1000 ms");
        }
        other => panic!("expected the raw descriptor write, got {other:?}"),
    }
    assert_eq!(
        next(),
        Op::Read { timeout: ms(20) },
        "the descriptor's reply read waits 20 ms"
    );
    // Erase wait: N−1 = 2 reads of 10 ms, then one of 1000 ms.
    for _ in 0..2 {
        assert_eq!(next(), Op::Read { timeout: ms(10) });
    }
    assert_eq!(next(), Op::Read { timeout: ms(1000) });
    // Data phase: raw writes, each followed by a read for SYNC.
    let pl = c.payload();
    for (k, chunk) in pl.chunks(B).enumerate() {
        match next() {
            Op::Raw { data, .. } => assert_eq!(data, chunk, "write {k} carries the stored bytes"),
            other => panic!("expected payload write {k}, got {other:?}"),
        }
        assert!(matches!(next(), Op::Read { .. }), "write {k} awaits SYNC");
    }
    // Verify: FINISHED within 1000 ms.
    assert_eq!(next(), Op::Read { timeout: ms(1000) });
}

#[test]
fn descriptor_carries_start_end_size_crc_model_version_and_date() {
    // The stored payload is 16 bytes longer than the declared size, as in
    // the vendor file; the descriptor carries the declared size.
    let bytes = container_bytes(P + 16, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P + 16));
    let _ = bench.run(&c);
    let desc = bench
        .boot_writes()
        .into_iter()
        .find(|w| w[0] == 0x42)
        .expect("a descriptor was sent");
    let d = &desc[4..];
    let word = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let text = |o: usize| {
        let f = &d[o..o + 12];
        let end = f.iter().position(|&b| b == 0).unwrap_or(12);
        assert!(f[end..].iter().all(|&b| b == 0), "NUL padded at {o:#x}");
        String::from_utf8(f[..end].to_vec()).unwrap()
    };
    assert_eq!(word(0x00), 0x4000, "start");
    assert_eq!(word(0x04), 0x4000 + P as u32, "end = start + size");
    assert_eq!(word(0x08), P as u32, "size from the container");
    assert_eq!(word(0x0C), 0xA175_E4A0, "CRC from the container");
    assert_eq!(text(0x10), "KM003C");
    assert_eq!(text(0x1C), "2.0.7");
    assert_eq!(text(0x28), "2026.9.29");
    assert!(d[0x38..].iter().all(|&b| b == 0), "the rest is zero");
}

#[test]
fn payload_is_streamed_byte_for_byte_as_stored_in_4080_byte_writes() {
    let bytes = container_bytes(P + 16, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P + 16));
    let (r, _) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    let writes = payload_writes(&bench.boot_ops());
    let lens: Vec<usize> = writes.iter().map(Vec::len).collect();
    assert_eq!(lens, vec![B, B, B, 116]);
    assert_eq!(writes.concat(), c.payload(), "never decrypted or changed");
}

#[test]
fn a_payload_of_whole_blocks_ends_without_an_empty_write() {
    // The zero-length packet after a 64-multiple is the transport's job;
    // the engine never writes an empty chunk.
    let bytes = container_bytes(2 * B, 2 * B);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(2 * B));
    let (r, _) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    let lens: Vec<usize> = payload_writes(&bench.boot_ops())
        .iter()
        .map(Vec::len)
        .collect();
    assert_eq!(lens, vec![B, B]);
}

#[test]
fn erase_wait_counts_whole_blocks_of_the_streamed_payload_inferred() {
    // [inferred] N counts whole 4080-byte blocks of the payload as stored
    // and streamed, not of the declared size: 3*4080+8 stored bytes give
    // N = 3 (two 10 ms reads) although the declared size gives only 2.
    let stored = 3 * B + 8;
    let bytes = container_bytes(stored, 3 * B - 8);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(stored));
    let (r, _) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    let short = bench
        .boot_ops()
        .iter()
        .filter(|o| **o == Op::Read { timeout: ms(10) })
        .count();
    assert_eq!(short, 2);
}

#[test]
fn a_payload_under_one_block_waits_only_for_the_final_erase_accept() {
    let bytes = container_bytes(100, 100);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(100));
    let (r, _) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    let ops = bench.boot_ops();
    assert!(!ops.contains(&Op::Read { timeout: ms(10) }));
    // CONNECT, HEAD, the raw descriptor, its 20 ms read, then the final wait.
    assert_eq!(ops[3], Op::Read { timeout: ms(20) });
    assert_eq!(ops[4], Op::Read { timeout: ms(1000) });
}

/// Run a bootloader-mode flash of the `P`-byte payload over `script`;
/// return the error, the bench and the progress.
fn boot_failure(script: Vec<Boot>) -> (FlashError, Bench) {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(script);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("the injected failure fails the flash");
    (e, bench)
}

#[test]
fn connect_without_accept_is_a_read_error() {
    // An unanswered CONNECT is retried once (the real bootloader ignores
    // the first one `[device]`); a REJECT is not.
    for (reply, connects) in [(Boot::Timeout, 2), (Boot::Frame(reject()), 1)] {
        let (e, bench) = boot_failure(fail_at(P, AT_CONNECT, reply.clone()));
        assert!(matches!(e, FlashError::Read { .. }), "{reply:?}: got {e:?}");
        assert_eq!(e.code(), -108);
        assert_eq!(bench.boot_writes().len(), connects, "{reply:?}");
    }
}

#[test]
fn a_head_reply_of_another_type_is_a_product_error() {
    let (e, _) = boot_failure(fail_at(P, AT_HEAD, Boot::Frame(reject())));
    assert!(matches!(e, FlashError::Product { .. }), "got {e:?}");
    assert_eq!(e.code(), -100);
}

#[test]
fn a_non_zero_head_field_is_a_work_mode_error() {
    let (e, bench) = boot_failure(fail_at(P, AT_HEAD, Boot::Frame(head(3))));
    assert!(matches!(e, FlashError::WorkMode { .. }), "got {e:?}");
    assert_eq!(e.code(), -111);
    assert_eq!(bench.boot_writes().len(), 2, "no descriptor after HEAD");
}

// ---- AC-V9SK54.2 / .6: bootloader HEAD failures abort before any write ----
//
// On the real meter HEAD in bootloader mode is unproven, so every way HEAD
// can fail must stop before the 0x42 descriptor or a payload write, carry
// the recovery text and (for every reply) the raw reply bytes in hex.

/// A HEAD reply whose type is neither 0x40 nor REJECT.
fn other_type() -> Vec<u8> {
    vec![0x41, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04]
}

/// A 0x40 reply too short to hold the u32 field.
fn short_head() -> Vec<u8> {
    vec![0x40, 0x00, 0x00, 0x01]
}

/// `bytes` as lowercase hex pairs joined by single spaces.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The reply `template` as the meter sent it (byte 1 = the HEAD request's
/// id, as the fake does), in lowercase spaced hex.
fn head_reply_hex(bench: &Bench, template: &[u8]) -> String {
    let ops = bench.boot_ops();
    let id = match &ops[1] {
        Op::Transact { req, .. } if req[0] & 0x7F == 0x40 => req[1],
        other => panic!("expected HEAD second in bootloader mode, got {other:?}"),
    };
    let mut r = template.to_vec();
    r[1] = id;
    hex(&r)
}

/// Every HEAD failure in bootloader mode: name, reply, expected code.
fn head_failures() -> Vec<(&'static str, Boot, i32)> {
    vec![
        ("timeout", Boot::Timeout, -108),
        ("reject", Boot::Frame(reject()), -100),
        ("other type", Boot::Frame(other_type()), -100),
        ("short 0x40", Boot::Frame(short_head()), -100),
        ("non-zero field", Boot::Frame(head(0x1234)), -111),
    ]
}

#[test]
fn every_bootloader_head_failure_stops_before_the_descriptor_with_the_recovery_text() {
    for (name, reply, code) in head_failures() {
        let (e, bench) = boot_failure(fail_at(P, AT_HEAD, reply));
        assert_eq!(e.code(), code, "{name}: got {e:?}");
        let writes = bench.boot_writes();
        assert_eq!(
            writes.len(),
            2,
            "{name}: only CONNECT and HEAD: {writes:02x?}"
        );
        assert_eq!(writes[0][0] & 0x7F, 0x02, "{name}: CONNECT first");
        assert_eq!(writes[1][0] & 0x7F, 0x40, "{name}: HEAD second");
        assert!(
            !sent_write_capable(&bench.ops()),
            "{name}: no jump, descriptor or payload write after HEAD"
        );
        assert!(
            !bench.boot_ops()[2..]
                .iter()
                .any(|o| matches!(o, Op::Raw { .. })),
            "{name}: no raw write after HEAD"
        );
        assert_eq!(bench.mode(), Some(DeviceMode::Bootloader), "{name}");
        assert_recovery_text(&e);
    }
}

#[test]
fn a_head_timeout_in_the_bootloader_is_a_read_error() {
    let (e, _) = boot_failure(fail_at(P, AT_HEAD, Boot::Timeout));
    assert!(matches!(e, FlashError::Read { .. }), "got {e:?}");
    assert_eq!(e.code(), -108);
    assert!(e.to_string().contains("HEAD"), "names HEAD: {e}");
}

#[test]
fn a_too_short_head_reply_is_a_product_error() {
    let (e, _) = boot_failure(fail_at(P, AT_HEAD, Boot::Frame(short_head())));
    assert!(matches!(e, FlashError::Product { .. }), "got {e:?}");
    assert_eq!(e.code(), -100);
}

#[test]
fn a_head_reply_of_a_non_reject_type_is_a_product_error() {
    let (e, _) = boot_failure(fail_at(P, AT_HEAD, Boot::Frame(other_type())));
    assert!(matches!(e, FlashError::Product { .. }), "got {e:?}");
    assert_eq!(e.code(), -100);
}

#[test]
fn every_head_failure_with_a_reply_carries_the_raw_reply_in_hex() {
    for (name, reply, _) in head_failures() {
        let Boot::Frame(template) = &reply else {
            continue;
        };
        let (e, bench) = boot_failure(fail_at(P, AT_HEAD, reply.clone()));
        let want = format!("head reply {}", head_reply_hex(&bench, template));
        let msg = e.to_string();
        assert!(
            msg.to_ascii_lowercase().contains(&want),
            "{name}: message lacks `{want}`: {msg}"
        );
    }
}

#[test]
fn a_rejected_head_message_shows_the_reject_bytes() {
    let (e, bench) = boot_failure(fail_at(P, AT_HEAD, Boot::Frame(reject())));
    // e.g. `HEAD reply 06 02 00 00`.
    let want = format!("head reply {}", head_reply_hex(&bench, &reject()));
    assert!(want.starts_with("head reply 06 "), "{want}");
    assert!(
        e.to_string().to_ascii_lowercase().contains(&want),
        "lacks `{want}`: {e}"
    );
}

#[test]
fn a_work_mode_error_names_the_field_in_hex_and_the_raw_reply() {
    let (e, bench) = boot_failure(fail_at(P, AT_HEAD, Boot::Frame(head(0x1234))));
    assert!(matches!(e, FlashError::WorkMode { .. }), "got {e:?}");
    let msg = e.to_string().to_ascii_lowercase();
    assert!(msg.contains("0x1234"), "field in hex: {msg}");
    let want = format!("head reply {}", head_reply_hex(&bench, &head(0x1234)));
    assert!(want.contains("34 12 00 00"), "{want}");
    assert!(msg.contains(&want), "lacks `{want}`: {msg}");
}

// ---- AC-V9SK52.3 (changed 2026-10-26): id-0 HEAD replies stay fail-closed ----
//
// In application mode the meter answers HEAD with id 0 (field 0x1000 on
// 2.0.6). The flash path in the bootloader never takes such a reply as a
// go-ahead: an id-0 reply, or a field of 0x1000, stops before the 0x42
// descriptor with a typed error that names the raw reply.

/// A HEAD reply with id 0 and `field`, kept as given (not re-stamped).
fn id0_head(field: u32) -> Vec<u8> {
    head(field)
}

fn assert_stops_before_the_descriptor(name: &str, e: &FlashError, bench: &Bench, raw: &[u8]) {
    let writes = bench.boot_writes();
    assert_eq!(
        writes.len(),
        2,
        "{name}: only CONNECT and HEAD: {writes:02x?}"
    );
    assert_eq!(writes[1][0] & 0x7F, 0x40, "{name}: HEAD second");
    assert_ne!(writes[1][1], 0, "{name}: the HEAD request id is not 0");
    assert!(
        !sent_write_capable(&bench.ops()),
        "{name}: no 0x42 descriptor or payload write"
    );
    assert!(
        !bench.boot_ops()[2..]
            .iter()
            .any(|o| matches!(o, Op::Raw { .. })),
        "{name}: no raw write after HEAD"
    );
    let want = format!("head reply {}", hex(raw));
    let msg = e.to_string().to_ascii_lowercase();
    assert!(msg.contains(&want), "{name}: lacks `{want}`: {msg}");
    assert_recovery_text(e);
}

#[test]
fn an_id_0_head_reply_with_field_0_in_the_bootloader_aborts_before_the_descriptor() {
    let raw = id0_head(0);
    let (e, bench) = boot_failure(fail_at(P, AT_HEAD, Boot::Exact(raw.clone())));
    assert!(
        matches!(
            e,
            FlashError::Product { .. } | FlashError::Read { .. } | FlashError::WorkMode { .. }
        ),
        "a typed HEAD error: {e:?}"
    );
    assert_stops_before_the_descriptor("id 0, field 0", &e, &bench, &raw);
}

#[test]
fn an_id_0_head_reply_with_field_0x1000_in_the_bootloader_aborts_before_the_descriptor() {
    let raw = id0_head(0x1000);
    let (e, bench) = boot_failure(fail_at(P, AT_HEAD, Boot::Exact(raw.clone())));
    assert!(
        matches!(
            e,
            FlashError::Product { .. } | FlashError::Read { .. } | FlashError::WorkMode { .. }
        ),
        "a typed HEAD error: {e:?}"
    );
    assert_stops_before_the_descriptor("id 0, field 0x1000", &e, &bench, &raw);
}

#[test]
fn a_head_field_of_0x1000_with_the_request_id_in_the_bootloader_is_a_work_mode_error() {
    let (e, bench) = boot_failure(fail_at(P, AT_HEAD, Boot::Frame(head(0x1000))));
    assert!(
        matches!(e, FlashError::WorkMode { field: 0x1000, .. }),
        "got {e:?}"
    );
    assert!(e.to_string().contains("0x1000"), "{e}");
    let mut bytes = head(0x1000);
    bytes[1] = bench.boot_writes()[1][1];
    assert_stops_before_the_descriptor("request id, field 0x1000", &e, &bench, &bytes);
}

#[test]
fn a_head_timeout_message_has_no_reply_bytes() {
    let (e, _) = boot_failure(fail_at(P, AT_HEAD, Boot::Timeout));
    assert!(
        !e.to_string().to_ascii_lowercase().contains("head reply "),
        "a timeout has no reply to show: {e}"
    );
}

#[test]
fn a_descriptor_that_is_answered_is_a_write_error() {
    // An ACCEPT with the descriptor id is the erase done (see the erase-wait
    // tests); any other answer is still a write error.
    for reply in [
        Boot::Exact(vec![0x05, 0x00, 0x00, 0x00]),
        Boot::Frame(reject()),
        Boot::Frame(sync()),
        Boot::Fail(TransportError::Disconnected),
    ] {
        let (e, bench) = boot_failure(fail_at(P, AT_DESCRIPTOR, reply.clone()));
        assert!(
            matches!(e, FlashError::Write { .. }),
            "{reply:?}: got {e:?}"
        );
        assert_eq!(e.code(), -107);
        assert_eq!(bench.boot_writes().len(), 3);
    }
}

/// `boot_ok(stored)` with the erase wait ended at short read `at` (0-based,
/// counting the descriptor read as 0) by `ack`: the reads after it are not
/// scripted, the payload SYNCs and FINISHED follow.
fn early_ack_script(stored: usize, at: usize, ack: Boot) -> Vec<Boot> {
    let n = stored / B;
    let chunks = stored.div_ceil(B);
    let mut v = vec![Boot::Frame(accept()), Boot::Frame(head(0))];
    v.extend((0..at).map(|_| Boot::Timeout));
    v.push(ack);
    assert!(at < n, "the ack must come before the final erase read");
    v.extend((0..chunks).map(|_| Boot::Frame(sync())));
    v.push(Boot::Frame(finished()));
    v
}

#[test]
fn an_accept_with_the_descriptor_id_during_the_erase_wait_ends_it() {
    // The real bootloader ACKs the descriptor as soon as the erase is done,
    // before the vendor's N−1 short reads are over `[device]`.
    for at in [0, 1] {
        let bytes = container_bytes(P, P);
        let c = parse(&bytes);
        let bench = Bench::boot().script(early_ack_script(P, at, Boot::Frame(accept())));
        let (r, _) = bench.run(&c);
        assert_eq!(
            r,
            Ok(FlashOutcome::Verified {
                version: "2.0.7".into()
            }),
            "ACCEPT at read {at}"
        );
        let ops = bench.boot_ops();
        let desc = ops
            .iter()
            .position(|o| o.written().is_some_and(is_raw_descriptor))
            .expect("the descriptor");
        let reads = ops[desc + 1..]
            .iter()
            .take_while(|o| matches!(o, Op::Read { .. }))
            .count();
        assert_eq!(reads, at + 1, "no read after the ACCEPT before the payload");
        assert_eq!(payload_writes(&ops).len(), P.div_ceil(B));
    }
}

#[test]
fn an_accept_with_another_id_during_the_erase_wait_is_an_erase_error() {
    for id in [0x00, 0x02, 0x04] {
        let (e, bench) = boot_failure(early_ack_script(P, 1, Boot::Exact(vec![0x05, id, 0, 0])));
        assert!(matches!(e, FlashError::Erase { .. }), "id {id}: got {e:?}");
        assert_eq!(e.code(), -105);
        assert!(payload_writes(&bench.boot_ops()).is_empty());
    }
}

#[test]
fn other_replies_during_the_erase_wait_are_an_erase_error() {
    for reply in [
        Boot::Frame(reject()),
        Boot::Frame(sync()),
        Boot::Frame(finished()),
    ] {
        let (e, bench) = boot_failure(fail_at(P, AT_ERASE_SHORT, reply.clone()));
        assert!(
            matches!(e, FlashError::Erase { .. }),
            "{reply:?}: got {e:?}"
        );
        assert_eq!(e.code(), -105);
        assert!(payload_writes(&bench.boot_ops()).is_empty());
    }
}

#[test]
fn no_accept_after_the_erase_wait_is_an_erase_error() {
    for reply in [Boot::Timeout, Boot::Frame(reject()), Boot::Frame(sync())] {
        let (e, bench) = boot_failure(fail_at(P, AT_ERASE_ACCEPT, reply.clone()));
        assert!(
            matches!(e, FlashError::Erase { .. }),
            "{reply:?}: got {e:?}"
        );
        assert_eq!(e.code(), -105);
        assert!(payload_writes(&bench.boot_ops()).is_empty());
    }
}

#[test]
fn a_missing_sync_is_a_download_error() {
    for reply in [Boot::Timeout, Boot::Frame(accept())] {
        // SYNC for the second write is missing.
        let (e, bench) = boot_failure(fail_at(P, AT_SYNC_1 + 1, reply.clone()));
        assert!(
            matches!(e, FlashError::Download { .. }),
            "{reply:?}: got {e:?}"
        );
        assert_eq!(e.code(), -106);
        assert_eq!(payload_writes(&bench.boot_ops()).len(), 2, "no third write");
    }
}

#[test]
fn a_failed_payload_write_is_a_download_error() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P)).raw_fail_at(3);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a USB error mid-transfer fails the flash");
    assert!(matches!(e, FlashError::Download { .. }), "got {e:?}");
    assert_eq!(payload_writes(&bench.boot_ops()).len(), 3);
}

#[test]
fn no_finished_or_another_reply_at_verify_is_a_verify_error() {
    for reply in [Boot::Timeout, Boot::Frame(reject()), Boot::Frame(sync())] {
        let (e, _) = boot_failure(fail_at(P, AT_FINISHED, reply.clone()));
        assert!(
            matches!(e, FlashError::Verify { .. }),
            "{reply:?}: got {e:?}"
        );
        assert_eq!(e.code(), -103);
    }
}

// ---- AC-V9SK54.3: the 0x4000 floor and the strict flash guard ----

#[test]
fn the_application_area_starts_at_0x4000_and_blocks_are_4080_bytes() {
    assert_eq!(APP_START, 0x4000);
    assert_eq!(BLOCK_LEN, 4080);
    // MAX_APP_END > APP_START is checked at compile time by the property
    // test's `1..=(MAX_APP_END - APP_START)` range below.
}

#[test]
fn the_flash_guard_refuses_a_descriptor_below_0x4000_without_writing() {
    let mut g = FlashGuard::new(Recorder::default());
    let frame = descriptor_frame(1, 0, P as u32, P as u32);
    assert_eq!(
        g.write_raw(&frame, ms(1000)),
        Err(TransportError::Forbidden { ty: 0x42 })
    );
    assert!(g.inner().ops.is_empty());
}

#[test]
fn the_flash_guard_passes_a_valid_descriptor_as_a_raw_write() {
    // The descriptor is a raw write (AC .2 review fix): the strict guard
    // accepts it through `write_raw` and forwards it untouched.
    let mut g = FlashGuard::new(Recorder::default());
    let frame = descriptor_frame(1, 0x4000, 0x4000 + P as u32, P as u32);
    assert_eq!(g.write_raw(&frame, ms(1000)), Ok(()));
    assert_eq!(
        g.inner().ops,
        vec![Op::Raw {
            data: frame,
            timeout: ms(1000)
        }]
    );
}

#[test]
fn a_raw_descriptor_arms_the_guard_for_payload_writes() {
    let mut g = FlashGuard::new(Recorder::default());
    let frame = descriptor_frame(1, 0x4000, 0x4000 + P as u32, P as u32);
    assert_eq!(g.write_raw(&frame, ms(1000)), Ok(()));
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    assert_eq!(forwarded_payload(&g.inner().ops), vec![B]);
}

#[test]
fn the_flash_guard_refuses_everything_outside_the_flash_set() {
    let refused: Vec<Vec<u8>> = vec![
        vec![0x04, 1, 0, 0],         // RESET
        vec![0x48, 1, 0, 0],         // SET_DATA
        vec![0x4A, 1, 0, 0],         // memory / flash write
        vec![0x0C, 1, 0x02, 0x00],   // GET_DATA
        vec![0x0E, 1, 0x00, 0x00],   // start queue
        vec![0x4C, 1, 0x00, 0x02],   // a bare auth header
        level2_auth(1),              // a level-2 (OTP credential) auth
        memread(1, 0x0420, 64),      // not the application info block
        memread(1, 0x0300_0C00, 64), // OTP
        memread(1, 0x4001_0450, 16), // not exactly the 12-byte UID
        memread(1, 0x4001_0440, 12), // not the UID address
    ];
    for frame in refused {
        let mut g = FlashGuard::new(Recorder::default());
        let r = g.transact(&frame, ms(1000));
        assert!(
            matches!(r, Err(TransportError::Forbidden { .. })),
            "{frame:02x?}: got {r:?}"
        );
        assert!(g.inner().ops.is_empty(), "{frame:02x?} reached the pipe");
    }
}

#[test]
fn the_flash_guard_allows_connect_head_the_info_and_uid_reads_and_a_level_1_auth() {
    let allowed: Vec<Vec<u8>> = vec![
        vec![0x02, 1, 0, 0],
        powerz_protocol::decode::head::request(2).to_vec(),
        memread(3, 0x4420, 64),
        memread(4, UID_ADDR, 12),
        level1_auth(5),
    ];
    for frame in allowed {
        let mut g = FlashGuard::new(Recorder::default());
        assert_eq!(g.transact(&frame, ms(1000)), Err(TransportError::Timeout));
        assert_eq!(g.inner().ops.len(), 1, "{frame:02x?} passes");
    }
}

#[test]
fn the_flash_guard_allows_jumps_as_raw_writes() {
    for jump in [[0x09, 1, 0, 0], [0x08, 2, 0, 0]] {
        let mut g = FlashGuard::new(Recorder::default());
        assert_eq!(g.write_raw(&jump, ms(1000)), Ok(()));
        assert_eq!(g.inner().ops.len(), 1);
    }
}

#[test]
fn the_flash_guard_refuses_payload_writes_before_a_valid_descriptor() {
    let mut g = FlashGuard::new(Recorder::default());
    let r = g.write_raw(&payload(B), ms(1000));
    assert!(
        matches!(r, Err(TransportError::Forbidden { .. })),
        "got {r:?}"
    );
    // A refused descriptor does not arm it either.
    let _ = g.write_raw(&descriptor_frame(1, 0x2000, 0x2000 + 64, 64), ms(1000));
    let r = g.write_raw(&payload(B), ms(1000));
    assert!(
        matches!(r, Err(TransportError::Forbidden { .. })),
        "got {r:?}"
    );
    assert!(g.inner().ops.is_empty());
}

#[test]
fn the_flash_guard_allows_payload_writes_after_a_valid_descriptor() {
    let mut g = FlashGuard::new(Recorder::default());
    let _ = g.write_raw(
        &descriptor_frame(1, 0x4000, 0x4000 + P as u32, P as u32),
        ms(1000),
    );
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    assert_eq!(g.inner().ops.len(), 2);
}

// ---- AC-V9SK54.3 (review fix): the armed guard is bounded ----

/// A guard armed by a valid descriptor that declares `size` bytes.
fn armed(size: u32) -> FlashGuard<Recorder> {
    let mut g = FlashGuard::new(Recorder::default());
    let _ = g.write_raw(&descriptor_frame(1, 0x4000, 0x4000 + size, size), ms(1000));
    assert_eq!(g.inner().ops.len(), 1, "the descriptor reaches the pipe");
    g
}

/// The payload bytes the guard let through (raw writes that are not jumps).
fn forwarded_payload(ops: &[Op]) -> Vec<usize> {
    ops.iter()
        .filter_map(|op| match op {
            Op::Raw { data, .. }
                if !((data.len() == 4 && matches!(data[0], 0x08 | 0x09))
                    || is_raw_descriptor(data)) =>
            {
                Some(data.len())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_armed_guard_refuses_a_payload_write_longer_than_one_block() {
    let mut g = armed(P as u32);
    let r = g.write_raw(&payload(B + 1), ms(1000));
    assert!(r.is_err(), "a {}-byte write passed: {r:?}", B + 1);
    let r = g.write_raw(&payload(P), ms(1000));
    assert!(r.is_err(), "the whole payload in one write passed: {r:?}");
    assert_eq!(
        g.inner().ops.len(),
        1,
        "an oversized write reached the pipe"
    );
    // One block is still fine.
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    assert_eq!(forwarded_payload(&g.inner().ops), vec![B]);
}

#[test]
fn the_armed_guard_allows_the_declared_size_plus_one_block_of_slack() {
    // P = 3 blocks + 100: the cap is P + B = 4 blocks + 100.
    let mut g = armed(P as u32);
    for k in 0..4 {
        assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()), "block {k}");
    }
    assert_eq!(g.write_raw(&payload(100), ms(1000)), Ok(()));
    assert_eq!(
        forwarded_payload(&g.inner().ops).iter().sum::<usize>(),
        P + B
    );
    // One more byte crosses the cap and is refused before writing.
    let before = g.inner().ops.len();
    let r = g.write_raw(&payload(1), ms(1000));
    assert!(r.is_err(), "a write past size + BLOCK_LEN passed: {r:?}");
    assert_eq!(g.inner().ops.len(), before);
}

#[test]
fn the_write_that_would_cross_the_cap_is_refused_whole() {
    // Size 100: the cap is 100 + B. After one block, 101 bytes would cross.
    let mut g = armed(100);
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    let r = g.write_raw(&payload(101), ms(1000));
    assert!(r.is_err(), "a write crossing the cap passed: {r:?}");
    assert_eq!(forwarded_payload(&g.inner().ops), vec![B]);
}

#[test]
fn jump_aprom_disarms_the_guard_until_a_new_descriptor() {
    let mut g = armed(P as u32);
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    assert_eq!(g.write_raw(&[0x08, 2, 0, 0], ms(1000)), Ok(()));
    let before = g.inner().ops.len();
    let r = g.write_raw(&payload(B), ms(1000));
    assert!(r.is_err(), "a payload write after JUMP_APROM passed: {r:?}");
    assert_eq!(g.inner().ops.len(), before);
    // A new valid descriptor arms it again.
    let _ = g.write_raw(
        &descriptor_frame(3, 0x4000, 0x4000 + P as u32, P as u32),
        ms(1000),
    );
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
}

#[test]
fn a_new_connect_disarms_the_guard_until_a_new_descriptor() {
    let mut g = armed(P as u32);
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    let _ = g.transact(&[0x02, 2, 0, 0], ms(1000));
    let before = g.inner().ops.len();
    assert_eq!(before, 3, "CONNECT itself reaches the pipe");
    let r = g.write_raw(&payload(B), ms(1000));
    assert!(r.is_err(), "a payload write after CONNECT passed: {r:?}");
    assert_eq!(g.inner().ops.len(), before);
    let _ = g.write_raw(
        &descriptor_frame(3, 0x4000, 0x4000 + P as u32, P as u32),
        ms(1000),
    );
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
}

#[test]
fn a_new_descriptor_starts_a_fresh_budget() {
    // Size 100, cap 100 + B: use it all, re-arm, and the full cap is back.
    let mut g = armed(100);
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    assert_eq!(g.write_raw(&payload(100), ms(1000)), Ok(()));
    let _ = g.transact(&[0x02, 2, 0, 0], ms(1000));
    let _ = g.write_raw(&descriptor_frame(3, 0x4000, 0x4000 + 100, 100), ms(1000));
    assert_eq!(g.write_raw(&payload(B), ms(1000)), Ok(()));
    assert_eq!(g.write_raw(&payload(100), ms(1000)), Ok(()));
    assert!(g.write_raw(&payload(1), ms(1000)).is_err());
    assert_eq!(forwarded_payload(&g.inner().ops), vec![B, 100, B, 100]);
}

proptest! {
    #[test]
    fn the_armed_guard_never_forwards_more_than_size_plus_one_block(
        size in 1u32..=(MAX_APP_END - APP_START),
        lens in prop::collection::vec(1usize..=2 * B, 0..160),
    ) {
        let mut g = armed(size);
        for len in lens {
            let before = g.inner().ops.len();
            let r = g.write_raw(&payload(len), ms(1000));
            if r.is_err() {
                prop_assert_eq!(g.inner().ops.len(), before, "a refused write reached the pipe");
            }
        }
        let sent = forwarded_payload(&g.inner().ops);
        prop_assert!(sent.iter().all(|&n| n <= B), "a write over one block: {:?}", sent);
        let total: usize = sent.iter().sum();
        prop_assert!(
            total <= size as usize + B,
            "forwarded {} bytes for a {}-byte descriptor",
            total,
            size
        );
    }

    #[test]
    fn the_armed_guard_passes_the_declared_size_and_the_padding(
        size in 1u32..=(MAX_APP_END - APP_START - 16),
        pad in prop_oneof![Just(0usize), Just(16usize)],
    ) {
        let mut g = armed(size);
        let data = payload(size as usize + pad);
        for chunk in data.chunks(B) {
            prop_assert_eq!(g.write_raw(chunk, ms(1000)), Ok(()));
        }
        prop_assert_eq!(
            forwarded_payload(&g.inner().ops).iter().sum::<usize>(),
            size as usize + pad
        );
    }
}

proptest! {
    #[test]
    fn any_descriptor_below_0x4000_is_refused_and_nothing_is_written(
        start in 0u32..0x4000,
        size in 0u32..0x0080_0000,
        id in any::<u8>(),
    ) {
        let mut g = FlashGuard::new(Recorder::default());
        let frame = descriptor_frame(id, start, start.wrapping_add(size), size);
        prop_assert_eq!(
            g.write_raw(&frame, ms(1000)),
            Err(TransportError::Forbidden { ty: 0x42 })
        );
        prop_assert!(g.inner().ops.is_empty());
    }

    #[test]
    fn any_descriptor_not_starting_at_0x4000_is_refused_and_nothing_is_written(
        start in any::<u32>().prop_filter("not the app start", |s| *s != 0x4000),
        size in any::<u32>(),
        id in any::<u8>(),
    ) {
        let mut g = FlashGuard::new(Recorder::default());
        let frame = descriptor_frame(id, start, start.wrapping_add(size), size);
        prop_assert_eq!(
            g.write_raw(&frame, ms(1000)),
            Err(TransportError::Forbidden { ty: 0x42 })
        );
        prop_assert!(g.inner().ops.is_empty());
    }

    #[test]
    fn a_descriptor_whose_end_is_not_start_plus_size_is_refused(
        size in 1u32..0x0001_0000,
        end in any::<u32>(),
    ) {
        prop_assume!(end != 0x4000 + size);
        let mut g = FlashGuard::new(Recorder::default());
        let frame = descriptor_frame(1, 0x4000, end, size);
        prop_assert_eq!(
            g.write_raw(&frame, ms(1000)),
            Err(TransportError::Forbidden { ty: 0x42 })
        );
        prop_assert!(g.inner().ops.is_empty());
    }

    #[test]
    fn a_descriptor_ending_past_the_top_is_refused(extra in 1u32..0x0100_0000) {
        let size = (MAX_APP_END - APP_START).saturating_add(extra);
        let mut g = FlashGuard::new(Recorder::default());
        let frame = descriptor_frame(1, 0x4000, 0x4000u32.wrapping_add(size), size);
        prop_assert_eq!(
            g.write_raw(&frame, ms(1000)),
            Err(TransportError::Forbidden { ty: 0x42 })
        );
        prop_assert!(g.inner().ops.is_empty());
    }

    #[test]
    fn a_descriptor_at_0x4000_within_the_area_is_written(
        size in 1u32..=(MAX_APP_END - APP_START),
        id in any::<u8>(),
    ) {
        let mut g = FlashGuard::new(Recorder::default());
        let frame = descriptor_frame(id, 0x4000, 0x4000 + size, size);
        prop_assert_eq!(g.write_raw(&frame, ms(1000)), Ok(()));
        prop_assert_eq!(g.inner().ops.len(), 1);
    }
}

#[test]
fn every_descriptor_the_engine_sends_starts_at_0x4000() {
    for stored in [100, B, 2 * B + 1, P + 16] {
        let bytes = container_bytes(stored, stored.min(P));
        let c = parse(&bytes);
        let bench = Bench::boot().script(boot_ok(stored));
        let _ = bench.run(&c);
        for w in bench.boot_writes().iter().filter(|w| w[0] == 0x42) {
            assert_eq!(&w[4..8], &0x4000u32.to_le_bytes(), "stored {stored}");
        }
    }
}

// ---- The shared guard is unchanged for every other path ----

#[test]
fn the_shared_guard_still_refuses_jumps_and_the_descriptor() {
    assert_eq!(FORBIDDEN_TYPES, [0x04, 0x08, 0x09, 0x0A, 0x42, 0x48, 0x4A]);
    for ty in [0x08, 0x09, 0x42] {
        assert_eq!(
            guard::check(&[ty, 0x01, 0x00, 0x00]),
            Err(TransportError::Forbidden { ty })
        );
        let mut g = GuardedTransport::new(Recorder::default());
        assert_eq!(
            g.transact(&[ty, 0x01, 0x00, 0x00], ms(1000)),
            Err(TransportError::Forbidden { ty })
        );
        assert!(g.inner().ops.is_empty());
    }
}

#[test]
fn bulk_open_still_returns_a_guarded_transport() {
    let open: fn(&DeviceEntry) -> Result<GuardedTransport<BulkTransport>, TransportError> =
        BulkTransport::open;
    let _ = open;
}

// ---- AC-V9SK54.4: progress stages ----

#[test]
fn progress_from_application_mode_is_switch_erase_write_verify_finished() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P));
    let (r, events) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    assert_eq!(
        stages(&events),
        vec![
            "switching",
            "replug",
            "erasing",
            "writing",
            "verifying",
            "finished",
            "returning",
            "replug"
        ]
    );
}

#[test]
fn progress_from_bootloader_mode_has_no_switch_stage() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P));
    let (r, events) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    assert_eq!(
        stages(&events),
        vec![
            "erasing",
            "writing",
            "verifying",
            "finished",
            "returning",
            "replug"
        ]
    );
}

#[test]
fn writing_progress_counts_acknowledged_bytes_up_to_the_payload_length() {
    let bytes = container_bytes(P + 16, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P + 16));
    let (_, events) = bench.run(&c);
    let writes: Vec<(usize, usize)> = events
        .iter()
        .filter_map(|e| match e {
            FlashProgress::Writing { done, total } => Some((*done, *total)),
            _ => None,
        })
        .filter(|(done, _)| *done > 0)
        .collect();
    let total = c.payload().len();
    assert!(writes.iter().all(|(_, t)| *t == total), "{writes:?}");
    let dones: Vec<usize> = writes.iter().map(|(d, _)| *d).collect();
    assert_eq!(dones, vec![B, 2 * B, 3 * B, total]);
}

#[test]
fn progress_stops_at_the_failing_stage() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(fail_at(P, AT_SYNC_1 + 1, Boot::Timeout));
    let (r, events) = bench.run(&c);
    assert!(r.is_err());
    assert_eq!(stages(&events), vec!["erasing", "writing"]);
}

// ---- AC-V9SK54.5: back to application mode, confirm the version ----

#[test]
fn after_finished_it_jumps_to_aprom_and_confirms_the_new_version() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P));
    let (r, _) = bench.run(&c);
    match r {
        Ok(FlashOutcome::Verified { version }) => assert_eq!(version, "2.0.7"),
        other => panic!("expected the new version, got {other:?}"),
    }
    let ops = bench.ops();
    let aprom: Vec<&Op> = ops
        .iter()
        .filter(|o| matches!(o, Op::Raw { data, .. } if data.len() == 4 && data[0] & 0x7F == 0x08))
        .collect();
    assert_eq!(aprom.len(), 1, "one write-only JUMP_APROM");
    assert!(
        !ops.iter()
            .any(|o| matches!(o, Op::Transact { req, .. } if req[0] & 0x7F == 0x08)),
        "JUMP_APROM awaits no reply"
    );
    let addrs = memread_addrs(&ops);
    assert!(addrs.contains(&(0x4420, 64)), "the info block is read");
    assert!(addrs.iter().all(|a| *a == (0x4420, 64)), "{addrs:x?}");
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
}

#[test]
fn a_meter_that_stays_in_the_bootloader_after_aprom_asks_for_a_power_cycle() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .script(boot_ok(P))
        .after_aprom(vec![Some(DeviceMode::Bootloader)]);
    let (r, _) = bench.run(&c);
    assert!(
        matches!(r, Ok(FlashOutcome::PowerCycleNeeded)),
        "not an error: got {r:?}"
    );
    assert!(bench.clock.total_slept() >= RETURN_TIMEOUT);
    let ops = bench.ops();
    let aprom = framed_types(&ops).iter().filter(|t| **t == 0x08).count();
    assert_eq!(aprom, 1, "JUMP_APROM is not retried");
    assert!(
        memread_addrs(&ops).is_empty(),
        "no info read without the app"
    );
}

#[test]
fn a_version_that_does_not_match_the_file_after_the_return_is_a_verify_error() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    // The image did not take: the meter still reports its old version.
    let bench = Bench::boot().script(boot_ok(P)).new_version("2.0.6");
    let (r, _) = bench.run(&c);
    let e = match r {
        Err(e) => e,
        Ok(outcome) => panic!("a version mismatch is not a success, got {outcome:?}"),
    };
    assert!(matches!(e, FlashError::Verify { .. }), "got {e:?}");
    assert_eq!(e.code(), -103);
    let msg = e.to_string();
    assert!(msg.contains("2.0.6"), "names the meter's version: {msg}");
    assert!(msg.contains("2.0.7"), "names the file's version: {msg}");
    assert!(
        msg.to_lowercase().contains("again") || msg.to_lowercase().contains("retry"),
        "says how to retry: {msg}"
    );
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
    let ops = bench.ops();
    assert_eq!(
        framed_types(&ops).iter().filter(|t| **t == 0x08).count(),
        1,
        "JUMP_APROM is not retried"
    );
}

#[test]
fn a_matching_version_after_the_return_is_verified() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P)).new_version("2.0.7");
    let (r, _) = bench.run(&c);
    assert_eq!(
        r,
        Ok(FlashOutcome::Verified {
            version: "2.0.7".into()
        })
    );
}

// ---- AC-V9SK54.6: no automatic retry, recovery text ----

#[test]
fn after_any_bootloader_failure_nothing_more_is_sent_and_the_hint_is_given() {
    let cases: Vec<(&str, Vec<Boot>, usize)> = vec![
        // Two CONNECTs: an unanswered one is retried once.
        ("connect", fail_at(P, AT_CONNECT, Boot::Timeout), 2),
        ("head", fail_at(P, AT_HEAD, Boot::Frame(head(1))), 2),
        (
            "descriptor",
            fail_at(P, AT_DESCRIPTOR, Boot::Frame(reject())),
            3,
        ),
        (
            "early erase",
            fail_at(P, AT_ERASE_SHORT, Boot::Frame(reject())),
            3,
        ),
        ("erase", fail_at(P, AT_ERASE_ACCEPT, Boot::Timeout), 3),
        ("sync", fail_at(P, AT_SYNC_1 + 2, Boot::Timeout), 3 + 3),
        ("verify", fail_at(P, AT_FINISHED, Boot::Timeout), 3 + 4),
    ];
    for (name, script, writes) in cases {
        let (e, bench) = boot_failure(script);
        assert_eq!(
            bench.boot_writes().len(),
            writes,
            "{name}: nothing is sent after the failing step"
        );
        assert!(
            !framed_types(&bench.ops()).contains(&0x08),
            "{name}: no JUMP_APROM after a failure"
        );
        assert_eq!(
            framed_types(&bench.ops())
                .iter()
                .filter(|t| **t == 0x02)
                .count(),
            if name == "connect" { 2 } else { 1 },
            "{name}: only an unanswered CONNECT is retried, once"
        );
        assert_eq!(bench.mode(), Some(DeviceMode::Bootloader), "{name}");
        assert_recovery_text(&e);
    }
}

#[test]
fn a_failure_after_jump_dfu_leaves_the_meter_in_the_bootloader_with_the_hint() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(fail_at(P, AT_ERASE_ACCEPT, Boot::Timeout));
    let (r, _) = bench.run(&c);
    let e = r.expect_err("the erase failure fails the flash");
    assert!(matches!(e, FlashError::Erase { .. }), "got {e:?}");
    assert_recovery_text(&e);
    assert_eq!(bench.mode(), Some(DeviceMode::Bootloader));
    let dfu = framed_types(&bench.ops())
        .iter()
        .filter(|t| **t == 0x09)
        .count();
    assert_eq!(dfu, 1, "JUMP_DFU is not repeated");
}

#[test]
fn a_failed_jump_aprom_write_carries_the_recovery_text_and_nothing_more_is_sent() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    // An I/O error (not the reset-as-it-jumps `Disconnected`) is fatal.
    let bench = Bench::boot()
        .script(boot_ok(P))
        .aprom_fail(TransportError::Io("pipe error".into()));
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a failed JUMP_APROM write fails the flash");
    assert_recovery_text(&e);
    assert!(
        e.to_string().to_lowercase().contains("bootloader mode"),
        "says the meter stays in bootloader mode: {e}"
    );
    assert!(
        e.to_string().contains("run the update again"),
        "says how to retry: {e}"
    );
    let ops = bench.ops();
    let last = ops.last().expect("something was sent");
    assert!(
        matches!(last, Op::Raw { data, .. } if data.len() == 4 && data[0] & 0x7F == 0x08),
        "the failed JUMP_APROM is the last thing sent, got {last:?}"
    );
    assert_eq!(
        framed_types(&ops).iter().filter(|t| **t == 0x08).count(),
        1,
        "JUMP_APROM is not retried"
    );
    assert!(memread_addrs(&ops).is_empty(), "no info read");
    assert_eq!(bench.mode(), Some(DeviceMode::Bootloader));
}

// ---- AC-V9SK54.6 (review fix): interruption between payload writes ----

/// Whether any write-capable frame went out: a jump, the descriptor or a
/// payload write.
fn sent_write_capable(ops: &[Op]) -> bool {
    let types = framed_types(ops);
    types.iter().any(|t| matches!(t, 0x08 | 0x09 | 0x42)) || !payload_writes(ops).is_empty()
}

#[test]
fn a_stop_flag_set_before_the_start_stops_before_jump_dfu() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P)).stopped();
    let (r, events) = bench.run(&c);
    let e = r.expect_err("a stop requested up front stops the flash");
    assert!(matches!(e, FlashError::Interrupted { .. }), "got {e:?}");
    assert!(
        !sent_write_capable(&bench.ops()),
        "nothing write-capable is sent: {:?}",
        bench.ops()
    );
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
    assert!(
        !events
            .iter()
            .any(|p| matches!(p, FlashProgress::Erasing | FlashProgress::Writing { .. })),
        "{events:?}"
    );
}

#[test]
fn a_stop_flag_set_before_the_start_in_bootloader_mode_sends_no_descriptor() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P)).stopped();
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a stop requested up front stops the flash");
    assert!(matches!(e, FlashError::Interrupted { .. }), "got {e:?}");
    assert!(!sent_write_capable(&bench.ops()), "{:?}", bench.ops());
}

#[test]
fn a_stop_between_payload_writes_ends_after_that_writes_sync_and_sends_nothing_more() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    // Ctrl-C lands while write 2 is in flight.
    let bench = Bench::boot().script(boot_ok(P)).stop_after_writes(2);
    let (r, events) = bench.run(&c);
    let e = r.expect_err("an interrupted flash is not a success");
    assert!(matches!(e, FlashError::Interrupted { .. }), "got {e:?}");
    assert_recovery_text(&e);
    let ops = bench.boot_ops();
    assert_eq!(payload_writes(&ops).len(), 2, "no third write: {ops:?}");
    // Never between a write and its SYNC read: write 2 is followed by its
    // SYNC read, and that read is the very last thing done.
    let n = ops.len();
    assert!(
        matches!(&ops[n - 2], Op::Raw { data, .. } if data.len() == 4080),
        "write 2 comes second to last, got {:?}",
        ops[n - 2]
    );
    assert_eq!(ops[n - 1], Op::Read { timeout: ms(1000) }, "its SYNC read");
    assert!(
        !framed_types(&bench.ops()).contains(&0x08),
        "no JUMP_APROM after an interruption"
    );
    assert_eq!(bench.mode(), Some(DeviceMode::Bootloader));
    assert_eq!(stages(&events), vec!["erasing", "writing"]);
    let done = events.iter().rev().find_map(|p| match p {
        FlashProgress::Writing { done, .. } => Some(*done),
        _ => None,
    });
    assert_eq!(done, Some(2 * B), "both acknowledged writes are reported");
}

#[test]
fn a_stop_after_jump_dfu_still_leaves_the_recovery_text() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P)).stop_after_writes(1);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("an interrupted flash is not a success");
    assert!(matches!(e, FlashError::Interrupted { .. }), "got {e:?}");
    assert_recovery_text(&e);
    assert_eq!(payload_writes(&bench.ops()).len(), 1);
    assert_eq!(
        framed_types(&bench.ops())
            .iter()
            .filter(|t| **t == 0x09)
            .count(),
        1
    );
    assert!(!framed_types(&bench.ops()).contains(&0x08));
}

// ---- AC-V9SK54.1 / .5 (review fix): a jump may reset the meter mid-write ----

#[test]
fn a_jump_dfu_write_that_reports_disconnected_goes_on_to_the_bootloader_wait() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app()
        .dfu_fail(TransportError::Disconnected)
        .script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(
        matches!(r, Ok(FlashOutcome::Verified { .. })),
        "the meter resetting as it jumps is not fatal: got {r:?}"
    );
    assert_eq!(payload_writes(&bench.boot_ops()).len(), 4);
}

#[test]
fn a_jump_dfu_write_failing_with_another_error_is_still_fatal() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app()
        .dfu_fail(TransportError::Io("pipe error".into()))
        .script(boot_ok(P));
    let (r, _) = bench.run(&c);
    let e = r.expect_err("an I/O error on JUMP_DFU stops the flash");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert_eq!(e.code(), -110);
    assert!(e.to_string().contains("JUMP_DFU"), "{e}");
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
    let ops = bench.ops();
    assert!(!framed_types(&ops).contains(&0x42));
    assert!(payload_writes(&ops).is_empty());
}

#[test]
fn a_jump_aprom_write_that_reports_disconnected_goes_on_to_the_return_wait() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .script(boot_ok(P))
        .aprom_fail(TransportError::Disconnected);
    let (r, _) = bench.run(&c);
    assert_eq!(
        r,
        Ok(FlashOutcome::Verified {
            version: "2.0.7".into()
        }),
        "the meter resetting as it jumps is not fatal"
    );
    assert!(memread_addrs(&bench.ops()).contains(&(0x4420, 64)));
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
}

// ---- AC-V9SK54.1 / .5 (review fix): waits measured on the clock ----

/// Every sleep is at most one poll interval, and the wait polls at it.
fn assert_polls_at_the_interval(bench: &Bench) {
    let slept = bench.clock.0.borrow().slept.clone();
    assert!(
        slept.iter().all(|d| *d <= POLL_INTERVAL),
        "a sleep longer than POLL_INTERVAL: {slept:?}"
    );
    assert!(slept.contains(&POLL_INTERVAL), "{slept:?}");
}

#[test]
fn the_switch_wait_ends_on_clock_time_even_when_each_open_is_slow() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    // Each look at the device list takes 400 ms of clock time.
    let bench = Bench::app()
        .after_dfu(vec![Some(DeviceMode::Application)])
        .open_cost(400);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("a meter that stays in application mode stops the flash");
    assert!(matches!(e, FlashError::NotInBootloader { .. }), "got {e:?}");
    let waited = ms(bench.now_ms() - bench.jump_at(0x09));
    assert!(waited >= SWITCH_TIMEOUT, "gave up early after {waited:?}");
    let slack = (POLL_INTERVAL + ms(400)) * 2;
    assert!(
        waited <= SWITCH_TIMEOUT + slack,
        "waited {waited:?} of clock time, more than {SWITCH_TIMEOUT:?} + {slack:?}"
    );
    assert_polls_at_the_interval(&bench);
}

#[test]
fn the_return_wait_ends_on_clock_time_even_when_each_open_is_slow() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .script(boot_ok(P))
        .after_aprom(vec![Some(DeviceMode::Bootloader)])
        .open_cost(400);
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::PowerCycleNeeded)), "got {r:?}");
    let waited = ms(bench.now_ms() - bench.jump_at(0x08));
    assert!(waited >= RETURN_TIMEOUT, "gave up early after {waited:?}");
    let slack = (POLL_INTERVAL + ms(400)) * 2;
    assert!(
        waited <= RETURN_TIMEOUT + slack,
        "waited {waited:?} of clock time, more than {RETURN_TIMEOUT:?} + {slack:?}"
    );
    assert_polls_at_the_interval(&bench);
}

#[test]
fn a_slow_re_enumeration_inside_the_switch_time_still_flashes() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app()
        .after_dfu(vec![None, None, None, Some(DeviceMode::Bootloader)])
        .open_cost(400)
        .script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
}

// ---- AC-V9SK54.3 (review fix): the container is checked before sending ----

/// Run `c` from `bench` and expect a typed refusal of the file with
/// nothing write-capable sent.
fn assert_container_refused(bench: &Bench, c: &FirmwareContainer<'_>, what: &str) {
    let (r, events) = bench.run(c);
    let e = r.expect_err(what);
    assert!(
        matches!(e, FlashError::Product { .. }),
        "{what}: a typed refusal of the file, got {e:?}"
    );
    assert_eq!(e.code(), -100, "{what}");
    assert!(
        !e.to_string().to_lowercase().contains("forbidden"),
        "{what}: not a generic guard refusal: {e}"
    );
    assert!(
        e.to_string().to_lowercase().contains("size"),
        "{what}: names the size: {e}"
    );
    assert!(
        !sent_write_capable(&bench.ops()),
        "{what}: nothing write-capable is sent: {:?}",
        framed_types(&bench.ops())
    );
    assert!(
        !events
            .iter()
            .any(|p| matches!(p, FlashProgress::Switching | FlashProgress::Erasing)),
        "{what}: {events:?}"
    );
}

/// Containers that must be refused before anything write-capable is sent.
fn bad_containers() -> Vec<(&'static str, Vec<u8>, usize)> {
    let past_top = (MAX_APP_END - APP_START + 1) as usize;
    vec![
        ("size 0", container_bytes(100, 0), 100),
        (
            "end past MAX_APP_END",
            container_bytes(past_top, past_top),
            past_top,
        ),
        (
            "stored payload longer than size + one block",
            container_bytes(100 + B + 1, 100),
            100 + B + 1,
        ),
    ]
}

#[test]
fn a_bad_container_is_refused_in_bootloader_mode_before_the_descriptor() {
    for (what, bytes, stored) in bad_containers() {
        let c = parse(&bytes);
        let bench = Bench::boot().script(boot_ok(stored));
        assert_container_refused(&bench, &c, what);
    }
}

#[test]
fn a_bad_container_is_refused_in_application_mode_before_jump_dfu() {
    for (what, bytes, stored) in bad_containers() {
        let c = parse(&bytes);
        let bench = Bench::app().script(boot_ok(stored));
        assert_container_refused(&bench, &c, what);
        assert_eq!(bench.mode(), Some(DeviceMode::Application), "{what}");
    }
}

#[test]
fn a_stored_payload_of_exactly_size_plus_one_block_is_flashed() {
    let bytes = container_bytes(100 + B, 100);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(100 + B));
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Ok(FlashOutcome::Verified { .. })), "got {r:?}");
}

// ---- AC-V9SK54.2 (review fix): the descriptor write and its read ----

#[test]
fn a_failed_descriptor_write_is_a_write_error_and_no_read_follows() {
    for fail in [
        TransportError::Io("pipe error".into()),
        // A write that times out is not the expected read timeout.
        TransportError::Timeout,
    ] {
        let bytes = container_bytes(P, P);
        let c = parse(&bytes);
        let bench = Bench::boot().script(boot_ok(P)).desc_fail(fail.clone());
        let (r, _) = bench.run(&c);
        let e = r.expect_err("a failed descriptor write fails the flash");
        assert!(matches!(e, FlashError::Write { .. }), "{fail:?}: got {e:?}");
        assert_eq!(e.code(), -107);
        assert!(e.to_string().contains("descriptor"), "{e}");
        assert_recovery_text(&e);
        let ops = bench.boot_ops();
        assert!(
            matches!(ops.last(), Some(Op::Raw { data, .. }) if is_raw_descriptor(data)),
            "{fail:?}: the failed descriptor write is the last op, got {:?}",
            ops.last()
        );
        assert!(payload_writes(&ops).is_empty());
    }
}

#[test]
fn the_bootloader_timeouts_are_pinned() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(boot_ok(P));
    let (r, _) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    let ops = bench.boot_ops();
    // HEAD: 1000 ms.
    let head = ops
        .iter()
        .find_map(|o| match o {
            Op::Transact { req, timeout } if req[0] == 0x40 => Some(*timeout),
            _ => None,
        })
        .expect("HEAD was sent");
    assert_eq!(head, ms(1000), "HEAD waits 1000 ms");
    // The descriptor: a 1000 ms raw write, then the 20 ms read.
    let at = ops
        .iter()
        .position(|o| matches!(o, Op::Raw { data, .. } if is_raw_descriptor(data)))
        .expect("the descriptor was written raw");
    assert!(
        matches!(&ops[at], Op::Raw { timeout, .. } if *timeout == ms(1000)),
        "{:?}",
        ops[at]
    );
    assert_eq!(ops[at + 1], Op::Read { timeout: ms(20) });
    assert!(
        !ops.iter()
            .any(|o| matches!(o, Op::Transact { req, .. } if req[0] == 0x42)),
        "the descriptor is not a transact"
    );
    // Each payload write: 1000 ms; each SYNC read right after it: 1000 ms.
    let mut writes = 0;
    for (i, o) in ops.iter().enumerate() {
        if let Op::Raw { data, timeout } = o
            && data.len() != 4
            && !is_raw_descriptor(data)
        {
            writes += 1;
            assert_eq!(*timeout, ms(1000), "payload write {writes}");
            assert_eq!(
                ops.get(i + 1),
                Some(&Op::Read { timeout: ms(1000) }),
                "the SYNC read after write {writes}"
            );
        }
    }
    assert_eq!(writes, 4);
}

// ---- AC-V9SK54.1 / .5: each jump reports its reply and asks for a replug ----

#[test]
fn each_jump_is_followed_by_a_replug_prompt_naming_the_target_and_the_reply() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P));
    let (r, events) = bench.run(&c);
    assert!(r.is_ok(), "got {r:?}");
    let replugs: Vec<&FlashProgress> = events
        .iter()
        .filter(|e| matches!(e, FlashProgress::Replug { .. }))
        .collect();
    assert_eq!(
        replugs,
        vec![
            &FlashProgress::Replug {
                to: DeviceMode::Bootloader,
                reply: JumpReply::NoReply
            },
            &FlashProgress::Replug {
                to: DeviceMode::Application,
                reply: JumpReply::NoReply
            },
        ]
    );
    // The prompt follows the jump write, never precedes it: the first
    // replug comes after the JUMP_DFU, the second after the JUMP_APROM.
    let dfu = bench.jump_at(0x09);
    let aprom = bench.jump_at(0x08);
    assert!(dfu <= aprom);
}

/// After a jump the meter stays where it is for `secs` seconds (it does not
/// restart by itself), then is unplugged and comes back in `then`.
fn replugged_after(stay: DeviceMode, secs: u64, then: DeviceMode) -> Vec<Option<DeviceMode>> {
    let polls = (secs * 1000 / POLL_INTERVAL.as_millis() as u64) as usize;
    let mut v = vec![Some(stay); polls];
    v.extend([None, None, Some(then)]);
    v
}

#[test]
fn a_jump_dfu_that_takes_effect_at_the_next_power_up_waits_for_the_replug() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::app().script(boot_ok(P)).after_dfu(replugged_after(
        DeviceMode::Application,
        90,
        DeviceMode::Bootloader,
    ));
    let (r, _) = bench.run(&c);
    assert_eq!(
        r,
        Ok(FlashOutcome::Verified {
            version: "2.0.7".into()
        })
    );
}

#[test]
fn after_jump_aprom_a_replug_into_the_application_is_waited_for_and_verified() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot()
        .script(boot_ok(P))
        .after_aprom(replugged_after(
            DeviceMode::Bootloader,
            90,
            DeviceMode::Application,
        ));
    let (r, _) = bench.run(&c);
    assert_eq!(
        r,
        Ok(FlashOutcome::Verified {
            version: "2.0.7".into()
        })
    );
}

// ---- AC-V9SK54.2: the real bootloader's CONNECT and HEAD (stage 1) ----

/// The bootloader frames recorded in stage 1 on the real KM003C
/// (`qa-runs/V9SK56.1-20261026-0928`, trace-boot.txt) `[device]`: the first
/// CONNECT got no reply, the second ACCEPT; HEAD came back with the extend
/// bit set, the request id echoed and field 0.
const STAGE1_CONNECT_1: [u8; 4] = [0x02, 0x01, 0x00, 0x00];
const STAGE1_CONNECT_2: [u8; 4] = [0x02, 0x02, 0x00, 0x00];
const STAGE1_ACCEPT: [u8; 4] = [0x05, 0x02, 0x00, 0x00];
const STAGE1_HEAD_REQ: [u8; 20] = [
    0x40, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const STAGE1_HEAD_REPLY: [u8; 20] = [
    0xc0, 0x03, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    0x00, 0x00, 0x00, 0x00,
];

/// `boot_ok(stored)` with its CONNECT/HEAD replaced by the stage-1 frames.
fn stage1_script(stored: usize) -> Vec<Boot> {
    let mut v = vec![
        Boot::Timeout,
        Boot::Exact(STAGE1_ACCEPT.to_vec()),
        Boot::Exact(STAGE1_HEAD_REPLY.to_vec()),
    ];
    v.extend(boot_ok(stored).into_iter().skip(2));
    v
}

#[test]
fn the_real_bootloader_head_reply_decodes_to_field_zero() {
    let head = HeadReply::decode(&STAGE1_HEAD_REPLY).expect("type 0x40 with the extend bit");
    assert_eq!(head.old_firmware_field(), 0);
    assert!(!head.update_required());
}

#[test]
fn the_stage1_bootloader_frames_pass_connect_and_head_and_the_flash_runs() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(stage1_script(P));
    let (r, _) = bench.run(&c);
    assert_eq!(
        r,
        Ok(FlashOutcome::Verified {
            version: "2.0.7".into()
        })
    );
    let ops = bench.ops();
    assert_eq!(
        ops[..4],
        [
            Op::Transact {
                req: STAGE1_CONNECT_1.to_vec(),
                timeout: ms(1000)
            },
            Op::Recover,
            Op::Transact {
                req: STAGE1_CONNECT_2.to_vec(),
                timeout: ms(1000)
            },
            Op::Transact {
                req: STAGE1_HEAD_REQ.to_vec(),
                timeout: ms(1000)
            },
        ],
        "the same frames as the stage-1 trace"
    );
    assert!(is_raw_descriptor(ops[4].written().expect("the descriptor")));
}

#[test]
fn an_unanswered_connect_is_retried_once_after_a_recover_and_not_again() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(vec![Boot::Timeout, Boot::Timeout]);
    let (r, _) = bench.run(&c);
    let e = r.expect_err("two unanswered CONNECTs");
    assert!(matches!(e, FlashError::Read { .. }), "got {e:?}");
    assert_eq!(e.code(), -108);
    assert_recovery_text(&e);
    let types: Vec<u8> = framed_types(&bench.ops());
    assert_eq!(types, vec![0x02, 0x02], "two CONNECTs, nothing else");
    assert_eq!(bench.ops().iter().filter(|o| **o == Op::Recover).count(), 1);
}

#[test]
fn a_rejected_connect_is_not_retried() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let bench = Bench::boot().script(vec![Boot::Frame(reject())]);
    let (r, _) = bench.run(&c);
    assert!(matches!(r, Err(FlashError::Read { .. })), "got {r:?}");
    assert_eq!(framed_types(&bench.ops()), vec![0x02], "one CONNECT only");
}

// ---- AC-V9SK54.2: the real bootloader's erase ACK (stage 3, run 2) ----

/// The frames recorded in stage 3 run 2 on the real KM003C
/// (`qa-runs/V9SK56.3-20261026-1305`, flash-trace.txt) `[device]`: CONNECT
/// answered first time, HEAD with id 2 and field 0, and the descriptor
/// (id 3) ACKed with `05 03 00 00` during the erase wait.
const RUN2_CONNECT: [u8; 4] = [0x02, 0x01, 0x00, 0x00];
const RUN2_ACCEPT: [u8; 4] = [0x05, 0x01, 0x00, 0x00];
const RUN2_HEAD_REQ: [u8; 20] = [
    0x40, 0x02, 0x00, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const RUN2_HEAD_REPLY: [u8; 20] = [
    0xc0, 0x02, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    0x00, 0x00, 0x00, 0x00,
];
const RUN2_ERASE_ACK: [u8; 4] = [0x05, 0x03, 0x00, 0x00];

#[test]
fn the_run2_bootloader_frames_flash_through_the_early_erase_ack() {
    let bytes = container_bytes(P, P);
    let c = parse(&bytes);
    let chunks = P.div_ceil(B);
    let mut script = vec![
        Boot::Exact(RUN2_ACCEPT.to_vec()),
        Boot::Exact(RUN2_HEAD_REPLY.to_vec()),
        // The descriptor read times out, the first erase read gets the ACK.
        Boot::Timeout,
        Boot::Exact(RUN2_ERASE_ACK.to_vec()),
    ];
    script.extend((0..chunks).map(|_| Boot::Frame(sync())));
    script.push(Boot::Frame(finished()));
    let bench = Bench::boot().script(script);
    let (r, _) = bench.run(&c);
    assert_eq!(
        r,
        Ok(FlashOutcome::Verified {
            version: "2.0.7".into()
        })
    );
    let ops = bench.ops();
    assert_eq!(
        ops[..2],
        [
            Op::Transact {
                req: RUN2_CONNECT.to_vec(),
                timeout: ms(1000)
            },
            Op::Transact {
                req: RUN2_HEAD_REQ.to_vec(),
                timeout: ms(1000)
            },
        ],
        "the same frames as the run-2 trace"
    );
    let desc = ops[2].written().expect("the descriptor");
    assert!(is_raw_descriptor(desc));
    assert_eq!(desc[1], 0x03, "the descriptor id the ACK echoes");
    assert_eq!(payload_writes(&ops).len(), chunks);
}

// ---- AC-V9SK54.9: finding the meter again by its USB serial ----
//
// The real device list cannot be driven without a meter, so the opener's
// choice is tested through the pure selection functions it uses:
// `find_flashed_meter` (one lookup) and `follow_flashed_meter` (a lookup
// that latches the serial of the first meter found when none was chosen;
// `BulkFlashOpener` keeps that serial between opens).

use powerz_protocol::transport::discovery::{PID_KM003C, find_flashed_meter, follow_flashed_meter};

fn entry(serial: Option<&str>, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: PID_KM003C,
        serial: serial.map(str::to_owned),
        product: None,
        interface_string: None,
        bus: "1".into(),
        address,
    }
}

fn addr(found: Option<&DeviceEntry>) -> Option<u8> {
    found.map(|d| d.address)
}

#[test]
fn with_a_serial_only_the_meter_with_that_serial_is_found() {
    let devices = vec![entry(Some("111111"), 1), entry(Some("222222"), 2)];
    let found = find_flashed_meter(&devices, Some("222222")).expect("a match");
    assert_eq!(addr(found), Some(2));
}

#[test]
fn with_a_serial_a_single_other_meter_is_never_picked() {
    for other in [entry(Some("BOOT0001"), 7), entry(None, 9)] {
        let devices = vec![other];
        let found = find_flashed_meter(&devices, Some("222222"))
            .expect("not an error: keep waiting for the meter");
        assert_eq!(
            addr(found),
            None,
            "{:?} is another meter",
            devices[0].serial
        );
    }
}

#[test]
fn with_a_serial_several_other_meters_are_none_not_ambiguous() {
    let devices = vec![entry(Some("333333"), 3), entry(None, 4)];
    let found = find_flashed_meter(&devices, Some("222222"))
        .expect("not an error: the chosen meter is just not back yet");
    assert_eq!(addr(found), None);
}

#[test]
fn without_a_serial_the_single_attached_meter_is_found() {
    for only in [entry(Some("111111"), 1), entry(None, 9)] {
        let devices = vec![only];
        let found = find_flashed_meter(&devices, None).expect("a match");
        assert_eq!(addr(found), Some(devices[0].address));
    }
}

#[test]
fn without_a_serial_several_meters_are_ambiguous_naming_them() {
    let devices = vec![entry(Some("333333"), 3), entry(Some("444444"), 4)];
    let e = find_flashed_meter(&devices, None).expect_err("two meters cannot be told apart");
    let msg = e.to_string();
    assert!(
        msg.contains("333333") && msg.contains("444444"),
        "names them: {msg}"
    );
}

#[test]
fn no_meter_at_all_is_none_with_or_without_a_serial() {
    assert_eq!(
        addr(find_flashed_meter(&[], Some("222222")).expect("ok")),
        None
    );
    assert_eq!(addr(find_flashed_meter(&[], None).expect("ok")), None);
}

#[test]
fn without_a_chosen_serial_the_first_meters_serial_is_kept() {
    let mut serial: Option<String> = None;
    let first = vec![entry(Some("111111"), 1)];
    let found = follow_flashed_meter(&first, &mut serial).expect("a match");
    assert_eq!(addr(found), Some(1));
    assert_eq!(serial.as_deref(), Some("111111"), "the serial is latched");
    // After the switch another meter alone is attached: never picked.
    let other = vec![entry(Some("222222"), 2)];
    let found = follow_flashed_meter(&other, &mut serial).expect("keep waiting");
    assert_eq!(addr(found), None, "another meter is never picked");
    assert_eq!(serial.as_deref(), Some("111111"));
    // A meter without a serial is not the first one either.
    let anon = vec![entry(None, 5)];
    let found = follow_flashed_meter(&anon, &mut serial).expect("keep waiting");
    assert_eq!(addr(found), None);
    // The first meter back (in bootloader mode, same serial) among others.
    let back = vec![entry(Some("222222"), 2), entry(Some("111111"), 8)];
    let found = follow_flashed_meter(&back, &mut serial).expect("a match");
    assert_eq!(addr(found), Some(8), "found again by its serial");
}

#[test]
fn a_chosen_serial_is_kept_and_never_replaced() {
    let mut serial = Some("222222".to_owned());
    let other = vec![entry(Some("111111"), 1)];
    let found = follow_flashed_meter(&other, &mut serial).expect("keep waiting");
    assert_eq!(addr(found), None);
    assert_eq!(serial.as_deref(), Some("222222"));
    let back = vec![entry(Some("222222"), 2)];
    let found = follow_flashed_meter(&back, &mut serial).expect("a match");
    assert_eq!(addr(found), Some(2));
    assert_eq!(serial.as_deref(), Some("222222"));
}

#[test]
fn a_first_meter_without_a_serial_falls_back_to_the_single_meter_rule() {
    let mut serial: Option<String> = None;
    let first = vec![entry(None, 1)];
    let found = follow_flashed_meter(&first, &mut serial).expect("a match");
    assert_eq!(addr(found), Some(1));
    assert_eq!(serial, None, "nothing to latch");
    // Still nothing latched: the single attached meter is found again...
    let again = vec![entry(None, 6)];
    let found = follow_flashed_meter(&again, &mut serial).expect("a match");
    assert_eq!(addr(found), Some(6));
    // ...and several are ambiguous.
    let several = vec![entry(None, 6), entry(None, 7)];
    assert!(follow_flashed_meter(&several, &mut serial).is_err());
}

#[test]
fn without_a_chosen_serial_several_meters_at_the_first_open_are_ambiguous() {
    let mut serial: Option<String> = None;
    let devices = vec![entry(Some("111111"), 1), entry(Some("222222"), 2)];
    assert!(follow_flashed_meter(&devices, &mut serial).is_err());
    assert_eq!(serial, None, "nothing latched from an ambiguous list");
}
