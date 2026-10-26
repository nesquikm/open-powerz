//! The write-only mode switch of `powerz firmware jump` (FR V9SK56, AC .7):
//! [`flash::jump`] sends exactly one JUMP_DFU (from application mode) or
//! JUMP_APROM (from bootloader mode) as a raw write through the strict
//! flash guard, reads the meter's reply to it once and reports it, then
//! waits on clock time (long enough for a replug: on 2.0.6 the jump takes
//! effect at the next power-up `[device]`) for the meter to come back and
//! reports the mode it came back in. Nothing else is sent: no CONNECT, no
//! HEAD, no descriptor, no info read.
//!
//! No test here opens a real device. The fake meter ([`Bench`]) records
//! every transfer and switches mode on a jump write the way the meter
//! re-enumerates (`[static]` frame types 0x09 / 0x08, `docs/protocol.md`).

mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::FakeClock;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::flash::{
    self, FlashAck, FlashError, FlashTransport, JUMP_REPLY_WAIT, JumpOutcome, JumpReply, JumpSent,
    JumpTarget, Meter, MeterOpener, POLL_INTERVAL, RETURN_TIMEOUT, SWITCH_TIMEOUT,
};
use powerz_protocol::transport::discovery::Model;
use powerz_protocol::transport::{Error as TransportError, Transport};

// ---------------------------------------------------------------- helpers

/// JUMP_DFU and JUMP_APROM frame types, written out independently of the
/// library `[static]`.
const JUMP_DFU: u8 = 0x09;
const JUMP_APROM: u8 = 0x08;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// One transfer the fake saw.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Transact { req: Vec<u8> },
    Read { timeout: Duration },
    Raw { data: Vec<u8> },
}

#[derive(Debug)]
struct State {
    /// `None` while no meter is attached.
    mode: Option<DeviceMode>,
    ops: Vec<Op>,
    /// What successive `open` calls see after a jump the meter obeys (the
    /// last stays).
    after_jump: Vec<Option<DeviceMode>>,
    upcoming: VecDeque<Option<DeviceMode>>,
    /// The meter ignores jumps and stays where it is.
    ignore_jump: bool,
    /// The jump raw write reports this error. `Disconnected` is the meter
    /// resetting as it jumps: the switch still happens.
    jump_fail: Option<TransportError>,
    opens: usize,
    clock: FakeClock,
    open_cost_ms: u64,
    /// Clock time of the jump write.
    jump_at: Option<u64>,
    /// After a jump write, this many `open` calls fail with this error
    /// (an ambiguous device list, an I/O error) before the device list
    /// is read again; `usize::MAX`: every one.
    open_err_after_jump: Option<(TransportError, usize)>,
    /// The armed copy of `open_err_after_jump`.
    open_errs: Option<(TransportError, usize)>,
    /// What the first read after the jump write returns (default: no
    /// reply, a timeout).
    reply_to_jump: Option<Result<Vec<u8>, TransportError>>,
    /// The armed copy of `reply_to_jump`.
    reply_pending: Option<Result<Vec<u8>, TransportError>>,
}

#[derive(Debug, Clone)]
struct Pipe(Rc<RefCell<State>>);

impl Transport for Pipe {
    fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Transact {
            req: request.to_vec(),
        });
        match s.mode {
            None => Err(TransportError::Disconnected),
            // Answer anything with REJECT: a jump must not ask for replies.
            Some(_) => Ok(vec![0x06, request.get(1).copied().unwrap_or(0), 0, 0]),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Read { timeout });
        if let Some(reply) = s.reply_pending.take() {
            return reply;
        }
        match s.mode {
            None => Err(TransportError::Disconnected),
            Some(_) => Err(TransportError::Timeout),
        }
    }
}

impl FlashTransport for Pipe {
    fn write_raw(&mut self, data: &[u8], _timeout: Duration) -> Result<(), TransportError> {
        let mut s = self.0.borrow_mut();
        s.ops.push(Op::Raw {
            data: data.to_vec(),
        });
        let Some(mode) = s.mode else {
            return Err(TransportError::Disconnected);
        };
        let jump = (data.len() == 4).then(|| data[0] & 0x7F);
        let obeys = matches!(
            (mode, jump),
            (DeviceMode::Application, Some(JUMP_DFU)) | (DeviceMode::Bootloader, Some(JUMP_APROM))
        );
        if jump.is_some() {
            let now = s.clock.0.borrow().now_ms;
            s.jump_at = Some(now);
            s.open_errs = s.open_err_after_jump.clone();
            s.reply_pending = s.reply_to_jump.clone();
        }
        let fail = s.jump_fail.clone();
        let happens = matches!(fail, None | Some(TransportError::Disconnected));
        if obeys && happens && !s.ignore_jump {
            s.upcoming = s.after_jump.clone().into();
        }
        fail.map_or(Ok(()), Err)
    }
}

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
            s.mode = next;
        }
        Ok(s.mode.map(|mode| Meter {
            mode,
            model: Model::Km003c,
            pipe: Pipe(self.0.clone()),
        }))
    }
}

/// A fake meter plus its clock.
struct Bench {
    s: Rc<RefCell<State>>,
    clock: FakeClock,
    stop: AtomicBool,
    /// Each jump report and the clock time it came at.
    sent: RefCell<Vec<(JumpSent, u64)>>,
}

impl Bench {
    fn new(mode: Option<DeviceMode>) -> Bench {
        let clock = FakeClock::new(1_790_000_000_000);
        let state = State {
            mode,
            ops: Vec::new(),
            after_jump: Vec::new(),
            upcoming: VecDeque::new(),
            ignore_jump: false,
            jump_fail: None,
            opens: 0,
            clock: clock.clone(),
            open_cost_ms: 0,
            jump_at: None,
            open_err_after_jump: None,
            open_errs: None,
            reply_to_jump: None,
            reply_pending: None,
        };
        Bench {
            s: Rc::new(RefCell::new(state)),
            clock,
            stop: AtomicBool::new(false),
            sent: RefCell::new(Vec::new()),
        }
    }

    /// In application mode; after JUMP_DFU it vanishes once, then shows up
    /// in bootloader mode.
    fn app() -> Bench {
        let b = Bench::new(Some(DeviceMode::Application));
        b.s.borrow_mut().after_jump = vec![None, Some(DeviceMode::Bootloader)];
        b
    }

    /// In bootloader mode; after JUMP_APROM it vanishes once, then shows up
    /// in application mode.
    fn boot() -> Bench {
        let b = Bench::new(Some(DeviceMode::Bootloader));
        b.s.borrow_mut().after_jump = vec![None, Some(DeviceMode::Application)];
        b
    }

    fn ignore_jump(self) -> Bench {
        self.s.borrow_mut().ignore_jump = true;
        self
    }

    fn after_jump(self, seen: Vec<Option<DeviceMode>>) -> Bench {
        self.s.borrow_mut().after_jump = seen;
        self
    }

    fn jump_fail(self, e: TransportError) -> Bench {
        self.s.borrow_mut().jump_fail = Some(e);
        self
    }

    /// After the jump, `times` opens fail with `e` (`usize::MAX`: all).
    fn open_err_after_jump(self, e: TransportError, times: usize) -> Bench {
        self.s.borrow_mut().open_err_after_jump = Some((e, times));
        self
    }

    /// The first read after the jump write returns `reply`.
    fn reply(self, reply: Result<Vec<u8>, TransportError>) -> Bench {
        self.s.borrow_mut().reply_to_jump = Some(reply);
        self
    }

    fn open_cost(self, ms: u64) -> Bench {
        self.s.borrow_mut().open_cost_ms = ms;
        self
    }

    fn stopped(self) -> Bench {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self
    }

    fn run(&self, target: JumpTarget) -> Result<JumpOutcome, FlashError> {
        let mut opener = Opener(self.s.clone());
        let clock = self.clock.clone();
        flash::jump(
            FlashAck::acknowledge(),
            target,
            &mut opener,
            self.clock.clone(),
            &self.stop,
            &mut |sent| {
                let now = clock.0.borrow().now_ms;
                self.sent.borrow_mut().push((*sent, now));
            },
        )
    }

    fn sent(&self) -> Vec<JumpSent> {
        self.sent.borrow().iter().map(|(s, _)| *s).collect()
    }

    fn ops(&self) -> Vec<Op> {
        self.s.borrow().ops.clone()
    }

    fn mode(&self) -> Option<DeviceMode> {
        self.s.borrow().mode
    }

    fn now_ms(&self) -> u64 {
        self.clock.0.borrow().now_ms
    }

    fn jump_at(&self) -> u64 {
        self.s.borrow().jump_at.expect("a jump was sent")
    }
}

/// The only write is one raw 4-byte write of frame type `ty`, `ty id 00
/// 00`, followed by at most the one read of the meter's reply.
fn assert_only_the_jump(ops: &[Op], ty: u8) {
    assert!(
        matches!(ops.len(), 1 | 2),
        "the jump and its reply read: {ops:02x?}"
    );
    if let Some(op) = ops.get(1) {
        assert!(
            matches!(op, Op::Read { .. }),
            "then a read only: {ops:02x?}"
        );
    }
    match &ops[0] {
        Op::Raw { data } => {
            assert_eq!(data.len(), 4, "a 4-byte jump frame: {data:02x?}");
            assert_eq!(data[0], ty, "frame type: {data:02x?}");
            assert_eq!(&data[2..4], &[0x00, 0x00], "no body: {data:02x?}");
        }
        other => panic!("the jump is a raw write, not {other:02x?}"),
    }
}

// ---- AC-V9SK56.7: from application mode, JUMP_DFU only ----

#[test]
fn from_application_mode_it_sends_only_jump_dfu_and_reports_the_bootloader() {
    let bench = Bench::app();
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
    assert_eq!(bench.mode(), Some(DeviceMode::Bootloader));
}

#[test]
fn from_bootloader_mode_it_sends_only_jump_aprom_and_reports_the_application() {
    let bench = Bench::boot();
    let r = bench.run(JumpTarget::Application);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Application)));
    assert_only_the_jump(&bench.ops(), JUMP_APROM);
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
}

#[test]
fn a_jump_sends_no_connect_head_descriptor_or_info_read() {
    for (bench, target) in [
        (Bench::app(), JumpTarget::Bootloader),
        (Bench::boot(), JumpTarget::Application),
    ] {
        bench.run(target).expect("the jump succeeds");
        let ops = bench.ops();
        assert!(
            !ops.iter().any(|o| matches!(o, Op::Transact { .. })),
            "no framed request: {ops:02x?}"
        );
    }
}

// ---- AC-V9SK56.7: refusals that send nothing ----

#[test]
fn already_in_the_bootloader_a_jump_to_it_is_refused_and_sends_nothing() {
    let bench = Bench::boot();
    let e = bench
        .run(JumpTarget::Bootloader)
        .expect_err("already there");
    assert_eq!(
        e,
        FlashError::AlreadyInMode {
            mode: DeviceMode::Bootloader
        }
    );
    assert!(e.to_string().contains("bootloader"), "{e}");
    assert!(bench.ops().is_empty(), "nothing sent: {:02x?}", bench.ops());
}

#[test]
fn already_in_the_application_a_jump_to_it_is_refused_and_sends_nothing() {
    let bench = Bench::app();
    let e = bench
        .run(JumpTarget::Application)
        .expect_err("already there");
    assert_eq!(
        e,
        FlashError::AlreadyInMode {
            mode: DeviceMode::Application
        }
    );
    assert!(e.to_string().contains("application"), "{e}");
    assert!(bench.ops().is_empty(), "nothing sent: {:02x?}", bench.ops());
}

#[test]
fn an_unknown_mode_is_refused_for_either_target_and_sends_nothing() {
    for target in [JumpTarget::Bootloader, JumpTarget::Application] {
        let bench = Bench::new(Some(DeviceMode::Unknown));
        let e = bench.run(target).expect_err("unknown mode");
        assert_eq!(e, FlashError::UnknownMode, "target {target:?}");
        assert!(e.to_string().contains("unknown"), "{e}");
        assert!(bench.ops().is_empty(), "nothing sent: {:02x?}", bench.ops());
    }
}

#[test]
fn no_meter_at_all_is_a_device_error_and_sends_nothing() {
    let bench = Bench::new(None);
    let e = bench.run(JumpTarget::Bootloader).expect_err("no meter");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert!(bench.ops().is_empty());
}

#[test]
fn a_stop_requested_before_the_start_sends_nothing() {
    let bench = Bench::app().stopped();
    let e = bench
        .run(JumpTarget::Bootloader)
        .expect_err("Ctrl-C before the jump");
    assert!(matches!(e, FlashError::Interrupted { .. }), "got {e:?}");
    assert!(bench.ops().is_empty(), "nothing sent: {:02x?}", bench.ops());
    assert_eq!(bench.mode(), Some(DeviceMode::Application));
}

// ---- AC-V9SK56.7: an ignored jump and the clock-time waits ----

#[test]
fn an_ignored_jump_dfu_reports_the_application_mode_after_the_switch_wait() {
    let bench = Bench::app().ignore_jump();
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(
        r,
        Ok(JumpOutcome::NotReached {
            seen: Some(DeviceMode::Application)
        })
    );
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
    let waited = ms(bench.now_ms() - bench.jump_at());
    assert!(waited >= SWITCH_TIMEOUT, "gave up early after {waited:?}");
    assert!(
        waited <= SWITCH_TIMEOUT + POLL_INTERVAL * 2,
        "waited {waited:?}"
    );
}

#[test]
fn an_ignored_jump_aprom_reports_the_bootloader_after_the_return_wait() {
    let bench = Bench::boot().ignore_jump();
    let r = bench.run(JumpTarget::Application);
    assert_eq!(
        r,
        Ok(JumpOutcome::NotReached {
            seen: Some(DeviceMode::Bootloader)
        })
    );
    assert_only_the_jump(&bench.ops(), JUMP_APROM);
    let waited = ms(bench.now_ms() - bench.jump_at());
    assert!(waited >= RETURN_TIMEOUT, "gave up early after {waited:?}");
    assert!(
        waited <= RETURN_TIMEOUT + POLL_INTERVAL * 2,
        "waited {waited:?}"
    );
}

#[test]
fn a_meter_that_never_comes_back_is_not_reached_with_no_mode() {
    let bench = Bench::app().after_jump(vec![None]);
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::NotReached { seen: None }));
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
}

#[test]
fn the_wait_ends_on_clock_time_even_when_each_open_is_slow() {
    let bench = Bench::app().ignore_jump().open_cost(400);
    let r = bench.run(JumpTarget::Bootloader);
    assert!(matches!(r, Ok(JumpOutcome::NotReached { .. })), "got {r:?}");
    let waited = ms(bench.now_ms() - bench.jump_at());
    assert!(waited >= SWITCH_TIMEOUT, "gave up early after {waited:?}");
    let slack = (POLL_INTERVAL + ms(400)) * 2;
    assert!(waited <= SWITCH_TIMEOUT + slack, "waited {waited:?}");
    let slept = bench.clock.0.borrow().slept.clone();
    assert!(slept.iter().all(|d| *d <= POLL_INTERVAL), "{slept:?}");
}

#[test]
fn a_slow_re_enumeration_inside_the_wait_still_reaches_the_target() {
    let bench = Bench::app()
        .after_jump(vec![None, None, None, Some(DeviceMode::Bootloader)])
        .open_cost(400);
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
}

// ---- AC-V9SK56.7: the jump write may see the meter vanish ----

#[test]
fn a_jump_dfu_write_that_reports_disconnected_still_waits_and_reaches_the_bootloader() {
    let bench = Bench::app().jump_fail(TransportError::Disconnected);
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
}

#[test]
fn a_jump_aprom_write_that_reports_disconnected_still_waits_and_reaches_the_application() {
    let bench = Bench::boot().jump_fail(TransportError::Disconnected);
    let r = bench.run(JumpTarget::Application);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Application)));
    assert_only_the_jump(&bench.ops(), JUMP_APROM);
}

#[test]
fn a_jump_write_failing_with_another_error_is_a_device_error_naming_the_jump() {
    let bench = Bench::app().jump_fail(TransportError::Io("pipe error".into()));
    let e = bench
        .run(JumpTarget::Bootloader)
        .expect_err("an I/O error on the jump");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert!(e.to_string().contains("JUMP_DFU"), "{e}");
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
    assert_eq!(bench.mode(), Some(DeviceMode::Application));

    let bench = Bench::boot().jump_fail(TransportError::Io("pipe error".into()));
    let e = bench
        .run(JumpTarget::Application)
        .expect_err("an I/O error on the jump");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert!(e.to_string().contains("JUMP_APROM"), "{e}");
    assert_eq!(bench.mode(), Some(DeviceMode::Bootloader));
}

// ---- AC-V9SK56.7: opener errors during the wait are not swallowed ----

/// An opener error that is not "no device yet": the device list is
/// ambiguous after the re-enumeration.
fn ambiguous() -> TransportError {
    TransportError::Io("two meters match; pass --device SERIAL".into())
}

#[test]
fn an_opener_error_through_the_whole_switch_wait_is_reported_not_called_no_meter() {
    let bench = Bench::app().open_err_after_jump(ambiguous(), usize::MAX);
    let r = bench.run(JumpTarget::Bootloader);
    assert_ne!(
        r,
        Ok(JumpOutcome::NotReached { seen: None }),
        "an opener error is not \"no meter seen\""
    );
    let e = r.expect_err("the last opener error ends the jump");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert!(
        e.to_string().contains("two meters match"),
        "carries the opener's reason: {e}"
    );
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
    let waited = ms(bench.now_ms() - bench.jump_at());
    assert!(waited >= SWITCH_TIMEOUT, "gave up early after {waited:?}");
}

#[test]
fn an_opener_error_through_the_whole_return_wait_is_reported_not_called_no_meter() {
    let bench = Bench::boot().open_err_after_jump(ambiguous(), usize::MAX);
    let r = bench.run(JumpTarget::Application);
    let e = r.expect_err("the last opener error ends the jump");
    assert!(matches!(e, FlashError::Device { .. }), "got {e:?}");
    assert!(
        e.to_string().contains("two meters match"),
        "carries the opener's reason: {e}"
    );
    assert_only_the_jump(&bench.ops(), JUMP_APROM);
}

#[test]
fn opener_errors_that_clear_inside_the_wait_still_reach_the_target() {
    let bench = Bench::app().open_err_after_jump(ambiguous(), 3);
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
}

#[test]
fn an_opener_error_then_a_meter_in_the_wrong_mode_is_not_reached_with_that_mode() {
    // One opener error right after the jump, then the meter opens fine but
    // stays in application mode: the later look wins over the old error.
    let bench = Bench::app()
        .ignore_jump()
        .open_err_after_jump(ambiguous(), 1);
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(
        r,
        Ok(JumpOutcome::NotReached {
            seen: Some(DeviceMode::Application)
        }),
        "a later successful open forgets the earlier opener error"
    );
    assert_only_the_jump(&bench.ops(), JUMP_DFU);

    let bench = Bench::boot()
        .ignore_jump()
        .open_err_after_jump(ambiguous(), 1);
    let r = bench.run(JumpTarget::Application);
    assert_eq!(
        r,
        Ok(JumpOutcome::NotReached {
            seen: Some(DeviceMode::Bootloader)
        }),
        "a later successful open forgets the earlier opener error"
    );
    assert_only_the_jump(&bench.ops(), JUMP_APROM);
}

#[test]
fn only_no_device_yet_through_the_wait_stays_not_reached_with_no_mode() {
    // `Ok(None)` on every open: today's "no meter seen" answer.
    let bench = Bench::app().after_jump(vec![None]);
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::NotReached { seen: None }));
}

#[test]
fn jump_targets_name_their_modes() {
    assert_eq!(JumpTarget::Bootloader.mode(), DeviceMode::Bootloader);
    assert_eq!(JumpTarget::Application.mode(), DeviceMode::Application);
}

// ---- AC-V9SK56.7: the meter's reply to the jump is reported ----

#[test]
fn the_reply_to_the_jump_is_read_once_and_reported_before_the_wait() {
    let bench = Bench::app().reply(Ok(vec![0x06, 0x01, 0x00, 0x00]));
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_eq!(
        bench.sent(),
        vec![JumpSent {
            from: DeviceMode::Application,
            target: JumpTarget::Bootloader,
            reply: JumpReply::Reject,
        }]
    );
    let at = bench.sent.borrow()[0].1;
    assert_eq!(at, bench.jump_at(), "reported before the wait starts");
    let ops = bench.ops();
    assert_only_the_jump(&ops, JUMP_DFU);
    assert_eq!(
        ops[1],
        Op::Read {
            timeout: JUMP_REPLY_WAIT
        },
        "one short reply read"
    );
    assert!(JUMP_REPLY_WAIT <= ms(1000), "{JUMP_REPLY_WAIT:?}");
}

#[test]
fn no_reply_accept_and_other_types_are_each_reported() {
    for (reply, want) in [
        (Err(TransportError::Timeout), JumpReply::NoReply),
        (Ok(vec![0x05, 0x01, 0x00, 0x00]), JumpReply::Accept),
        (Ok(vec![0x0B, 0x01, 0x00, 0x00]), JumpReply::Other(0x0B)),
        (Err(TransportError::Disconnected), JumpReply::Disconnected),
    ] {
        let bench = Bench::boot().reply(reply.clone());
        bench.run(JumpTarget::Application).expect("the jump runs");
        assert_eq!(
            bench.sent(),
            vec![JumpSent {
                from: DeviceMode::Bootloader,
                target: JumpTarget::Application,
                reply: want,
            }],
            "reply {reply:02x?}"
        );
    }
    let bench = Bench::app();
    bench.run(JumpTarget::Bootloader).expect("the jump runs");
    assert_eq!(
        bench.sent()[0].reply,
        JumpReply::NoReply,
        "default: silence"
    );
}

#[test]
fn a_jump_write_that_sees_the_meter_vanish_reports_it_and_reads_nothing() {
    let bench = Bench::app().jump_fail(TransportError::Disconnected);
    bench.run(JumpTarget::Bootloader).expect("the jump runs");
    assert_eq!(bench.sent()[0].reply, JumpReply::Disconnected);
    assert_eq!(bench.ops().len(), 1, "no read: {:02x?}", bench.ops());
}

#[test]
fn replies_name_themselves_for_the_cli() {
    assert_eq!(JumpReply::Accept.to_string(), "ACCEPT");
    assert_eq!(JumpReply::Reject.to_string(), "REJECT");
    assert_eq!(JumpReply::Other(0x0B).to_string(), "type 0x0b");
    assert_eq!(JumpReply::NoReply.to_string(), "none");
    assert!(
        JumpReply::Disconnected.to_string().contains("disconnected"),
        "{}",
        JumpReply::Disconnected
    );
}

#[test]
fn a_refused_or_stopped_jump_reports_nothing() {
    let bench = Bench::boot();
    let _ = bench.run(JumpTarget::Bootloader);
    let stopped = Bench::app().stopped();
    let _ = stopped.run(JumpTarget::Bootloader);
    let failed = Bench::app().jump_fail(TransportError::Io("pipe error".into()));
    let _ = failed.run(JumpTarget::Bootloader);
    assert!(bench.sent().is_empty() && stopped.sent().is_empty());
    assert!(
        failed.sent().is_empty(),
        "a failed write is not a sent jump"
    );
}

// ---- AC-V9SK56.7 / AC-V9SK54.1: the jump takes effect at the next power-up ----

#[test]
fn the_waits_leave_two_minutes_for_a_replug() {
    assert!(SWITCH_TIMEOUT >= ms(120_000), "{SWITCH_TIMEOUT:?}");
    assert!(RETURN_TIMEOUT >= ms(120_000), "{RETURN_TIMEOUT:?}");
}

/// The meter stays in `stay` for `secs` seconds of polling after the jump
/// (2.0.6 does not switch live), then the user unplugs it and plugs it back
/// in and it comes up in `then`.
fn replugged_after(stay: DeviceMode, secs: u64, then: DeviceMode) -> Vec<Option<DeviceMode>> {
    let polls = (secs * 1000 / POLL_INTERVAL.as_millis() as u64) as usize;
    let mut v = vec![Some(stay); polls];
    v.extend([None, None, Some(then)]);
    v
}

#[test]
fn a_jump_dfu_that_waits_for_a_replug_reaches_the_bootloader() {
    let bench = Bench::app().after_jump(replugged_after(
        DeviceMode::Application,
        90,
        DeviceMode::Bootloader,
    ));
    let r = bench.run(JumpTarget::Bootloader);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Bootloader)));
    assert_only_the_jump(&bench.ops(), JUMP_DFU);
    assert_eq!(bench.sent().len(), 1);
}

#[test]
fn a_jump_aprom_that_waits_for_a_replug_reaches_the_application() {
    let bench = Bench::boot().after_jump(replugged_after(
        DeviceMode::Bootloader,
        90,
        DeviceMode::Application,
    ));
    let r = bench.run(JumpTarget::Application);
    assert_eq!(r, Ok(JumpOutcome::Reached(DeviceMode::Application)));
    assert_only_the_jump(&bench.ops(), JUMP_APROM);
}
