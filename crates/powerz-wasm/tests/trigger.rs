//! FR W8SHXC: protocol triggering in the browser.
//!
//! The browser runs the desktop trigger engine compiled to WebAssembly. These
//! parity tests drive the binding's [`TriggerEngine`] and the shared native
//! engine (`powerz_trigger::Session`, the one the desktop app's trigger thread
//! runs) over two identical simulated PDM ports and compare, step by step,
//! every answer, every error (`kind`, `message`, `hint`) and every line written
//! to the port.
//!
//! - AC-W8SHXC.1: every trigger command (open, detect, list, risk, request,
//!   armed, disarm, charger gone, PD entered, back to 5 V, close, message risk
//!   and send, cable read, console risk and send) gives the same answers and
//!   errors for the same port traffic.
//! - AC-W8SHXC.3: above 5 V is refused unless confirmed or covered by "don't ask
//!   again"; only a PD request of at most 20 V can set it; it ends at 5 V, a
//!   disconnect, or any event that ends it on the desktop; EPR requests and
//!   console lines always need a confirmation.
//! - AC-W8SHXC.4 (engine side): closing returns the charger to 5 V and sends
//!   `pdm close`; leaving the view does the way back first.
//! - AC-W8SHXC.5: the VBUS watch gets the session's readings (`push_vbus`) as
//!   on the desktop: a collapse under a contract ends "don't ask again" and
//!   refuses requests until a PD listing.
//!
//! The Rust-facing binding API these tests pin down (the JS methods of the same
//! names wrap it, with serde values in and out):
//!
//! - `TriggerEngine::with_port(port: impl Read + Write + 'static, clock: impl
//!   Clock + 'static) -> Result<TriggerEngine, BindError>` (the PDM handshake
//!   runs here), and `TriggerEngine::demo(now_ms: u64)` over the desktop demo
//!   device's simulated charger (`SimCharger::MultiProtocol` with an
//!   `SimCable::Epr240W` cable).
//! - `detect`, `list(TriggerProtocol)`, `risk(TriggerRequest)`,
//!   `request(TriggerRequest, confirmed: bool, remember: bool)`, `armed`,
//!   `disarm`, `charger_gone`, `pd_entered`, `back_to_5v`, `leave_to_5v` (the
//!   desktop's leave-view: disarm as "left the trigger view", then back to 5 V
//!   without waiting for offers), `message_risk`, `send_message(msg,
//!   confirmed)`, `cable_read`, `console_risk(String)`, `console_send(String,
//!   confirmed)`, `push_vbus(VbusReading)` and `close`, each returning
//!   `Result<_, BindError>` whose `kind` is the desktop error kind's snake_case
//!   name (`usage`, `timeout`, `disconnected`, ...).
//!
//! The native side is driven exactly as the desktop `TriggerControl` drives the
//! engine, through the same shared rules (`powerz_trigger::guarded`,
//! `leave_view`, `console_refusal`, `Confirm::of`): `Confirm::YesRemember` only
//! for confirmed + remember, a port error (`disconnected` / `internal`)
//! disarms, GoodCRC console lines are refused before the engine, and leave-view
//! is `disarm(LeftView)` + `leave_to_5v`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use powerz_sim::{SimCable, SimCharger, SimClock, SimPdm, SimPdmPort};
use powerz_trigger::{
    ArmedEnd, Confirm, Engine, MessageSop, PdMessageRequest, RiskClass, Session, TriggerError,
    TriggerProtocol, TriggerRequest, VbusReading, console_refusal, guarded, leave_view,
};
use powerz_wasm::{BindError, TriggerEngine};
use serde::Serialize;
use serde_json::{Value, json};

const T0: u64 = 1_790_000_000_000;

// ---- requests and messages ----

fn pd(position: u8) -> TriggerRequest {
    TriggerRequest::Pd {
        position,
        voltage_mv: None,
        current_ma: None,
    }
}

fn pps(voltage_mv: u32) -> TriggerRequest {
    TriggerRequest::Pd {
        position: 5,
        voltage_mv: Some(voltage_mv),
        current_ma: Some(1000),
    }
}

/// Soft_Reset, a built control message at or below 5 V.
fn soft_reset() -> PdMessageRequest {
    PdMessageRequest {
        sop: MessageSop::Sop,
        msg_type: 13,
        data_objects: Vec::new(),
    }
}

/// A built Request for position 2 (9 V): above 5 V.
fn built_9v() -> PdMessageRequest {
    PdMessageRequest {
        sop: MessageSop::Sop,
        msg_type: 2,
        data_objects: vec![0x2304_B12C],
    }
}

fn reading(volts: f64, time_ms: u32) -> VbusReading {
    VbusReading {
        volts,
        cc1_v: 1.5,
        cc2_v: 0.0,
        time_ms,
        period: Duration::from_millis(20),
    }
}

// ---- the script both engines run ----

#[derive(Debug, Clone)]
enum Step {
    Detect,
    List(TriggerProtocol),
    Risk(TriggerRequest),
    Request(TriggerRequest, bool, bool),
    Armed,
    Disarm,
    ChargerGone,
    PdEntered,
    BackTo5v,
    Leave,
    MessageRisk(PdMessageRequest),
    SendMessage(PdMessageRequest, bool),
    CableRead,
    ConsoleRisk(&'static str),
    ConsoleSend(&'static str, bool),
    Vbus(VbusReading),
    /// The serial port goes away: another open of the tty makes the session's
    /// handle fail (as the desktop's "losing the serial port" test does).
    Unplug,
    Close,
}

fn ok<T: Serialize>(v: T) -> Value {
    json!({ "ok": serde_json::to_value(v).expect("serializable") })
}

fn native_err(e: &TriggerError) -> Value {
    json!({ "err": {
        "kind": serde_json::to_value(&e.kind).expect("kind serializes"),
        "message": e.message,
        "hint": e.hint,
    }})
}

fn bind_err(e: &BindError) -> Value {
    json!({ "err": { "kind": e.kind, "message": e.message, "hint": e.hint } })
}

fn native_out<T: Serialize>(r: Result<T, TriggerError>) -> Value {
    match r {
        Ok(v) => ok(v),
        Err(e) => native_err(&e),
    }
}

fn wasm_out<T: Serialize>(r: Result<T, BindError>) -> Value {
    match r {
        Ok(v) => ok(v),
        Err(e) => bind_err(&e),
    }
}

/// The two sims: identical chargers on identical clocks.
fn sim(charger: SimCharger, cable: Option<SimCable>) -> (SimPdm, SimClock) {
    let clock = SimClock::new(T0);
    let s = SimPdm::new(charger, clock.clone());
    let s = match cable {
        Some(c) => s.with_cable(c),
        None => s,
    };
    (s, clock)
}

/// The writes after the first `skip`, trimmed.
fn writes_after(sim: &SimPdm, skip: usize) -> Vec<String> {
    sim.writes()
        .into_iter()
        .skip(skip)
        .map(|w| w.trim().to_owned())
        .collect()
}

/// The native engine as the desktop trigger thread holds it.
struct Native {
    sim: SimPdm,
    session: Option<Session<SimPdmPort, SimClock>>,
    vbus: Arc<Mutex<Option<VbusReading>>>,
    _held: Vec<SimPdmPort>,
}

impl Native {
    fn open(charger: SimCharger, cable: Option<SimCable>) -> (Native, Value) {
        let (sim, clock) = sim(charger, cable);
        let before = sim.writes().len();
        let opened = Session::open(sim.open_port(), clock);
        let (session, out) = match opened {
            Ok(mut s) => {
                let vbus = Arc::new(Mutex::new(None));
                let cell = Arc::clone(&vbus);
                s.watch_vbus(move || *cell.lock().expect("vbus cell"));
                let n = Native {
                    sim,
                    session: Some(s),
                    vbus,
                    _held: Vec::new(),
                };
                let writes = writes_after(&n.sim, before);
                return (
                    n,
                    json!({ "step": "Open", "out": ok(()), "writes": writes }),
                );
            }
            Err(e) => (None, native_err(&e)),
        };
        let writes = writes_after(&sim, before);
        (
            Native {
                sim,
                session,
                vbus: Arc::new(Mutex::new(None)),
                _held: Vec::new(),
            },
            json!({ "step": "Open", "out": out, "writes": writes }),
        )
    }

    fn step(&mut self, step: &Step) -> Value {
        if let Step::Unplug = step {
            self._held.push(self.sim.open_port());
            return Value::Null;
        }
        let Some(s) = self.session.as_mut() else {
            return json!({ "err": "closed" });
        };
        // The desktop TriggerControl's rules, through the shared ones: a port
        // failure ends "don't ask again" (`guarded`), GoodCRC console lines are
        // refused before the engine, leave-view is `leave_view`.
        match step.clone() {
            Step::Detect => native_out(guarded(s, |s| s.detect())),
            Step::List(p) => native_out(guarded(s, |s| s.list(p))),
            Step::Risk(r) => native_out(guarded(s, |s| s.risk(r))),
            Step::Request(r, confirmed, remember) => {
                let confirm = Confirm::of(confirmed, remember);
                native_out(guarded(s, |s| s.request(r, confirm)))
            }
            Step::Armed => ok(s.armed()),
            Step::Disarm => {
                s.disarm(ArmedEnd::Cleared);
                ok(())
            }
            Step::ChargerGone => ok(s.charger_gone()),
            Step::PdEntered => ok(s.pd_entered()),
            Step::BackTo5v => native_out(guarded(s, |s| s.back_to_5v())),
            Step::Leave => native_out(guarded(s, leave_view)),
            Step::MessageRisk(m) => native_out(guarded(s, |s| s.message_risk(m))),
            Step::SendMessage(m, c) => native_out(guarded(s, |s| s.send_message(m, c))),
            Step::CableRead => native_out(guarded(s, |s| s.cable_read())),
            Step::ConsoleRisk(l) => native_out(guarded(s, |s| s.console_risk(l.to_owned()))),
            Step::ConsoleSend(l, c) => match console_refusal(l) {
                Some(refusal) => native_err(&refusal),
                None => native_out(guarded(s, |s| s.console_send(l.to_owned(), c))),
            },
            Step::Vbus(r) => {
                *self.vbus.lock().expect("vbus cell") = Some(r);
                s.check_vbus();
                ok(())
            }
            Step::Close => {
                let Some(s) = self.session.take() else {
                    unreachable!()
                };
                native_out(s.shutdown())
            }
            Step::Unplug => unreachable!(),
        }
    }
}

/// The binding's engine over the same kind of port.
struct Wasm {
    sim: SimPdm,
    engine: Option<TriggerEngine>,
    _held: Vec<SimPdmPort>,
}

impl Wasm {
    fn open(charger: SimCharger, cable: Option<SimCable>) -> (Wasm, Value) {
        let (sim, clock) = sim(charger, cable);
        let before = sim.writes().len();
        let (engine, out) = match TriggerEngine::with_port(sim.open_port(), clock) {
            Ok(e) => (Some(e), ok(())),
            Err(e) => (None, bind_err(&e)),
        };
        let writes = writes_after(&sim, before);
        (
            Wasm {
                sim,
                engine,
                _held: Vec::new(),
            },
            json!({ "step": "Open", "out": out, "writes": writes }),
        )
    }

    fn step(&mut self, step: &Step) -> Value {
        if let Step::Unplug = step {
            self._held.push(self.sim.open_port());
            return Value::Null;
        }
        let Some(e) = self.engine.as_mut() else {
            return json!({ "err": "closed" });
        };
        match step.clone() {
            Step::Detect => wasm_out(e.detect()),
            Step::List(p) => wasm_out(e.list(p)),
            Step::Risk(r) => wasm_out(e.risk(r)),
            Step::Request(r, c, rem) => wasm_out(e.request(r, c, rem)),
            Step::Armed => wasm_out(e.armed()),
            Step::Disarm => wasm_out(e.disarm()),
            Step::ChargerGone => wasm_out(e.charger_gone()),
            Step::PdEntered => wasm_out(e.pd_entered()),
            Step::BackTo5v => wasm_out(e.back_to_5v()),
            Step::Leave => wasm_out(e.leave_to_5v()),
            Step::MessageRisk(m) => wasm_out(e.message_risk(m)),
            Step::SendMessage(m, c) => wasm_out(e.send_message(m, c)),
            Step::CableRead => wasm_out(e.cable_read()),
            Step::ConsoleRisk(l) => wasm_out(e.console_risk(l.to_owned())),
            Step::ConsoleSend(l, c) => wasm_out(e.console_send(l.to_owned(), c)),
            Step::Vbus(r) => wasm_out(e.push_vbus(r)),
            Step::Close => wasm_out(e.close()),
            Step::Unplug => unreachable!(),
        }
    }
}

/// Run `script` on both engines; each transcript entry is the step, its answer
/// and the lines it wrote. Returns the binding's transcript.
fn parity(charger: SimCharger, cable: Option<SimCable>, script: &[Step]) -> Vec<Value> {
    let (mut n, n_open) = Native::open(charger, cable);
    let (mut w, w_open) = Wasm::open(charger, cable);
    assert_eq!(w_open, n_open, "open differs");
    let mut transcript = vec![w_open];
    for step in script {
        let (nb, wb) = (n.sim.writes().len(), w.sim.writes().len());
        let native = n.step(step);
        let wasm = w.step(step);
        let n_writes = writes_after(&n.sim, nb);
        let w_writes = writes_after(&w.sim, wb);
        let name = format!("{step:?}");
        assert_eq!(wasm, native, "answer of {name} differs");
        assert_eq!(w_writes, n_writes, "lines written by {name} differ");
        transcript.push(json!({ "step": name, "out": wasm, "writes": w_writes }));
    }
    transcript
}

fn out_of(t: &[Value], i: usize) -> &Value {
    &t[i]["out"]
}

fn writes_of(t: &[Value], i: usize) -> Vec<String> {
    t[i]["writes"]
        .as_array()
        .expect("writes")
        .iter()
        .map(|v| v.as_str().expect("line").to_owned())
        .collect()
}

fn err_kind(v: &Value) -> Option<&str> {
    v.pointer("/err/kind").and_then(Value::as_str)
}

fn err_message(v: &Value) -> Option<&str> {
    v.pointer("/err/message").and_then(Value::as_str)
}

const NEEDS_CONFIRMATION: &str = "A request above 5 V needs your confirmation.";

/// A usage refusal asking for the above-5 V confirmation (requests, built
/// messages and console lines each name what needs it).
fn asks(v: &Value) -> bool {
    err_kind(v) == Some("usage")
        && err_message(v).is_some_and(|m| m.ends_with("needs your confirmation."))
}

// ---- AC-W8SHXC.1: every command, the same answers and errors ----

#[test]
fn detect_and_every_listing_match_the_native_engine() {
    let mut script = vec![Step::Detect];
    script.extend(TriggerProtocol::ALL.iter().map(|&p| Step::List(p)));
    script.push(Step::PdEntered);
    script.push(Step::Close);
    let t = parity(SimCharger::MultiProtocol, Some(SimCable::Epr240W), &script);
    // Real answers, not two matching errors.
    assert!(out_of(&t, 1).pointer("/ok/entries").is_some(), "{t:?}");
    let pd = out_of(&t, 2);
    assert_eq!(pd.pointer("/ok/supported"), Some(&json!(true)), "{pd}");
    assert!(
        !pd.pointer("/ok/offers")
            .and_then(Value::as_array)
            .expect("offers")
            .is_empty(),
        "{pd}"
    );
    // The last listing was not PD.
    assert_eq!(out_of(&t, t.len() - 2), &ok(false));
}

#[test]
fn risk_and_requests_pd_and_legacy_match_the_native_engine() {
    let t = parity(
        SimCharger::MultiProtocol,
        None,
        &[
            Step::List(TriggerProtocol::Pd),
            Step::Risk(pd(1)),
            Step::Risk(pd(2)),
            Step::Request(pd(2), false, false),
            Step::Request(pd(2), true, false),
            Step::PdEntered,
            Step::List(TriggerProtocol::Qc),
            Step::Risk(TriggerRequest::Qc { volts: 9 }),
            Step::Request(TriggerRequest::Qc { volts: 9 }, false, false),
            Step::Request(TriggerRequest::Qc { volts: 9 }, true, false),
            Step::BackTo5v,
            Step::Close,
        ],
    );
    assert_eq!(out_of(&t, 2), &ok(RiskClass::AtMost5V));
    assert_eq!(out_of(&t, 3), &ok(RiskClass::Above5VRememberable));
    // Unconfirmed above 5 V: refused, nothing written.
    assert_eq!(err_kind(out_of(&t, 4)), Some("usage"));
    assert_eq!(err_message(out_of(&t, 4)), Some(NEEDS_CONFIRMATION));
    assert_eq!(writes_of(&t, 4), Vec::<String>::new());
    assert_eq!(
        out_of(&t, 5).pointer("/ok/command").and_then(Value::as_str),
        Some("pd req=2")
    );
    assert_eq!(out_of(&t, 6), &ok(true));
    assert_eq!(err_message(out_of(&t, 9)), Some(NEEDS_CONFIRMATION));
    assert!(out_of(&t, 10).get("ok").is_some(), "{t:?}");
    // The close returns to 5 V and sends pdm close (AC-W8SHXC.4).
    let close = writes_of(&t, 12);
    assert_eq!(
        close.last().map(String::as_str),
        Some("pdm close"),
        "{close:?}"
    );
}

#[test]
fn built_messages_cable_read_and_console_match_the_native_engine() {
    let t = parity(
        SimCharger::MultiProtocol,
        Some(SimCable::Epr240W),
        &[
            // Not in PD yet: the cable read is refused, nothing entered.
            Step::CableRead,
            Step::List(TriggerProtocol::Pd),
            Step::CableRead,
            Step::MessageRisk(soft_reset()),
            Step::SendMessage(soft_reset(), false),
            Step::MessageRisk(built_9v()),
            Step::SendMessage(built_9v(), false),
            Step::ConsoleRisk("pdm parm"),
            Step::ConsoleRisk("pd req=3"),
            Step::ConsoleSend("pd req=3", false),
            // GoodCRC is always refused.
            Step::ConsoleSend("pd cmd=1", true),
            Step::ConsoleSend("pd hrst", false),
            Step::Close,
        ],
    );
    assert_eq!(err_kind(out_of(&t, 1)), Some("usage"), "{t:?}");
    assert_eq!(writes_of(&t, 1), Vec::<String>::new());
    assert_eq!(
        out_of(&t, 3).pointer("/ok/kind").and_then(Value::as_str),
        Some("found"),
        "{t:?}"
    );
    assert_eq!(out_of(&t, 6), &ok(RiskClass::Above5V));
    assert!(asks(out_of(&t, 7)), "{t:?}");
    assert_eq!(writes_of(&t, 7), Vec::<String>::new());
    assert_eq!(out_of(&t, 9), &ok(RiskClass::Above5V));
    assert!(asks(out_of(&t, 10)), "{t:?}");
    assert_eq!(writes_of(&t, 10), Vec::<String>::new());
    assert_eq!(err_kind(out_of(&t, 11)), Some("usage"));
    assert_eq!(writes_of(&t, 11), Vec::<String>::new());
    assert!(writes_of(&t, 12).iter().any(|l| l == "pd hrst"), "{t:?}");
}

#[test]
fn an_open_on_a_port_that_fails_gives_the_native_error() {
    let (n_sim, n_clock) = sim(SimCharger::Epr140W, None);
    let n_port = n_sim.open_port();
    let _n_other = n_sim.open_port();
    let native = match Session::open(n_port, n_clock) {
        Ok(_) => panic!("the native open succeeded on a dead port"),
        Err(e) => native_err(&e),
    };
    let (w_sim, w_clock) = sim(SimCharger::Epr140W, None);
    let w_port = w_sim.open_port();
    let _w_other = w_sim.open_port();
    let wasm = match TriggerEngine::with_port(w_port, w_clock) {
        Ok(_) => panic!("the binding opened on a dead port"),
        Err(e) => bind_err(&e),
    };
    assert_eq!(wasm, native);
}

#[test]
fn a_lost_port_gives_the_native_error_and_disarms() {
    let t = parity(
        SimCharger::Epr140W,
        None,
        &[
            Step::List(TriggerProtocol::Pd),
            Step::Request(pd(2), true, true),
            Step::Armed,
            Step::Unplug,
            Step::Request(pd(3), false, false),
            Step::Armed,
        ],
    );
    assert_eq!(out_of(&t, 3), &ok(true));
    assert!(t[5]["out"].get("err").is_some(), "{t:?}");
    assert_eq!(out_of(&t, 6), &ok(false), "a lost port leaves it armed");
}

#[test]
fn a_closed_engine_refuses_every_call() {
    let (sim, clock) = sim(SimCharger::Epr140W, None);
    let mut e = TriggerEngine::with_port(sim.open_port(), clock).expect("open");
    e.close().expect("close");
    let err = e.detect().expect_err("closed");
    assert_eq!(err.kind, "usage");
    assert_eq!(err.message, "The trigger session is closed.");
    let before = sim.writes().len();
    e.request(pd(2), true, false).expect_err("closed");
    assert_eq!(writes_after(&sim, before), Vec::<String>::new());
}

#[test]
fn the_demo_engine_is_the_desktop_demo_charger() {
    let (n_sim, n_clock) = sim(SimCharger::MultiProtocol, Some(SimCable::Epr240W));
    let mut n = Session::open(n_sim.open_port(), n_clock).expect("native demo open");
    let mut w = TriggerEngine::demo(T0).expect("demo open");
    assert_eq!(
        wasm_out(w.list(TriggerProtocol::Pd)),
        native_out(n.list(TriggerProtocol::Pd))
    );
    assert_eq!(wasm_out(w.cable_read()), native_out(n.cable_read()));
    assert_eq!(
        wasm_out(w.list(TriggerProtocol::Qc)),
        native_out(n.list(TriggerProtocol::Qc))
    );
    let found = wasm_out(w.list(TriggerProtocol::Pd));
    assert_eq!(found.pointer("/ok/supported"), Some(&json!(true)));
    w.close().expect("close");
}

// ---- AC-W8SHXC.3: confirmation and "don't ask again" ----

#[test]
fn only_a_pd_request_of_at_most_20v_arms_and_epr_and_console_always_ask() {
    let t = parity(
        SimCharger::Epr140W,
        None,
        &[
            Step::List(TriggerProtocol::Pd),
            Step::Armed,
            // EPR: remembering is refused, nothing written.
            Step::Risk(pd(8)),
            Step::Request(pd(8), true, true),
            Step::Armed,
            // 9 V remembered: arms.
            Step::Request(pd(2), true, true),
            Step::Armed,
            Step::Risk(pd(2)),
            // Covered: no confirmation needed.
            Step::Request(pd(2), false, false),
            Step::Armed,
            // EPR and the console still ask.
            Step::Risk(pd(8)),
            Step::Request(pd(8), false, false),
            Step::ConsoleRisk("pd req=3"),
            Step::ConsoleSend("pd req=3", false),
            Step::MessageRisk(built_9v()),
            Step::SendMessage(built_9v(), false),
            Step::Armed,
            Step::Close,
        ],
    );
    assert_eq!(out_of(&t, 2), &ok(false));
    assert_eq!(out_of(&t, 3), &ok(RiskClass::Above5V));
    assert_eq!(err_kind(out_of(&t, 4)), Some("usage"));
    assert_eq!(writes_of(&t, 4), Vec::<String>::new());
    assert_eq!(out_of(&t, 5), &ok(false));
    assert_eq!(out_of(&t, 7), &ok(true));
    assert_eq!(out_of(&t, 8), &ok(RiskClass::Armed));
    assert!(out_of(&t, 9).get("ok").is_some(), "{t:?}");
    assert_eq!(out_of(&t, 10), &ok(true));
    assert_eq!(out_of(&t, 11), &ok(RiskClass::Above5V));
    for i in [12, 14, 16] {
        assert!(asks(out_of(&t, i)), "step {i}: {t:?}");
        assert_eq!(writes_of(&t, i), Vec::<String>::new(), "step {i}");
    }
    assert_eq!(out_of(&t, 17), &ok(true), "a refusal ended it");
}

#[test]
fn above_20v_pps_never_arms_and_asks_when_armed() {
    let t = parity(
        SimCharger::Pps65W,
        None,
        &[
            Step::List(TriggerProtocol::Pd),
            Step::Request(pd(2), true, true),
            Step::Armed,
            Step::Risk(pps(20_100)),
            Step::Request(pps(20_100), false, false),
            Step::Request(pps(21_000), true, true),
            Step::Close,
        ],
    );
    assert_eq!(out_of(&t, 3), &ok(true));
    assert_eq!(out_of(&t, 4), &ok(RiskClass::Above5V));
    assert_eq!(err_message(out_of(&t, 5)), Some(NEEDS_CONFIRMATION));
    assert_eq!(writes_of(&t, 5), Vec::<String>::new());
    assert_eq!(err_kind(out_of(&t, 6)), Some("usage"));
}

fn armed_then(end: Vec<Step>) -> Vec<Value> {
    let mut script = vec![
        Step::List(TriggerProtocol::Pd),
        Step::Request(pd(2), true, true),
        Step::Armed,
    ];
    script.extend(end);
    script.push(Step::Armed);
    script.push(Step::Risk(pd(2)));
    script.push(Step::Request(pd(2), false, false));
    parity(SimCharger::Epr140W, None, &script)
}

/// After an event that ends "don't ask again": not armed, 9 V asks again and an
/// unconfirmed 9 V is refused with nothing written.
fn ended(t: &[Value]) {
    assert_eq!(out_of(t, 3), &ok(true), "the arming request arms: {t:?}");
    let n = t.len();
    assert_eq!(out_of(t, n - 3), &ok(false), "still armed: {t:?}");
    assert_eq!(out_of(t, n - 2), &ok(RiskClass::Above5VRememberable));
    assert_eq!(err_message(out_of(t, n - 1)), Some(NEEDS_CONFIRMATION));
    assert_eq!(writes_of(t, n - 1), Vec::<String>::new());
}

#[test]
fn back_to_5v_ends_dont_ask_again() {
    ended(&armed_then(vec![Step::BackTo5v]));
}

#[test]
fn leaving_the_view_ends_dont_ask_again_and_goes_back_to_5v() {
    let t = armed_then(vec![Step::Leave]);
    ended(&t);
    assert!(writes_of(&t, 4).iter().any(|l| l == "pd req=1"), "{t:?}");
}

#[test]
fn clearing_detect_a_built_message_and_the_console_end_it() {
    ended(&armed_then(vec![Step::Disarm]));
    ended(&armed_then(vec![
        Step::Detect,
        Step::List(TriggerProtocol::Pd),
    ]));
    ended(&armed_then(vec![Step::SendMessage(soft_reset(), false)]));
    ended(&armed_then(vec![Step::ConsoleSend("pd hrst", false)]));
}

#[test]
fn a_new_engine_on_the_same_meter_starts_disarmed() {
    let (sim, clock) = sim(SimCharger::Epr140W, None);
    let mut e = TriggerEngine::with_port(sim.open_port(), clock.clone()).expect("open");
    e.list(TriggerProtocol::Pd).expect("list");
    e.request(pd(2), true, true).expect("9 V remembered");
    assert!(e.armed().expect("armed"));
    e.close().expect("close");
    let mut next = TriggerEngine::with_port(sim.open_port(), clock).expect("reopen");
    assert!(!next.armed().expect("armed"));
    next.list(TriggerProtocol::Pd).expect("list");
    let err = next.request(pd(2), false, false).expect_err("asks again");
    assert_eq!(err.message, NEEDS_CONFIRMATION);
    next.close().expect("close");
}

// ---- AC-W8SHXC.4: close and leave go back to 5 V, then pdm close ----

#[test]
fn closing_above_5v_goes_back_to_5v_then_pdm_close() {
    let t = parity(
        SimCharger::Epr140W,
        None,
        &[
            Step::List(TriggerProtocol::Pd),
            Step::Request(pd(2), true, false),
            Step::Close,
        ],
    );
    let close = writes_of(&t, 3);
    let back = close.iter().position(|l| l == "pd req=1");
    let pdm_close = close.iter().position(|l| l == "pdm close");
    assert!(back.is_some() && pdm_close.is_some(), "{close:?}");
    assert!(back < pdm_close, "{close:?}");
}

#[test]
fn a_leave_then_close_sends_one_way_back_then_pdm_close() {
    let t = parity(
        SimCharger::Epr140W,
        None,
        &[
            Step::List(TriggerProtocol::Pd),
            Step::Request(pd(2), true, false),
            Step::Leave,
            Step::Close,
        ],
    );
    let leave = writes_of(&t, 3);
    let close = writes_of(&t, 4);
    assert_eq!(
        leave.iter().filter(|l| *l == "pd req=1").count(),
        1,
        "{leave:?}"
    );
    assert_eq!(close, vec!["pdm close".to_owned()]);
}

// ---- AC-W8SHXC.5: the VBUS watch ----

fn collapse() -> Vec<Step> {
    let mut steps = vec![Step::Vbus(reading(9.0, 1_000))];
    steps.extend((1..=20).map(|i| Step::Vbus(reading(0.5, 1_000 + i * 20))));
    steps
}

#[test]
fn a_vbus_collapse_under_a_contract_ends_it_as_on_the_desktop() {
    let mut script = vec![
        Step::List(TriggerProtocol::Pd),
        Step::Request(pd(2), true, true),
        Step::Armed,
        Step::ChargerGone,
    ];
    script.extend(collapse());
    script.extend([
        Step::ChargerGone,
        Step::Armed,
        Step::Risk(pd(2)),
        Step::Request(pd(2), true, false),
        // A PD listing brings the offers back.
        Step::List(TriggerProtocol::Pd),
        Step::ChargerGone,
    ]);
    let t = parity(SimCharger::Epr140W, None, &script);
    let n = t.len();
    assert_eq!(out_of(&t, 3), &ok(true));
    assert_eq!(out_of(&t, 4), &ok(false));
    assert_eq!(
        out_of(&t, n - 6),
        &ok(true),
        "the collapse was not seen: {t:?}"
    );
    assert_eq!(out_of(&t, n - 5), &ok(false));
    for i in [n - 4, n - 3] {
        assert_eq!(
            err_message(out_of(&t, i)),
            Some("The charger went away. Press Detect.")
        );
    }
    assert_eq!(writes_of(&t, n - 3), Vec::<String>::new());
    assert_eq!(out_of(&t, n - 1), &ok(false));
}

#[test]
fn low_vbus_before_any_contract_means_nothing() {
    let mut script = vec![Step::List(TriggerProtocol::Pd)];
    script.extend((0..20).map(|i| Step::Vbus(reading(0.0, i * 20))));
    script.push(Step::ChargerGone);
    let t = parity(SimCharger::Epr140W, None, &script);
    assert_eq!(out_of(&t, t.len() - 1), &ok(false));
}

#[test]
fn one_low_reading_between_normal_ones_is_no_collapse() {
    let t = parity(
        SimCharger::Epr140W,
        None,
        &[
            Step::List(TriggerProtocol::Pd),
            Step::Request(pd(2), true, true),
            Step::Vbus(reading(9.0, 1_000)),
            Step::Vbus(reading(0.5, 1_020)),
            Step::Vbus(reading(9.0, 1_040)),
            Step::Vbus(reading(9.0, 1_400)),
            Step::ChargerGone,
            Step::Armed,
        ],
    );
    assert_eq!(out_of(&t, 7), &ok(false));
    assert_eq!(out_of(&t, 8), &ok(true));
}
