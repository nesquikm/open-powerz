//! Shared helpers for the desktop backend tests (FR VHY378).
//!
//! Every identifier is synthetic: the simulator's made-up serial `123456`
//! and its made-up UID.

#![allow(dead_code, unused_imports)]

pub mod slow_sink;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use open_powerz_desktop_lib::worker::{BackendEvent, EventSink, Status, Step, Worker};
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::transport::{self, Transport};
pub use powerz_sim::{FaultPlan, Persona, SIM_SERIAL, Scenario, SimClock, SimDevice};

/// Simulated wall-clock start.
pub const T0: u64 = 1_790_000_000_000;

/// Memory blocks the protocol allows reading (`docs/protocol.md`).
pub const ALLOWED_READS: [(u32, u32); 4] = [
    (0x0000_0420, 64),
    (0x0000_4420, 64),
    (0x0300_0C00, 64),
    (0x4001_0450, 12),
];

/// Frame types of the safe command set (`docs/protocol.md`): CONNECT,
/// DISCONNECT, GET_DATA, start / stop queue, HEAD (read-only old-firmware
/// check, FR V9SK52), memory read, auth.
pub const SAFE_TYPES: [u8; 8] = [0x02, 0x03, 0x0C, 0x0E, 0x0F, 0x40, 0x44, 0x4C];

pub fn clock() -> SimClock {
    SimClock::new(T0)
}

/// What the test pipe saw.
#[derive(Debug, Default)]
pub struct Log {
    /// Every request that reached the pipe, with the simulated time it was sent.
    pub frames: Vec<(u64, Vec<u8>)>,
    /// Queue records the meter delivered in successful queue-poll replies.
    pub records_delivered: usize,
    /// Simulated time of the last successful queue poll.
    pub last_good_poll_ms: Option<u64>,
    /// Requests in flight right now / at most.
    pub in_flight: usize,
    pub max_in_flight: usize,
    /// The pipe was dropped (closed).
    pub closed: bool,
}

impl Log {
    /// Requests of message type `ty`.
    pub fn of_type(&self, ty: u8) -> Vec<(u64, Vec<u8>)> {
        self.frames
            .iter()
            .filter(|(_, f)| f.first().map(|b| b & 0x7F) == Some(ty))
            .cloned()
            .collect()
    }
}

/// Fault knobs on top of the simulator.
#[derive(Debug, Default, Clone)]
pub struct PipeOpts {
    /// Answer 0x4C with REJECT, never forwarding it.
    pub reject_auth: bool,
    /// Answer 0x0E with REJECT, never forwarding it.
    pub reject_queue_start: bool,
    /// Patch every ADC snapshot reply so the temperature reads invalid (0x8000).
    pub invalid_temp: bool,
    /// Before forwarding queue poll number `n` (from 1), advance the clock by `ms`.
    pub stall_at_poll: Option<(usize, u64)>,
    /// Fail ADC snapshot number `n` (from 1) with a timeout.
    pub fail_snapshot: Option<usize>,
    /// From ADC snapshot number `n` (from 1) on, the temperature reads invalid.
    pub invalid_temp_from_snapshot: Option<usize>,
    /// A queue started by an earlier session still runs: 0x0E is REJECTed
    /// until a 0x0F has stopped it (FR X2SPDW).
    pub stale_stream: bool,
    /// Every PD_PACKET poll takes this many ms of the worker's time.
    pub pd_poll_cost_ms: Option<u64>,
}

/// The simulated meter seen through a recording, fault-injecting pipe. It
/// sits *under* the worker's guard, so it logs exactly what left the host.
pub struct Pipe {
    inner: SimDevice,
    clock: SimClock,
    opts: PipeOpts,
    polls: usize,
    snapshots: usize,
    /// The stale stream of [`PipeOpts::stale_stream`] has been stopped.
    stale_stopped: bool,
    pub log: Arc<Mutex<Log>>,
    /// What the meter's CDC pipe saw (see [`Transport::settings_pipe`]).
    pub cdc_log: Arc<Mutex<Log>>,
}

impl Pipe {
    pub fn new(inner: SimDevice, clock: &SimClock, opts: PipeOpts) -> Pipe {
        Pipe {
            inner,
            clock: clock.clone(),
            opts,
            polls: 0,
            snapshots: 0,
            stale_stopped: false,
            log: Arc::new(Mutex::new(Log::default())),
            cdc_log: Arc::new(Mutex::new(Log::default())),
        }
    }

    /// A bulk-persona simulator on `clock`.
    pub fn sim(scenario: Scenario, faults: FaultPlan, clock: &SimClock, opts: PipeOpts) -> Pipe {
        let dev = SimDevice::with_clock(Persona::Bulk, scenario, clock.clone()).with_faults(faults);
        Pipe::new(dev, clock, opts)
    }
}

fn is_queue_poll(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0)
}

fn is_pd_poll(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() & Attribute::PD_PACKET.bits() != 0)
}

fn is_snapshot(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() == Attribute::ADC.bits())
}

impl Transport for Pipe {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        {
            let mut log = self.log.lock().unwrap();
            log.frames.push((self.clock.now_ms(), request.to_vec()));
            log.in_flight += 1;
            log.max_in_flight = log.max_in_flight.max(log.in_flight);
        }
        let ty = request.first().map(|b| b & 0x7F);
        let result = if self.opts.reject_auth && ty == Some(0x4C) {
            Ok(vec![0x06, 0x00, 0x00, 0x00])
        } else if ty == Some(0x0E)
            && (self.opts.reject_queue_start || (self.opts.stale_stream && !self.stale_stopped))
        {
            Ok(vec![0x06, request[1], 0x00, 0x00])
        } else if is_snapshot(request) && {
            self.snapshots += 1;
            self.opts.fail_snapshot == Some(self.snapshots)
        } {
            Err(transport::Error::Timeout)
        } else {
            if let Some(ms) = self.opts.pd_poll_cost_ms
                && is_pd_poll(request)
            {
                self.clock.advance(Duration::from_millis(ms));
            }
            if is_queue_poll(request) {
                self.polls += 1;
                if let Some((n, ms)) = self.opts.stall_at_poll
                    && n == self.polls
                {
                    self.clock.advance(Duration::from_millis(ms));
                }
            }
            let mut r = self.inner.transact(request, timeout);
            if ty == Some(0x0F) && r.as_ref().is_ok_and(|reply| reply.first() == Some(&0x05)) {
                self.stale_stopped = true;
            }
            let invalid_now = self.opts.invalid_temp
                || self
                    .opts
                    .invalid_temp_from_snapshot
                    .is_some_and(|n| is_snapshot(request) && self.snapshots >= n);
            if invalid_now
                && is_snapshot(request)
                && let Ok(reply) = &mut r
                && reply.len() >= 8 + 0x1A
            {
                reply[8 + 0x18] = 0x00;
                reply[8 + 0x19] = 0x80;
            }
            r
        };
        let mut log = self.log.lock().unwrap();
        log.in_flight -= 1;
        if is_queue_poll(request)
            && let Ok(reply) = &result
        {
            log.last_good_poll_ms = Some(self.clock.now_ms());
            if reply.len() > 4
                && let Ok(sections) = frame::parse_put_data(reply)
            {
                log.records_delivered += sections
                    .iter()
                    .filter(|s| s.att == Attribute::ADC_QUEUE)
                    .map(|s| s.payload.len() / 20)
                    .sum::<usize>();
            }
        }
        result
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }

    /// The same simulated meter's CDC persona, logging to `cdc_log`.
    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, transport::Error>> {
        let mut twin = Pipe::new(self.inner.cdc_twin(), &self.clock, PipeOpts::default());
        twin.log = Arc::clone(&self.cdc_log);
        twin.cdc_log = Arc::clone(&self.cdc_log);
        Some(Ok(Box::new(twin)))
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        if let Ok(mut log) = self.log.lock() {
            log.closed = true;
        }
    }
}

/// Events captured with the simulated time they were emitted at.
pub type Events = Arc<Mutex<Vec<(u64, BackendEvent)>>>;

/// A sink that records every event with the simulated time.
pub fn capture(clock: &SimClock) -> (Events, impl EventSink) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let (e, c) = (Arc::clone(&events), clock.clone());
    let sink = move |ev: BackendEvent| e.lock().unwrap().push((c.now_ms(), ev));
    (events, sink)
}

/// Step `worker` until `ms` of simulated time have passed or it ends.
pub fn run_for<T: Transport>(worker: &mut Worker<T, SimClock>, clock: &SimClock, ms: u64) -> Step {
    let start = clock.now_ms();
    for _ in 0..1_000_000 {
        if clock.now_ms() - start >= ms {
            return Step::Continue;
        }
        if let Step::Ended = worker.step() {
            return Step::Ended;
        }
    }
    panic!("worker.step() never advanced the clock");
}

/// Every sample in batch events, in emission order.
pub fn samples(events: &Events) -> Vec<u32> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, e)| match e {
            BackendEvent::Batch(b) => Some(b.samples.iter().map(|s| s.t_ms).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Every status event, in order.
pub fn statuses(events: &Events) -> Vec<Status> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, e)| match e {
            BackendEvent::Status(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}
