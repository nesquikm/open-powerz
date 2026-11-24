//! FR DJYNRP in the flash engine, over a hand-written fake bootloader.
//!
//! - AC .2: without the opt-in the old bootloader is refused before any
//!   frame, and the refusal names the flag and says it is experimental;
//!   `jump` refuses it even with the opt-in.
//! - AC .4: with `FlashAck::with_experimental_old_bootloader`, the download
//!   on the old bootloader takes a HEAD reply by type whatever its id (field
//!   still 0), an erase ACCEPT whatever its id, waits 2 s for the first
//!   CONNECT and retries it once with no pipe recovery, and reads past stray
//!   ACCEPTs (late CONNECT answers) so none is taken for the erase. On
//!   bootloader 2.5 the HEAD id check stays fail-closed and the sequence is
//!   the same with and without the opt-in.
//! - AC .6: the container checks, the model check and the 0x4000 descriptor
//!   still apply on the old bootloader.
//!
//! The fake models a bootloader that numbers its replies itself (own ids,
//! from 0x50) or one that echoes the request id (2.5), with a FIFO of
//! replies that arrive late. No real device is opened; every container is
//! synthetic.

mod common;
#[path = "support/fw_container.rs"]
mod fw_container;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::FakeClock;
use fw_container::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::firmware::flash::{
    self, FlashAck, FlashError, FlashOutcome, FlashTransport, JumpTarget, LEGACY_BOOTLOADER_HELP,
    Meter, MeterOpener,
};
use powerz_protocol::transport::discovery::Model;
use powerz_protocol::transport::{Error as TransportError, Transport};

// ---------------------------------------------------------------- helpers

/// One data-phase block, written out independently of the library.
const B: usize = 4080;
/// 3 whole blocks plus a 100-byte remainder.
const P: usize = 3 * B + 100;
/// The flag the refusal must name.
const FLAG: &str = "--experimental-old-bootloader";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 37 + 11) as u8 | 0x80).collect()
}

/// A KM003C 2.0.7 container: `P + 16` stored bytes declaring `P`.
fn container_bytes() -> Vec<u8> {
    ContainerBuilder::new()
        .payload(&payload(P + 16))
        .field(SIZE, &P.to_string())
        .build()
}

fn parse(bytes: &[u8]) -> FirmwareContainer<'_> {
    FirmwareContainer::parse(bytes).expect("a synthetic container parses")
}

fn accept(id: u8) -> Vec<u8> {
    vec![0x05, id, 0x00, 0x00]
}

fn head_reply(id: u8, field: u32) -> Vec<u8> {
    let mut r = vec![0x40, id, 0x00, 0x01];
    r.extend(field.to_le_bytes());
    r.extend([0u8; 12]);
    r
}

/// One transfer the fake saw.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Transact { req: Vec<u8>, timeout: Duration },
    Read { timeout: Duration },
    Raw { data: Vec<u8>, timeout: Duration },
    Recover,
}

/// How the fake numbers its replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplyIds {
    /// Its own counter, from 0x50 (bootloader 2.1 in the field).
    Own,
    /// The request's id (bootloader 2.5).
    Echo,
}

/// How the fake treats the first CONNECT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstConnect {
    /// Answered at once.
    Answered,
    /// Never answered.
    Silent,
    /// Not answered within the wait; its ACCEPT is what the next transfer
    /// returns.
    Late,
}

#[derive(Debug)]
struct State {
    mode: DeviceMode,
    model: Model,
    ids: ReplyIds,
    own: u8,
    ops: Vec<Op>,
    pending: VecDeque<Vec<u8>>,
    first_connect: FirstConnect,
    every_connect_silent: bool,
    connects: usize,
    head_field: u32,
    /// HEAD replies carry this id instead (2.5 fail-closed test).
    head_id: Option<u8>,
    /// Stray ACCEPTs that arrive right after the HEAD reply.
    strays_after_head: usize,
    /// Other replies that arrive right after the HEAD reply (and strays).
    after_head: Vec<Vec<u8>>,
    /// The id the erase-done ACCEPT carried.
    erase_reply_id: Option<u8>,
    /// A late CONNECT answer that only a read waiting at least 1 s sees.
    slow_stray: bool,
    desc_id: u8,
    size: usize,
    erasing: bool,
    erase_done: bool,
    erase_reads: usize,
    /// The erase ends at this read after the descriptor (1-based); `None`:
    /// at the first read that waits at least 100 ms.
    erase_at_read: Option<usize>,
    received: usize,
    finished_sent: bool,
    /// Payload writes that came before the erase was done.
    payload_before_erase: usize,
    jumped: bool,
    recover_ok: bool,
}

impl State {
    fn reply_id(&mut self, request_id: u8) -> u8 {
        match self.ids {
            ReplyIds::Echo => request_id,
            ReplyIds::Own => {
                let id = self.own;
                self.own = self.own.wrapping_add(1);
                id
            }
        }
    }

    /// Deliver `reply` behind whatever is already queued.
    fn deliver(&mut self, reply: Vec<u8>) -> Vec<u8> {
        if self.pending.is_empty() {
            reply
        } else {
            self.pending.push_back(reply);
            self.pending.pop_front().expect("not empty")
        }
    }
}

#[derive(Debug, Clone)]
struct Pipe(Rc<RefCell<State>>);

impl Transport for Pipe {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Transact {
            req: request.to_vec(),
            timeout,
        });
        let id = request.get(1).copied().unwrap_or(0);
        match request.first().map(|b| b & 0x7F) {
            Some(0x02) => {
                s.connects += 1;
                let silent = s.every_connect_silent
                    || (s.connects == 1 && s.first_connect == FirstConnect::Silent);
                if silent {
                    return s.pending.pop_front().ok_or(TransportError::Timeout);
                }
                let reply = accept(s.reply_id(id));
                if s.connects == 1 && s.first_connect == FirstConnect::Late {
                    s.pending.push_back(reply);
                    return Err(TransportError::Timeout);
                }
                Ok(s.deliver(reply))
            }
            Some(0x40) => {
                let rid = s.reply_id(id);
                let rid = s.head_id.unwrap_or(rid);
                let field = s.head_field;
                let r = s.deliver(head_reply(rid, field));
                for _ in 0..s.strays_after_head {
                    let stray = accept(s.reply_id(0));
                    s.pending.push_back(stray);
                }
                let after = std::mem::take(&mut s.after_head);
                s.pending.extend(after);
                Ok(r)
            }
            _ => s.pending.pop_front().ok_or(TransportError::Timeout),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Read { timeout });
        if s.slow_stray && timeout >= ms(1000) {
            s.slow_stray = false;
            let id = s.reply_id(0);
            return Ok(accept(id));
        }
        if let Some(r) = s.pending.pop_front() {
            return Ok(r);
        }
        if s.erasing && !s.erase_done {
            s.erase_reads += 1;
            let done = match s.erase_at_read {
                Some(n) => s.erase_reads >= n,
                None => timeout >= ms(100),
            };
            if done {
                s.erase_done = true;
                let d = s.desc_id;
                let id = s.reply_id(d);
                s.erase_reply_id = Some(id);
                return Ok(accept(id));
            }
            return Err(TransportError::Timeout);
        }
        if s.erase_done && s.received >= s.size && !s.finished_sent {
            s.finished_sent = true;
            let id = s.reply_id(0);
            return Ok(vec![0x07, id, 0x00, 0x00]);
        }
        Err(TransportError::Timeout)
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Recover);
        if s.recover_ok {
            Ok(())
        } else {
            Err(TransportError::Io("simulated pipe recovery failed".into()))
        }
    }
}

impl FlashTransport for Pipe {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Raw {
            data: data.to_vec(),
            timeout,
        });
        if data.len() == 4 && data[0] == 0x08 {
            s.jumped = true;
        } else if data.len() == 4 + 64 && data[0] == 0x42 {
            s.desc_id = data[1];
            s.size = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
            s.erasing = true;
        } else if s.erase_done {
            s.received += data.len();
            let id = s.reply_id(0);
            s.pending.push_back(vec![0x01, id, 0x00, 0x00]);
        } else {
            s.payload_before_erase += 1;
        }
        Ok(())
    }
}

/// Opens the fake meter until JUMP_APROM; after it, nothing is attached
/// (the flash then ends asking for a power cycle).
struct Opener(Rc<RefCell<State>>);

impl MeterOpener for Opener {
    type Pipe = Pipe;

    fn open(&mut self) -> Result<Option<Meter<Pipe>>, TransportError> {
        let s = self.0.borrow();
        if s.jumped {
            return Ok(None);
        }
        Ok(Some(Meter {
            mode: s.mode,
            model: s.model,
            pipe: Pipe(self.0.clone()),
        }))
    }
}

struct Fake(Rc<RefCell<State>>);

impl Fake {
    fn new(mode: DeviceMode, ids: ReplyIds) -> Fake {
        Fake(Rc::new(RefCell::new(State {
            mode,
            model: Model::Km003c,
            ids,
            own: 0x50,
            ops: Vec::new(),
            pending: VecDeque::new(),
            first_connect: FirstConnect::Answered,
            every_connect_silent: false,
            connects: 0,
            head_field: 0,
            head_id: None,
            strays_after_head: 0,
            after_head: Vec::new(),
            erase_reply_id: None,
            slow_stray: false,
            desc_id: 0,
            size: usize::MAX,
            erasing: false,
            erase_done: false,
            erase_reads: 0,
            erase_at_read: None,
            received: 0,
            finished_sent: false,
            payload_before_erase: 0,
            jumped: false,
            recover_ok: ids == ReplyIds::Echo,
        })))
    }

    /// The old bootloader that numbers its own replies.
    fn old() -> Fake {
        Fake::new(DeviceMode::LegacyBootloader, ReplyIds::Own)
    }

    /// Bootloader 2.5, echoing the request ids.
    fn bl25() -> Fake {
        Fake::new(DeviceMode::Bootloader, ReplyIds::Echo)
    }

    fn with(self, f: impl FnOnce(&mut State)) -> Fake {
        f(&mut self.0.borrow_mut());
        self
    }

    fn ops(&self) -> Vec<Op> {
        self.0.borrow().ops.clone()
    }

    fn run_with(&self, ack: FlashAck, bytes: &[u8]) -> Result<FlashOutcome, FlashError> {
        let c = parse(bytes);
        let stop = AtomicBool::new(false);
        flash::flash(
            ack,
            &c,
            &mut Opener(self.0.clone()),
            FakeClock::new(1_790_000_000_000),
            &stop,
            &mut |_| {},
        )
    }

    /// Flash with the opt-in.
    fn run(&self) -> Result<FlashOutcome, FlashError> {
        self.run_with(opted_in(), &container_bytes())
    }
}

fn opted_in() -> FlashAck {
    FlashAck::acknowledge().with_experimental_old_bootloader()
}

fn is_connect(op: &Op) -> bool {
    matches!(op, Op::Transact { req, .. } if req.len() == 4 && req[0] == 0x02)
}

fn is_head(op: &Op) -> bool {
    matches!(op, Op::Transact { req, .. } if req[0] == 0x40)
}

fn is_descriptor(op: &Op) -> bool {
    matches!(op, Op::Raw { data, .. } if data.len() == 68 && data[0] == 0x42)
}

fn payload_writes(ops: &[Op]) -> usize {
    ops.iter()
        .filter(|o| matches!(o, Op::Raw { data, .. } if data.len() > 68))
        .count()
}

fn sent_jump_aprom(ops: &[Op]) -> bool {
    ops.iter()
        .any(|o| matches!(o, Op::Raw { data, .. } if data.len() == 4 && data[0] == 0x08))
}

/// The flash ran to its end: the image accepted, then JUMP_APROM.
fn assert_flashed(name: &str, r: &Result<FlashOutcome, FlashError>, fake: &Fake) {
    assert!(r.is_ok(), "{name}: the flash must succeed: {r:?}");
    let ops = fake.ops();
    assert_eq!(payload_writes(&ops), 4, "{name}: four payload writes");
    assert!(sent_jump_aprom(&ops), "{name}: JUMP_APROM sent");
    assert!(fake.0.borrow().finished_sent, "{name}: FINISHED reached");
}

// ---------------------------------------------------------------- AC-DJYNRP.2

#[test]
fn djynrp_without_the_opt_in_the_old_bootloader_is_refused_before_any_frame() {
    let fake = Fake::old();
    let r = fake.run_with(FlashAck::acknowledge(), &container_bytes());
    assert!(matches!(r, Err(FlashError::LegacyBootloader)), "{r:?}");
    assert!(fake.ops().is_empty(), "nothing sent: {:?}", fake.ops());
}

#[test]
fn djynrp_the_refusal_names_the_flag() {
    let msg = FlashError::LegacyBootloader.to_string();
    assert!(
        msg.contains(FLAG),
        "the refusal does not name {FLAG}: {msg}"
    );
    assert!(
        LEGACY_BOOTLOADER_HELP.contains(FLAG),
        "{LEGACY_BOOTLOADER_HELP}"
    );
}

#[test]
fn djynrp_the_refusal_says_the_path_is_experimental() {
    let msg = FlashError::LegacyBootloader.to_string();
    // The flag's own name does not count.
    let rest = msg.replace(FLAG, "").to_lowercase();
    assert!(
        rest.contains("experimental"),
        "not called experimental: {msg}"
    );
}

#[test]
fn djynrp_jump_keeps_refusing_the_old_bootloader_with_the_opt_in() {
    for target in [JumpTarget::Application, JumpTarget::Bootloader] {
        let fake = Fake::old();
        let stop = AtomicBool::new(false);
        let r = flash::jump(
            opted_in(),
            target,
            &mut Opener(fake.0.clone()),
            FakeClock::new(1_790_000_000_000),
            &stop,
            &mut |_| {},
        );
        assert!(
            matches!(r, Err(FlashError::LegacyBootloader)),
            "{target:?}: {r:?}"
        );
        assert!(fake.ops().is_empty(), "{target:?}: {:?}", fake.ops());
    }
}

// ---------------------------------------------------------------- AC-DJYNRP.4

#[test]
fn djynrp_old_boot_flashes_with_own_reply_ids_everywhere() {
    let fake = Fake::old();
    let r = fake.run();
    assert_flashed("own ids", &r, &fake);
}

#[test]
fn djynrp_old_boot_takes_a_head_reply_by_type_whatever_its_id() {
    // The HEAD reply carries an id no request had.
    let fake = Fake::old().with(|s| s.head_id = Some(0xA7));
    let r = fake.run();
    assert_flashed("HEAD id 0xa7", &r, &fake);
    assert!(
        fake.ops().iter().any(is_descriptor),
        "the descriptor followed"
    );
}

#[test]
fn djynrp_old_boot_takes_an_id_0_head_reply() {
    let fake = Fake::old().with(|s| s.head_id = Some(0x00));
    let r = fake.run();
    assert_flashed("HEAD id 0", &r, &fake);
}

#[test]
fn djynrp_old_boot_head_field_must_still_be_0() {
    let fake = Fake::old().with(|s| s.head_field = 0x1000);
    let r = fake.run();
    assert!(
        matches!(r, Err(FlashError::WorkMode { field: 0x1000, .. })),
        "a work mode error: {r:?}"
    );
    let ops = fake.ops();
    assert!(!ops.iter().any(is_descriptor), "no descriptor: {ops:?}");
    assert_eq!(payload_writes(&ops), 0);
}

#[test]
fn djynrp_old_boot_takes_an_early_erase_accept_whatever_its_id() {
    // The erase ends on the first 10 ms erase read (read 2 after the
    // descriptor), with the meter's own id.
    let fake = Fake::old().with(|s| s.erase_at_read = Some(2));
    let r = fake.run();
    assert_flashed("early erase ACCEPT", &r, &fake);
    assert_eq!(fake.0.borrow().payload_before_erase, 0);
}

#[test]
fn djynrp_old_boot_takes_a_final_erase_accept_whatever_its_id() {
    let fake = Fake::old();
    let r = fake.run();
    assert_flashed("final erase ACCEPT", &r, &fake);
    let s = fake.0.borrow();
    assert_ne!(
        s.erase_reply_id,
        Some(s.desc_id),
        "the fixture's erase ACCEPT id really differs from the descriptor's"
    );
}

#[test]
fn djynrp_old_boot_waits_2_s_for_the_first_connect() {
    let fake = Fake::old();
    let _ = fake.run();
    match fake.ops().first() {
        Some(Op::Transact { req, timeout }) => {
            assert_eq!(req[0], 0x02, "CONNECT first");
            assert_eq!(*timeout, ms(2000), "the first CONNECT waits 2 s");
        }
        other => panic!("expected CONNECT first, got {other:?}"),
    }
}

#[test]
fn djynrp_old_boot_retries_an_unanswered_first_connect_once_without_a_recover() {
    let fake = Fake::old().with(|s| s.first_connect = FirstConnect::Silent);
    let r = fake.run();
    assert_flashed("silent first CONNECT", &r, &fake);
    let ops = fake.ops();
    let before_head: Vec<&Op> = ops.iter().take_while(|o| !is_head(o)).collect();
    assert!(
        !before_head.iter().any(|o| matches!(o, Op::Recover)),
        "no pipe recovery on the old bootloader: {before_head:?}"
    );
    let connects: Vec<&Op> = before_head.into_iter().filter(|o| is_connect(o)).collect();
    assert_eq!(connects.len(), 2, "CONNECT sent twice: {connects:?}");
    for c in connects {
        assert!(
            matches!(c, Op::Transact { timeout, .. } if *timeout == ms(2000)),
            "each CONNECT waits 2 s: {c:?}"
        );
    }
}

#[test]
fn djynrp_old_boot_retries_connect_only_once() {
    let fake = Fake::old().with(|s| s.every_connect_silent = true);
    let r = fake.run();
    assert!(matches!(r, Err(FlashError::Read { .. })), "{r:?}");
    let ops = fake.ops();
    assert_eq!(ops.iter().filter(|o| is_connect(o)).count(), 2, "{ops:?}");
    assert!(!ops.iter().any(|o| matches!(o, Op::Recover)), "{ops:?}");
    assert!(!ops.iter().any(is_head), "no HEAD after a failed CONNECT");
}

#[test]
fn djynrp_old_boot_a_late_connect_answer_is_not_taken_for_the_erase() {
    // The first CONNECT's ACCEPT arrives late: the retry gets it, and the
    // retry's own ACCEPT is still queued when HEAD goes out.
    let fake = Fake::old().with(|s| s.first_connect = FirstConnect::Late);
    let r = fake.run();
    assert_flashed("late first CONNECT", &r, &fake);
    assert_eq!(
        fake.0.borrow().payload_before_erase,
        0,
        "no payload write before the erase was done"
    );
}

#[test]
fn djynrp_old_boot_reads_past_a_stray_accept_in_place_of_the_head_reply() {
    // A stray ACCEPT already queued: HEAD's transfer returns it, the HEAD
    // reply comes next.
    let fake = Fake::old().with(|s| s.pending.push_back(accept(0x4F)));
    let r = fake.run();
    assert_flashed("stray before HEAD", &r, &fake);
}

#[test]
fn djynrp_old_boot_drains_stray_accepts_before_the_descriptor() {
    let fake = Fake::old().with(|s| s.strays_after_head = 2);
    let r = fake.run();
    assert_flashed("two strays after HEAD", &r, &fake);
    assert_eq!(fake.0.borrow().payload_before_erase, 0);
    let ops = fake.ops();
    let head = ops.iter().position(is_head).expect("HEAD sent");
    let desc = ops.iter().position(is_descriptor).expect("descriptor sent");
    let between = &ops[head + 1..desc];
    assert!(
        (3..=4).contains(&between.len()),
        "two strays and one expired read, at most four: {between:?}"
    );
    for op in between {
        assert!(
            matches!(op, Op::Read { timeout } if *timeout <= ms(50)),
            "only short reads before the descriptor: {op:?}"
        );
    }
}

#[test]
fn djynrp_old_boot_another_reply_type_before_the_descriptor_is_a_write_error() {
    // A REJECT arrives right after the HEAD reply.
    let fake = Fake::old().with(|s| s.after_head.push(vec![0x06, 0x4D, 0x00, 0x00]));
    let r = fake.run();
    assert!(matches!(r, Err(FlashError::Write { .. })), "{r:?}");
    assert!(!fake.ops().iter().any(is_descriptor), "{:?}", fake.ops());
}

#[test]
fn djynrp_old_boot_more_strays_than_the_drain_takes_is_a_write_error() {
    // Five stray ACCEPTs after HEAD: the drain must not leave one behind
    // for the erase wait to take as erase done.
    let fake = Fake::old().with(|s| s.strays_after_head = 5);
    let r = fake.run();
    assert!(matches!(r, Err(FlashError::Write { .. })), "{r:?}");
    assert!(!fake.ops().iter().any(is_descriptor), "{:?}", fake.ops());
}

#[test]
fn djynrp_old_boot_a_retried_connect_waits_out_its_late_answer_before_the_descriptor() {
    // The first CONNECT goes unanswered and is retried; its own answer
    // shows up only to a long read, and must be drained, not taken for
    // the erase.
    let fake = Fake::old().with(|s| {
        s.first_connect = FirstConnect::Silent;
        s.slow_stray = true;
    });
    let r = fake.run();
    assert_flashed("a retried CONNECT with a slow late answer", &r, &fake);
    assert_eq!(fake.0.borrow().payload_before_erase, 0);
    let ops = fake.ops();
    let head = ops.iter().position(is_head).expect("HEAD sent");
    let desc = ops.iter().position(is_descriptor).expect("descriptor sent");
    assert!(
        ops[head + 1..desc]
            .iter()
            .any(|op| matches!(op, Op::Read { timeout } if *timeout >= ms(1000))),
        "a long drain read after a retried CONNECT: {ops:?}"
    );
}

// ---- bootloader 2.5: unchanged ----

#[test]
fn djynrp_bl25_head_with_another_id_stays_a_read_error_with_the_opt_in() {
    let fake = Fake::bl25().with(|s| s.head_id = Some(0x33));
    let r = fake.run();
    match &r {
        Err(FlashError::Read { detail }) => assert!(
            detail.contains("the reply id is not the request id"),
            "{detail}"
        ),
        other => panic!("expected the HEAD id read error, got {other:?}"),
    }
    let ops = fake.ops();
    assert!(!ops.iter().any(is_descriptor), "no descriptor: {ops:?}");
}

#[test]
fn djynrp_bl25_sequence_is_identical_with_and_without_the_opt_in() {
    let bytes = container_bytes();
    let plain = Fake::bl25();
    let r1 = plain.run_with(FlashAck::acknowledge(), &bytes);
    let opted = Fake::bl25();
    let r2 = opted.run_with(opted_in(), &bytes);
    assert!(r1.is_ok() && r2.is_ok(), "{r1:?} / {r2:?}");
    assert_eq!(opted.ops(), plain.ops(), "frames and timeouts differ");
}

#[test]
fn djynrp_bl25_silent_first_connect_is_identical_with_and_without_the_opt_in() {
    let bytes = container_bytes();
    let silent = |s: &mut State| s.first_connect = FirstConnect::Silent;
    let plain = Fake::bl25().with(silent);
    let r1 = plain.run_with(FlashAck::acknowledge(), &bytes);
    let opted = Fake::bl25().with(silent);
    let r2 = opted.run_with(opted_in(), &bytes);
    assert_eq!(format!("{r1:?}"), format!("{r2:?}"));
    assert_eq!(
        opted.ops(),
        plain.ops(),
        "frames, timeouts and recovers differ"
    );
    assert!(
        plain.ops().iter().any(|o| matches!(o, Op::Recover)),
        "2.5 still recovers"
    );
}

// ---------------------------------------------------------------- AC-DJYNRP.6

#[test]
fn djynrp_old_boot_model_check_still_applies() {
    let fake = Fake::old().with(|s| s.model = Model::Km002c);
    let r = fake.run();
    match &r {
        Err(FlashError::Product { detail }) => assert!(detail.contains("KM002C"), "{detail}"),
        other => panic!("expected a product error, got {other:?}"),
    }
    assert!(fake.ops().is_empty(), "nothing sent: {:?}", fake.ops());
}

#[test]
fn djynrp_old_boot_a_size_past_the_application_area_is_refused_before_any_frame() {
    // 0x4000 + 0x80000 ends past the 512 KiB area; the payload holds the
    // whole declared size so the container itself parses.
    let bytes = ContainerBuilder::new()
        .payload(&payload(0x0008_0000))
        .field(SIZE, &0x0008_0000u32.to_string())
        .build();
    let fake = Fake::old();
    let r = fake.run_with(opted_in(), &bytes);
    assert!(matches!(r, Err(FlashError::Product { .. })), "{r:?}");
    assert!(fake.ops().is_empty(), "{:?}", fake.ops());
}

#[test]
fn djynrp_old_boot_a_payload_past_the_size_budget_is_refused_before_any_frame() {
    let bytes = ContainerBuilder::new()
        .payload(&payload(P + B + 1))
        .field(SIZE, &P.to_string())
        .build();
    let fake = Fake::old();
    let r = fake.run_with(opted_in(), &bytes);
    assert!(matches!(r, Err(FlashError::Product { .. })), "{r:?}");
    assert!(fake.ops().is_empty(), "{:?}", fake.ops());
}

#[test]
fn djynrp_old_boot_descriptor_names_the_application_area_from_0x4000() {
    let fake = Fake::old();
    let r = fake.run();
    assert!(r.is_ok(), "{r:?}");
    let ops = fake.ops();
    let Some(Op::Raw { data, .. }) = ops.iter().find(|o| is_descriptor(o)) else {
        panic!("no descriptor: {ops:?}");
    };
    let word = |o: usize| u32::from_le_bytes([data[4 + o], data[5 + o], data[6 + o], data[7 + o]]);
    assert_eq!(word(0), 0x4000, "start");
    assert_eq!(word(8), P as u32, "size");
    assert_eq!(word(4), 0x4000 + P as u32, "end");
    let written: usize = ops
        .iter()
        .filter_map(|o| match o {
            Op::Raw { data, .. } if data.len() > 68 => Some(data.len()),
            _ => None,
        })
        .sum();
    assert!(written <= P + B, "payload within the budget: {written}");
}
