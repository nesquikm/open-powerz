//! Protocol triggering in the browser (FR W8SHXC): the desktop app's trigger
//! engine ([`powerz_trigger::Session`]) behind a binding. In the web edition
//! it runs in a dedicated worker whose port blocks on shared memory while the
//! page moves the bytes over Web Serial; the demo meter runs it in the page
//! over the simulator's PDM persona.
//!
//! The binding applies the same thin rules around the engine as the desktop
//! `TriggerControl`, through the one shared implementation in
//! `powerz_trigger` ([`powerz_trigger::guarded`], [`powerz_trigger::leave_view`],
//! [`powerz_trigger::console_refusal`], [`Confirm::of`]); after the close
//! every call refuses.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::session::Clock;
use powerz_sim::{SimCable, SimCharger, SimClock, SimPdm};
use powerz_trigger::{
    ArmedEnd, CableRead, Confirm, ConsoleLine, Detection, Engine, LogSink, PdMessageRequest,
    PdSendReply, ProtocolSection, RiskClass, Session, SharedLog, TriggerError, TriggerErrorKind,
    TriggerProtocol, TriggerReply, TriggerRequest, VbusReading, console_refusal, guarded,
    leave_view, set_log_sink,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::BindError;

/// Any port the engine can run on.
trait Port: Read + Write {}
impl<T: Read + Write> Port for T {}

/// A boxed port, so one engine type takes every port.
struct DynPort(Box<dyn Port>);

impl Read for DynPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for DynPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// A boxed clock, so one engine type takes every clock.
struct DynClock(Box<dyn Clock>);

impl Clock for DynClock {
    fn now_ms(&mut self) -> u64 {
        self.0.now_ms()
    }
    fn random_u64(&mut self) -> u64 {
        self.0.random_u64()
    }
    fn sleep(&mut self, duration: Duration) {
        self.0.sleep(duration);
    }
}

/// The latest VBUS reading the engine's watch sees.
type VbusSlot = Rc<Cell<Option<VbusReading>>>;

/// The trigger engine the desktop app runs, for the web page.
#[wasm_bindgen]
pub struct TriggerEngine {
    session: Option<Session<DynPort, DynClock>>,
    vbus: VbusSlot,
    /// The page's log callback, kept until the session above has gone (its
    /// last lines are logged as it drops). (AC-AHTZW1.4)
    _log: Option<LogCallback>,
}

impl std::fmt::Debug for TriggerEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriggerEngine")
            .field("open", &self.session.is_some())
            .finish()
    }
}

/// The engine's error as the binding's: the same kind names as the desktop.
fn bind(e: TriggerError) -> BindError {
    BindError {
        kind: kind_name(&e.kind).to_owned(),
        message: e.message,
        hint: e.hint,
    }
}

/// The desktop error kind's snake_case name.
fn kind_name(kind: &TriggerErrorKind) -> &'static str {
    match kind {
        TriggerErrorKind::DeviceBusy => "device_busy",
        TriggerErrorKind::Timeout => "timeout",
        TriggerErrorKind::Protocol => "protocol",
        TriggerErrorKind::Disconnected => "disconnected",
        TriggerErrorKind::Usage => "usage",
        TriggerErrorKind::OutOfSync => "out_of_sync",
        TriggerErrorKind::Internal => "internal",
    }
}

/// The handle of a session that has ended (the desktop wording).
fn closed() -> BindError {
    BindError::new("usage", "The trigger session is closed.")
}

impl TriggerEngine {
    /// Open the PDM session on `port` (handshake included).
    pub fn with_port(
        port: impl Read + Write + 'static,
        clock: impl Clock + 'static,
    ) -> Result<TriggerEngine, BindError> {
        TriggerEngine::open_on(DynPort(Box::new(port)), DynClock(Box::new(clock)), None)
    }

    /// The desktop demo device's simulated charger (a multi-protocol charger
    /// behind an EPR 240 W cable) on a simulated clock reading `now_ms`.
    pub fn demo(now_ms: u64) -> Result<TriggerEngine, BindError> {
        let clock = SimClock::new(now_ms);
        let sim =
            SimPdm::new(SimCharger::MultiProtocol, clock.clone()).with_cable(SimCable::Epr240W);
        TriggerEngine::with_port(sim.open_port(), clock)
    }

    fn open_on(
        port: DynPort,
        clock: DynClock,
        source: Option<Box<dyn FnMut() -> Option<VbusReading>>>,
    ) -> Result<TriggerEngine, BindError> {
        TriggerEngine::open_logged(port, clock, source, None)
    }

    /// [`TriggerEngine::open_on`], the session's log handed to `log` (the
    /// lines of the handshake included).
    fn open_logged(
        port: DynPort,
        clock: DynClock,
        source: Option<Box<dyn FnMut() -> Option<VbusReading>>>,
        log: Option<LogCallback>,
    ) -> Result<TriggerEngine, BindError> {
        let lines = SharedLog::default();
        if let Some(callback) = &log {
            set_log_sink(&lines, callback.sink());
        }
        let mut session = Session::open_logged(port, clock, lines).map_err(bind)?;
        let vbus: VbusSlot = Rc::new(Cell::new(None));
        let slot = Rc::clone(&vbus);
        let mut source = source;
        session.watch_vbus(move || {
            if let Some(read) = source.as_mut()
                && let Some(r) = read()
            {
                slot.set(Some(r));
            }
            slot.get()
        });
        Ok(TriggerEngine {
            session: Some(session),
            vbus,
            _log: log,
        })
    }

    /// Run `job` on the open session; a port failure ends "don't ask again".
    fn call<T>(
        &mut self,
        job: impl FnOnce(&mut Session<DynPort, DynClock>) -> Result<T, TriggerError>,
    ) -> Result<T, BindError> {
        let session = self.session.as_mut().ok_or_else(closed)?;
        guarded(session, job).map_err(bind)
    }

    /// BC 1.2, Apple and the automatic list.
    pub fn detect(&mut self) -> Result<Detection, BindError> {
        self.call(|s| s.detect())
    }

    /// Enter `protocol` and report its section.
    pub fn list(&mut self, protocol: TriggerProtocol) -> Result<ProtocolSection, BindError> {
        self.call(|s| s.list(protocol))
    }

    /// The library risk class of `request`.
    pub fn risk(&mut self, request: TriggerRequest) -> Result<RiskClass, BindError> {
        self.call(|s| s.risk(request))
    }

    /// Send `request`; `remember` with `confirmed` asks to arm "don't ask again".
    pub fn request(
        &mut self,
        request: TriggerRequest,
        confirmed: bool,
        remember: bool,
    ) -> Result<TriggerReply, BindError> {
        let confirm = Confirm::of(confirmed, remember);
        self.call(|s| s.request(request, confirm))
    }

    /// Whether "don't ask again" is armed.
    pub fn armed(&mut self) -> Result<bool, BindError> {
        self.call(|s| Ok(s.armed()))
    }

    /// End "don't ask again"; nothing is written.
    pub fn disarm(&mut self) -> Result<(), BindError> {
        self.call(|s| {
            s.disarm(ArmedEnd::Cleared);
            Ok(())
        })
    }

    /// Whether VBUS collapsed under an accepted contract.
    pub fn charger_gone(&mut self) -> Result<bool, BindError> {
        self.call(|s| Ok(s.charger_gone()))
    }

    /// Whether PD is the protocol entered last.
    pub fn pd_entered(&mut self) -> Result<bool, BindError> {
        self.call(|s| Ok(s.pd_entered()))
    }

    /// Back to 5 V; the session stays open.
    pub fn back_to_5v(&mut self) -> Result<(), BindError> {
        self.call(|s| s.back_to_5v())
    }

    /// The trigger view was left: "don't ask again" ends, then back to 5 V.
    pub fn leave_to_5v(&mut self) -> Result<(), BindError> {
        self.call(leave_view)
    }

    /// The library risk class of a built message.
    pub fn message_risk(&mut self, message: PdMessageRequest) -> Result<RiskClass, BindError> {
        self.call(|s| s.message_risk(message))
    }

    /// Send a built message; above 5 V it needs `confirmed`.
    pub fn send_message(
        &mut self,
        message: PdMessageRequest,
        confirmed: bool,
    ) -> Result<PdSendReply, BindError> {
        self.call(|s| s.send_message(message, confirmed))
    }

    /// One SOP' Discover Identity inside the open PD session.
    pub fn cable_read(&mut self) -> Result<CableRead, BindError> {
        self.call(|s| s.cable_read())
    }

    /// The library risk class of a raw console line.
    pub fn console_risk(&mut self, line: String) -> Result<RiskClass, BindError> {
        self.call(|s| s.console_risk(line))
    }

    /// Send a raw console line; GoodCRC is always refused.
    pub fn console_send(
        &mut self,
        line: String,
        confirmed: bool,
    ) -> Result<Vec<ConsoleLine>, BindError> {
        if self.session.is_none() {
            return Err(closed());
        }
        if let Some(refusal) = console_refusal(&line) {
            return Err(bind(refusal));
        }
        self.call(|s| s.console_send(line, confirmed))
    }

    /// The stream's latest VBUS reading; the watch looks at it at once.
    pub fn push_vbus(&mut self, reading: VbusReading) -> Result<(), BindError> {
        self.vbus.set(Some(reading));
        self.call(|s| {
            s.check_vbus();
            Ok(())
        })
    }

    /// Back to 5 V, then `pdm close`; every later call refuses.
    pub fn close(&mut self) -> Result<(), BindError> {
        let session = self.session.take().ok_or_else(closed)?;
        session.shutdown().map_err(bind)
    }
}

// ---- the JS side ----

#[wasm_bindgen]
extern "C" {
    /// The worker's blocking port: `read(timeoutMs)` gives the bytes that
    /// arrived (empty when none before the timeout), `write(bytes)` sends.
    #[wasm_bindgen(typescript_type = "TriggerPortJs")]
    pub type TriggerPortJs;
    #[wasm_bindgen(method, catch)]
    fn read(this: &TriggerPortJs, timeout_ms: f64) -> Result<Vec<u8>, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn write(this: &TriggerPortJs, bytes: &[u8]) -> Result<(), JsValue>;

    /// The worker's clock: wall-clock `nowMs()` and a blocking `sleep(ms)`.
    #[wasm_bindgen(typescript_type = "TriggerClockJs")]
    pub type TriggerClockJs;
    #[wasm_bindgen(method, js_name = nowMs)]
    fn now_ms(this: &TriggerClockJs) -> f64;
    #[wasm_bindgen(method)]
    fn sleep(this: &TriggerClockJs, ms: f64);

    /// The latest VBUS reading in shared memory: `get()` gives it, or `null`.
    #[wasm_bindgen(typescript_type = "TriggerVbusJs")]
    pub type TriggerVbusJs;
    #[wasm_bindgen(method)]
    fn get(this: &TriggerVbusJs) -> JsValue;

    /// The worker's log callback: called with each line of the session's log.
    #[wasm_bindgen(typescript_type = "TriggerLogJs")]
    pub type TriggerLogJs;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call_line(this: &TriggerLogJs, this_arg: &JsValue, line: &str) -> Result<(), JsValue>;
}

thread_local! {
    /// The log callbacks of the open engines, by id: the engine's log sink
    /// must be `Send`, a JS function is not, so the sink carries the id.
    static LOG_CALLBACKS: RefCell<HashMap<u64, TriggerLogJs>> = RefCell::new(HashMap::new());
    static NEXT_LOG_ID: Cell<u64> = const { Cell::new(0) };
}

/// One engine's log callback in [`LOG_CALLBACKS`], removed on drop.
struct LogCallback(u64);

impl LogCallback {
    fn new(js: TriggerLogJs) -> LogCallback {
        let id = NEXT_LOG_ID.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1));
            id
        });
        LOG_CALLBACKS.with(|m| m.borrow_mut().insert(id, js));
        LogCallback(id)
    }

    /// The session's log sink: each line to the callback; one that throws
    /// loses that line only.
    fn sink(&self) -> LogSink {
        let id = self.0;
        Box::new(move |line: String| {
            let js = LOG_CALLBACKS.with(|m| {
                m.borrow()
                    .get(&id)
                    .map(|f| JsValue::clone(f).unchecked_into::<TriggerLogJs>())
            });
            if let Some(js) = js {
                let _ = js.call_line(&JsValue::NULL, &line);
            }
        })
    }
}

impl Drop for LogCallback {
    fn drop(&mut self) {
        LOG_CALLBACKS.with(|m| m.borrow_mut().remove(&self.0));
    }
}

#[wasm_bindgen(typescript_custom_section)]
const TRIGGER_TS: &str = r#"
/** The worker's blocking port. */
export interface TriggerPortJs {
  read(timeoutMs: number): Uint8Array;
  write(bytes: Uint8Array): void;
}
/** The worker's clock. */
export interface TriggerClockJs {
  nowMs(): number;
  sleep(ms: number): void;
}
/** One VBUS reading of the stream. */
export interface TriggerVbusReading {
  volts: number;
  cc1_v: number;
  cc2_v: number;
  time_ms: number;
  period_ms: number;
}
/** The latest VBUS reading, or null. */
export interface TriggerVbusJs {
  get(): TriggerVbusReading | null;
}
/** Called with each line of the session's log. */
export type TriggerLogJs = (line: string) => void;
"#;

/// How long one port read waits for bytes, ms.
const READ_WAIT_MS: f64 = 10.0;

/// [`TriggerPortJs`] as a `Read + Write` port.
struct JsPort {
    js: TriggerPortJs,
    pending: Vec<u8>,
}

fn port_gone() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "the serial port went away")
}

impl Read for JsPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            self.pending = self.js.read(READ_WAIT_MS).map_err(|_| port_gone())?;
        }
        if self.pending.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no reply pending"));
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

impl Write for JsPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.js.write(buf).map_err(|_| port_gone())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// [`TriggerClockJs`] as the session clock.
struct JsClock {
    js: TriggerClockJs,
    rng: u64,
}

impl Clock for JsClock {
    fn now_ms(&mut self) -> u64 {
        let ms = self.js.now_ms();
        if ms.is_finite() && ms > 0.0 {
            ms as u64
        } else {
            0
        }
    }
    fn random_u64(&mut self) -> u64 {
        // splitmix64, seeded from the clock.
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn sleep(&mut self, duration: Duration) {
        self.js.sleep(duration.as_secs_f64() * 1000.0);
    }
}

/// A VBUS reading as JS gives it.
#[derive(Debug, Deserialize)]
struct VbusJs {
    volts: f64,
    cc1_v: f64,
    cc2_v: f64,
    time_ms: u32,
    period_ms: f64,
}

impl From<VbusJs> for VbusReading {
    fn from(r: VbusJs) -> VbusReading {
        // The page supplies it: never panic on NaN, a negative or a value
        // past `Duration::MAX`; anything unusable is "no period".
        let period = Duration::try_from_secs_f64(r.period_ms / 1000.0).unwrap_or(Duration::ZERO);
        VbusReading {
            volts: r.volts,
            cc1_v: r.cc1_v,
            cc2_v: r.cc2_v,
            time_ms: r.time_ms,
            period,
        }
    }
}

fn reading_of(value: JsValue) -> Option<VbusReading> {
    if value.is_null() || value.is_undefined() {
        return None;
    }
    serde_wasm_bindgen::from_value::<VbusJs>(value)
        .ok()
        .map(VbusReading::from)
}

fn arg<T: for<'de> Deserialize<'de>>(value: JsValue) -> Result<T, BindError> {
    serde_wasm_bindgen::from_value(value).map_err(|e| BindError::invalid(e.to_string()))
}

/// `value` for JS, every `None` and `()` as `null` (as Tauri answers).
fn out<T: Serialize>(value: Result<T, BindError>) -> Result<JsValue, BindError> {
    crate::decoders::to_js_view(&value?)
}

#[wasm_bindgen]
impl TriggerEngine {
    /// Open the PDM session on the worker's `port` and `clock` (handshake
    /// included); `vbus`, when given, is where the VBUS watch reads, and
    /// `log`, when given, takes each line of the session's log.
    #[wasm_bindgen(js_name = open)]
    pub fn open_js(
        port: TriggerPortJs,
        clock: TriggerClockJs,
        vbus: Option<TriggerVbusJs>,
        log: Option<TriggerLogJs>,
    ) -> Result<TriggerEngine, BindError> {
        let seed = clock.now_ms() as u64 ^ 0x5DEE_CE66_D1CE_4E5B;
        let source = vbus.map(|v| {
            Box::new(move || reading_of(v.get())) as Box<dyn FnMut() -> Option<VbusReading>>
        });
        TriggerEngine::open_logged(
            DynPort(Box::new(JsPort {
                js: port,
                pending: Vec::new(),
            })),
            DynClock(Box::new(JsClock {
                js: clock,
                rng: seed,
            })),
            source,
            log.map(LogCallback::new),
        )
    }

    /// [`TriggerEngine::demo`] for JS.
    #[wasm_bindgen(js_name = openDemo)]
    pub fn open_demo_js(now_ms: f64) -> Result<TriggerEngine, BindError> {
        let now = if now_ms.is_finite() && now_ms > 0.0 {
            now_ms as u64
        } else {
            0
        };
        TriggerEngine::demo(now)
    }

    #[wasm_bindgen(js_name = detect)]
    pub fn detect_js(&mut self) -> Result<JsValue, BindError> {
        out(self.detect())
    }

    #[wasm_bindgen(js_name = list)]
    pub fn list_js(&mut self, protocol: JsValue) -> Result<JsValue, BindError> {
        let protocol = arg(protocol)?;
        out(self.list(protocol))
    }

    #[wasm_bindgen(js_name = risk)]
    pub fn risk_js(&mut self, request: JsValue) -> Result<JsValue, BindError> {
        let request = arg(request)?;
        out(self.risk(request))
    }

    #[wasm_bindgen(js_name = request)]
    pub fn request_js(
        &mut self,
        request: JsValue,
        confirmed: bool,
        remember: Option<bool>,
    ) -> Result<JsValue, BindError> {
        let request = arg(request)?;
        out(self.request(request, confirmed, remember.unwrap_or(false)))
    }

    #[wasm_bindgen(js_name = armed)]
    pub fn armed_js(&mut self) -> Result<bool, BindError> {
        self.armed()
    }

    #[wasm_bindgen(js_name = disarm)]
    pub fn disarm_js(&mut self) -> Result<(), BindError> {
        self.disarm()
    }

    #[wasm_bindgen(js_name = chargerGone)]
    pub fn charger_gone_js(&mut self) -> Result<bool, BindError> {
        self.charger_gone()
    }

    #[wasm_bindgen(js_name = pdEntered)]
    pub fn pd_entered_js(&mut self) -> Result<bool, BindError> {
        self.pd_entered()
    }

    #[wasm_bindgen(js_name = backTo5v)]
    pub fn back_to_5v_js(&mut self) -> Result<(), BindError> {
        self.back_to_5v()
    }

    #[wasm_bindgen(js_name = leaveTo5v)]
    pub fn leave_to_5v_js(&mut self) -> Result<(), BindError> {
        self.leave_to_5v()
    }

    #[wasm_bindgen(js_name = messageRisk)]
    pub fn message_risk_js(&mut self, message: JsValue) -> Result<JsValue, BindError> {
        let message = arg(message)?;
        out(self.message_risk(message))
    }

    #[wasm_bindgen(js_name = sendMessage)]
    pub fn send_message_js(
        &mut self,
        message: JsValue,
        confirmed: bool,
    ) -> Result<JsValue, BindError> {
        let message = arg(message)?;
        out(self.send_message(message, confirmed))
    }

    #[wasm_bindgen(js_name = cableRead)]
    pub fn cable_read_js(&mut self) -> Result<JsValue, BindError> {
        out(self.cable_read())
    }

    #[wasm_bindgen(js_name = consoleRisk)]
    pub fn console_risk_js(&mut self, line: String) -> Result<JsValue, BindError> {
        out(self.console_risk(line))
    }

    #[wasm_bindgen(js_name = consoleSend)]
    pub fn console_send_js(&mut self, line: String, confirmed: bool) -> Result<JsValue, BindError> {
        out(self.console_send(line, confirmed))
    }

    /// Give the VBUS watch the stream's latest reading (`null`: none).
    #[wasm_bindgen(js_name = pushVbus)]
    pub fn push_vbus_js(&mut self, reading: JsValue) -> Result<(), BindError> {
        match reading_of(reading) {
            Some(r) => self.push_vbus(r),
            None => self.call(|s| {
                s.check_vbus();
                Ok(())
            }),
        }
    }

    #[wasm_bindgen(js_name = close)]
    pub fn close_js(&mut self) -> Result<(), BindError> {
        self.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn period_of(period_ms: f64) -> Duration {
        VbusReading::from(VbusJs {
            volts: 5.0,
            cc1_v: 1.6,
            cc2_v: 0.0,
            time_ms: 1,
            period_ms,
        })
        .period
    }

    #[test]
    fn a_page_supplied_period_never_panics() {
        for bad in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.0,
            -0.0,
            f64::MAX,
            1e300,
            1e30,
        ] {
            assert_eq!(period_of(bad), Duration::ZERO, "{bad}");
        }
        assert_eq!(period_of(0.0), Duration::ZERO);
        assert_eq!(period_of(100.0), Duration::from_millis(100));
        assert_eq!(period_of(0.5), Duration::from_micros(500));
    }
}
