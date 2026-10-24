//! Running a parsed command line against real devices: discovery, device
//! selection, opening the chosen pipe, Ctrl-C and exit codes.

use std::io::{IsTerminal, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use powerz_protocol::decode::AdcSnapshot;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::frame;
use powerz_protocol::pdm::detect::{Hint, hint as dpdm_hint};
use powerz_protocol::pdm::legacy::{LegacyProtocol, Request as LegacyRequest};
use powerz_protocol::pdm::{self, FixedProtocol, PdmSession};
use powerz_protocol::session::{Clock, Session, SystemClock};
use powerz_protocol::transport::bulk::BulkTransport;
use powerz_protocol::transport::cdc::CdcTransport;
use powerz_protocol::transport::discovery::{DeviceEntry, Model, PID_KM003C};
use powerz_protocol::transport::hid::HidTransport;
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{
    FaultPlan, PdScenario, Persona, SIM_SERIAL, SimCable, SimCharger, SimClock, SimDevice, SimPdm,
};

use crate::args::{
    Cli, Command, FixedAction, LimitsAction, PdAction, PdDecode, Qc3Action, QcAction, RequestFlags,
    TransportKind, TriggerProtocol, UfcsAction, needs_connect, rate_from_sps, validate,
};
use crate::commands::{
    DetectOptions, LegacyRequestOptions, PdRequestOptions, PdSendOptions, PdSettingsChange,
    StreamOptions, WatchOptions, console, detect, info_in_mode, list, pd, pd_decode, pd_save,
    pd_send, read, select_device, stream, trigger_legacy_list, trigger_legacy_request,
    trigger_pd_list, trigger_pd_request, trigger_pd_reset, trigger_pd_settings, watch,
};
use crate::error::CliError;
use crate::trace::{TracePort, TraceTransport};

/// Run the parsed command line against real devices; returns the exit code.
pub fn run(cli: Cli) -> i32 {
    match try_run(&cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("powerz: {e}");
            e.exit_code()
        }
    }
}

/// Start of the simulated clock (`POWERZ_SIM_CLOCK=fake`):
/// 2026-01-01T00:00:00Z, so fake-clock output is deterministic.
const FAKE_CLOCK_EPOCH_MS: u64 = 1_767_225_600_000;

/// The demo device as a `list` entry (bus `demo`, address 0).
fn demo_entry() -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: PID_KM003C,
        serial: Some(SIM_SERIAL.to_string()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: "demo".to_string(),
        address: 0,
    }
}

/// Validate, discover, select the meter, open the chosen pipe and dispatch.
fn try_run(cli: &Cli) -> Result<(), CliError> {
    if let Some(d) = &cli.pd_file.decode {
        return decode_file(d);
    }
    validate(cli)?;
    if let Command::List { json, include_demo } = cli.command {
        let mut devices = if include_demo {
            // The demo device is listed even where USB discovery fails.
            transport::discovery::list_devices().unwrap_or_default()
        } else {
            transport::discovery::list_devices()?
        };
        if include_demo {
            devices.push(demo_entry());
        }
        return list(
            &devices,
            json,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr().lock(),
        );
    }
    if cli.pd_file.send.is_some()
        || matches!(
            cli.command,
            Command::Trigger { .. } | Command::Detect { .. } | Command::Console { .. }
        )
    {
        return run_pdm(cli);
    }
    if cli.transport == TransportKind::Sim {
        return run_sim(cli);
    }
    let devices = transport::discovery::list_devices()?;
    let device = select_device(&devices, cli.device.as_deref())?;
    let mode = device.mode();
    match cli.transport {
        TransportKind::Sim => unreachable!("the simulator is handled above"),
        TransportKind::Bulk => with_transport(cli, mode, BulkTransport::open(device)?),
        TransportKind::Cdc => with_transport(cli, mode, open_cdc(device)?),
        TransportKind::Hid => {
            with_transport(cli, mode, HidTransport::open(device.serial.as_deref())?)
        }
    }
}

/// The meter's CDC pipe, found by its USB serial number.
fn open_cdc(device: &DeviceEntry) -> Result<impl Transport, CliError> {
    let serial = device.serial.as_deref().ok_or_else(|| {
        CliError::NoDevice("the meter reports no serial number; cannot find its CDC port".into())
    })?;
    Ok(CdcTransport::open(serial)?)
}

/// `pd decode FILE`: decode a saved pdcap file; no device is opened.
fn decode_file(d: &PdDecode) -> Result<(), CliError> {
    let name = d.file.display().to_string();
    let file = std::fs::File::open(&d.file)
        .map_err(|e| CliError::Usage(format!("cannot open {name}: {e}")))?;
    pd_decode(
        std::io::BufReader::new(file),
        &name,
        d.json,
        &mut std::io::stdout().lock(),
    )
}

/// `trigger` / `detect`: open the meter's serial port (or, with
/// `--transport sim`, the simulated meter with the charger
/// `POWERZ_SIM_CHARGER` names) and run.
fn run_pdm(cli: &Cli) -> Result<(), CliError> {
    // Before any command reaches the meter: a Ctrl-C must never kill the
    // process between a high-voltage request and the return to 5 V and
    // `pdm close`; it only sets the stop flag. `detect` stays at 5 V but
    // must still always end with `pdm close`.
    ctrl_c_flag()?;
    if cli.transport != TransportKind::Sim {
        let hint = detect_hint(cli)?;
        let port = pdm::open_port(cli.device.as_deref())?;
        return pdm_on(cli, port, SystemClock, hint);
    }
    select_demo(cli)?;
    let hint = detect_hint(cli)?;
    let charger = sim_charger()?;
    if fake_sim_clock() {
        let clock = SimClock::new(FAKE_CLOCK_EPOCH_MS);
        let sim = with_sim_knobs(SimPdm::new(charger, clock.clone()))?;
        pdm_on(cli, sim.open_port(), clock, hint)
    } else {
        let sim = with_sim_knobs(SimPdm::new(charger, SimClock::new(SystemClock.now_ms())))?;
        pdm_on(cli, sim.open_port(), SystemClock, hint)
    }
}

/// `detect` only: the D+ / D- hint from one ADC snapshot over the binary
/// protocol (`--transport`; the simulated meter's `--scenario` with
/// `--transport sim`). `None` for `trigger`.
fn detect_hint(cli: &Cli) -> Result<Option<Hint>, CliError> {
    if !matches!(cli.command, Command::Detect { .. }) {
        return Ok(None);
    }
    let a = match cli.transport {
        TransportKind::Sim => {
            if fake_sim_clock() {
                let clock = SimClock::new(FAKE_CLOCK_EPOCH_MS);
                let sim = SimDevice::with_clock(Persona::Bulk, cli.scenario, clock.clone());
                adc_snapshot(cli, sim, clock)?
            } else {
                adc_snapshot(
                    cli,
                    SimDevice::new(Persona::Bulk, cli.scenario),
                    SystemClock,
                )?
            }
        }
        kind => {
            let devices = transport::discovery::list_devices()?;
            let device = select_device(&devices, cli.device.as_deref())?;
            match kind {
                TransportKind::Cdc => adc_snapshot(cli, open_cdc(device)?, SystemClock)?,
                TransportKind::Hid => adc_snapshot(
                    cli,
                    HidTransport::open(device.serial.as_deref())?,
                    SystemClock,
                )?,
                _ => adc_snapshot(cli, BulkTransport::open(device)?, SystemClock)?,
            }
        }
    };
    Ok(Some(dpdm_hint(
        volts_to_mv(a.dp_v()),
        volts_to_mv(a.dm_v()),
    )))
}

/// Volts to whole millivolts (negative readings clamp to 0).
fn volts_to_mv(v: f64) -> u32 {
    // The cast saturates; ADC volts are far below u32::MAX mV.
    (v * 1000.0).round().max(0.0) as u32
}

/// One ADC snapshot on `t` (traced with `--trace`; CONNECT where needed).
fn adc_snapshot<T: Transport, C: Clock>(
    cli: &Cli,
    t: T,
    clock: C,
) -> Result<AdcSnapshot, CliError> {
    fn snap<T: Transport, C: Clock>(
        cli: &Cli,
        mut session: Session<T, C>,
    ) -> Result<AdcSnapshot, CliError> {
        if needs_connect(cli.transport) {
            session.connect()?;
        }
        Ok(session.read_adc()?)
    }
    if cli.trace {
        snap(
            cli,
            Session::with_clock(TraceTransport::new(t, std::io::stderr()), clock),
        )
    } else {
        snap(cli, Session::with_clock(t, clock))
    }
}

/// `--device`, when given, must name the demo device's synthetic serial.
fn select_demo(cli: &Cli) -> Result<(), CliError> {
    let demo = [demo_entry()];
    select_device(&demo, cli.device.as_deref())?;
    Ok(())
}

/// `POWERZ_SIM_CLOCK=fake` (test-only): run the simulator on a fake clock.
fn fake_sim_clock() -> bool {
    std::env::var("POWERZ_SIM_CLOCK").as_deref() == Ok("fake")
}

/// Honour `POWERZ_SIM_CHARGER=<name>` (test-only): `epr-140w` (default),
/// `pps-65w` or `multi-protocol`.
fn sim_charger() -> Result<SimCharger, CliError> {
    match std::env::var("POWERZ_SIM_CHARGER").as_deref() {
        Err(_) | Ok("epr-140w") => Ok(SimCharger::Epr140W),
        Ok("pps-65w") => Ok(SimCharger::Pps65W),
        Ok("multi-protocol") => Ok(SimCharger::MultiProtocol),
        Ok(other) => Err(CliError::Usage(format!(
            "invalid POWERZ_SIM_CHARGER {other:?}: expected epr-140w, pps-65w or multi-protocol"
        ))),
    }
}

/// Honour the test-only simulated-meter knobs:
/// - `POWERZ_SIM_CABLE=epr-240w` puts the session-2 e-marked 240 W cable
///   on the simulated meter; unset, no cable.
/// - `POWERZ_SIM_ENTRY_BC=fail`: `entry bc` answers `fail`.
/// - `POWERZ_SIM_PDM_LAG=after-list`: one reply late after the `reset`
///   that ends a stalled list.
fn with_sim_knobs(sim: SimPdm) -> Result<SimPdm, CliError> {
    let sim = sim_knob(
        sim,
        "POWERZ_SIM_ENTRY_BC",
        "fail",
        SimPdm::with_entry_bc_fail,
    )?;
    let sim = sim_knob(
        sim,
        "POWERZ_SIM_PDM_LAG",
        "after-list",
        SimPdm::with_lag_after_list,
    )?;
    sim_knob(sim, "POWERZ_SIM_CABLE", "epr-240w", |s| {
        s.with_cable(SimCable::Epr240W)
    })
}

/// A one-value test knob: unset leaves `sim` as is, `var=value` applies
/// `apply`, anything else is a usage error.
fn sim_knob(
    sim: SimPdm,
    var: &str,
    value: &str,
    apply: impl FnOnce(SimPdm) -> SimPdm,
) -> Result<SimPdm, CliError> {
    match std::env::var(var).as_deref() {
        Err(_) => Ok(sim),
        Ok(v) if v == value => Ok(apply(sim)),
        Ok(other) => Err(CliError::Usage(format!(
            "invalid {var} {other:?}: expected {value}"
        ))),
    }
}

/// Wrap `port` for `--trace` if asked, open the PDM session and run.
fn pdm_on<P: Read + Write, C: Clock>(
    cli: &Cli,
    port: P,
    clock: C,
    hint: Option<Hint>,
) -> Result<(), CliError> {
    if cli.trace {
        let port = TracePort::new(port, std::io::stderr());
        run_pdm_session(cli, PdmSession::open_with_clock(port, clock)?, hint)
    } else {
        run_pdm_session(cli, PdmSession::open_with_clock(port, clock)?, hint)
    }
}

/// Run `trigger`, `detect` or `pd send` on an open PDM session.
fn run_pdm_session<P: Read + Write, C: Clock>(
    cli: &Cli,
    session: PdmSession<P, C>,
    hint: Option<Hint>,
) -> Result<(), CliError> {
    if let Some(send) = &cli.pd_file.send {
        let opts = PdSendOptions {
            hex: send.hex.as_deref(),
            preset: send.preset,
            sop: send.sop,
            allow_high_voltage: send.allow_high_voltage,
            json: send.json,
        };
        return pd_send(session, &opts, &mut std::io::stdout().lock());
    }
    match &cli.command {
        Command::Trigger { protocol } => run_trigger_session(protocol, session),
        Command::Console { allow_high_voltage } => console(
            session,
            *allow_high_voltage,
            std::io::BufReader::new(std::io::stdin()),
            &stop_flag(),
            &mut std::io::stdout().lock(),
        ),
        Command::Detect {
            list,
            list_plus,
            stall_secs,
            json,
        } => {
            let opts = DetectOptions {
                list: *list || *list_plus,
                plus: *list_plus,
                stall_limit: Duration::from_secs(*stall_secs),
                json: *json,
            };
            // Always computed for `detect` (see `detect_hint`); D+ / D- at
            // 0 V only if it somehow was not.
            let hint = hint.unwrap_or_else(|| dpdm_hint(0, 0));
            detect(
                session,
                &opts,
                &hint,
                &stop_flag(),
                &mut std::io::stdout().lock(),
            )
        }
        _ => unreachable!("only trigger and detect use the PDM session"),
    }
}

/// Run one `trigger` action on an open PDM session.
fn run_trigger_session<P: Read + Write, C: Clock>(
    protocol: &TriggerProtocol,
    session: PdmSession<P, C>,
) -> Result<(), CliError> {
    let mut out = std::io::stdout().lock();
    match protocol {
        TriggerProtocol::Pd {
            action: PdAction::List { json },
        } => trigger_pd_list(session, *json, &mut out),
        TriggerProtocol::Pd {
            action: PdAction::Reset { json },
        } => trigger_pd_reset(session, *json, &mut out),
        TriggerProtocol::Pd {
            action:
                PdAction::Request {
                    position,
                    voltage_mv,
                    current_ma,
                    allow_high_voltage,
                    hold,
                    json,
                },
        } => {
            let opts = PdRequestOptions {
                position: *position,
                voltage_mv: *voltage_mv,
                current_ma: *current_ma,
                allow_high_voltage: *allow_high_voltage,
                hold: hold_duration(*hold),
                json: *json,
            };
            trigger_pd_request(session, &opts, &mut ctrl_c_hold(), &mut out)
        }
        TriggerProtocol::Pd {
            action:
                PdAction::Settings {
                    pd_type,
                    em,
                    sink,
                    json,
                },
        } => {
            let change = PdSettingsChange {
                pd_type: pd_type.as_deref(),
                emarker: em.as_deref(),
                sink: sink.as_deref(),
            };
            trigger_pd_settings(session, &change, *json, &mut out)
        }
        TriggerProtocol::Qc { .. }
        | TriggerProtocol::Qc3 { .. }
        | TriggerProtocol::Fcp { .. }
        | TriggerProtocol::Afc { .. }
        | TriggerProtocol::Sfcp { .. }
        | TriggerProtocol::Scp { .. }
        | TriggerProtocol::Vfcp { .. }
        | TriggerProtocol::Tfcp { .. }
        | TriggerProtocol::Ufcs { .. } => match legacy_action(protocol) {
            Some(LegacyAction::List { protocol, json }) => {
                trigger_legacy_list(session, protocol, json, &mut out)
            }
            Some(LegacyAction::Request(opts)) => {
                trigger_legacy_request(session, &opts, &mut ctrl_c_hold(), &mut out)
            }
            None => Err(CliError::Usage("PD is triggered with `trigger pd`".into())),
        },
    }
}

/// A non-PD `trigger` action, mapped to the library.
enum LegacyAction {
    /// `trigger scp|vfcp|tfcp|ufcs list`.
    List {
        protocol: LegacyProtocol,
        json: bool,
    },
    /// Any request: enter, send, hold, back to 5 V.
    Request(LegacyRequestOptions),
}

/// The library call for a `trigger qc|qc3|fcp|afc|sfcp|scp|vfcp|tfcp|ufcs`
/// action; `None` for PD, which has its own flow.
fn legacy_action(protocol: &TriggerProtocol) -> Option<LegacyAction> {
    use LegacyProtocol as L;
    let (legacy, request, flags) = match protocol {
        TriggerProtocol::Qc {
            action: QcAction::Request { volts, flags },
        } => fixed(L::Qc, FixedProtocol::Qc, *volts, *flags),
        TriggerProtocol::Qc3 {
            action: Qc3Action::Request { voltage_mv, flags },
        } => (
            L::Qc,
            LegacyRequest::Qc3Volt {
                voltage_mv: *voltage_mv,
            },
            *flags,
        ),
        TriggerProtocol::Qc3 {
            action: Qc3Action::Inc { count, flags },
        } => (L::Qc, LegacyRequest::Qc3Inc(*count), *flags),
        TriggerProtocol::Qc3 {
            action: Qc3Action::Dec { count, flags },
        } => (L::Qc, LegacyRequest::Qc3Dec(*count), *flags),
        TriggerProtocol::Fcp {
            action: FixedAction::Request { volts, flags },
        } => fixed(L::Fcp, FixedProtocol::Fcp, *volts, *flags),
        TriggerProtocol::Afc {
            action: FixedAction::Request { volts, flags },
        } => fixed(L::Afc, FixedProtocol::Afc, *volts, *flags),
        TriggerProtocol::Sfcp {
            action: FixedAction::Request { volts, flags },
        } => fixed(L::Sfcp, FixedProtocol::Sfcp, *volts, *flags),
        TriggerProtocol::Scp { action }
        | TriggerProtocol::Vfcp { action }
        | TriggerProtocol::Tfcp { action } => {
            let legacy = match protocol {
                TriggerProtocol::Scp { .. } => L::Scp,
                TriggerProtocol::Vfcp { .. } => L::Vfcp,
                TriggerProtocol::Tfcp { .. } => L::Tfcp,
                _ => return None,
            };
            match action {
                LimitsAction::List { json } => {
                    return Some(LegacyAction::List {
                        protocol: legacy,
                        json: *json,
                    });
                }
                LimitsAction::Request {
                    voltage_mv,
                    current_ma,
                    flags,
                } => {
                    let (voltage_mv, current_ma) = (*voltage_mv, *current_ma);
                    let request = match legacy {
                        L::Scp => LegacyRequest::Scp {
                            voltage_mv,
                            current_ma,
                        },
                        L::Vfcp => LegacyRequest::Vfcp {
                            voltage_mv,
                            current_ma,
                        },
                        L::Tfcp => LegacyRequest::Tfcp {
                            voltage_mv,
                            current_ma,
                        },
                        _ => return None,
                    };
                    (legacy, request, *flags)
                }
            }
        }
        TriggerProtocol::Ufcs { action } => match action {
            UfcsAction::List { json } => {
                return Some(LegacyAction::List {
                    protocol: L::Ufcs,
                    json: *json,
                });
            }
            UfcsAction::Request {
                index,
                voltage_mv,
                current_ma,
                flags,
            } => (
                L::Ufcs,
                LegacyRequest::Ufcs {
                    index: *index,
                    voltage_mv: *voltage_mv,
                    current_ma: *current_ma,
                },
                *flags,
            ),
            UfcsAction::Cmd { number, flags } => (L::Ufcs, LegacyRequest::UfcsCmd(*number), *flags),
        },
        TriggerProtocol::Pd { .. } => return None,
    };
    Some(LegacyAction::Request(LegacyRequestOptions {
        protocol: legacy,
        request,
        allow_high_voltage: flags.allow_high_voltage,
        hold: hold_duration(flags.hold),
        json: flags.json,
    }))
}

/// The protocol, request and flags of a fixed-voltage `request <VOLTS>`.
fn fixed(
    legacy: LegacyProtocol,
    protocol: FixedProtocol,
    volts: u8,
    flags: RequestFlags,
) -> (LegacyProtocol, LegacyRequest, RequestFlags) {
    (legacy, LegacyRequest::Fixed { protocol, volts }, flags)
}

/// `--hold <SECS>`; without it, hold until Ctrl-C.
fn hold_duration(secs: Option<u64>) -> Duration {
    secs.map_or(Duration::MAX, Duration::from_secs)
}

/// The hold of a `trigger ... request`: Ctrl-C (handler installed by
/// run_pdm) ends it early; the meter still goes back to 5 V.
fn ctrl_c_hold() -> impl FnMut(Duration) {
    let stop = stop_flag();
    move |d: Duration| sleep_unless_stopped(d, &stop)
}

/// `--transport sim`: open the demo device (bulk persona, `--scenario`)
/// without touching USB; `--device` must name its synthetic serial.
fn run_sim(cli: &Cli) -> Result<(), CliError> {
    select_demo(cli)?;
    let mode = demo_entry().mode();
    let faults = sim_faults()?;
    // `pd` on the demo device plays the PD capture scenario.
    let with_pd = |sim: SimDevice| match cli.command {
        Command::Pd { .. } => sim.with_pd_scenario(PdScenario::Negotiation),
        _ => sim,
    };
    if fake_sim_clock() {
        let clock = SimClock::new(FAKE_CLOCK_EPOCH_MS);
        let sim = with_pd(SimDevice::with_clock(
            Persona::Bulk,
            cli.scenario,
            clock.clone(),
        ));
        let sim = sim_firmware_knobs(sim)?;
        with_ctrl_c_after(cli, mode, sim.with_faults(faults), clock)
    } else {
        let sim = sim_firmware_knobs(with_pd(SimDevice::new(Persona::Bulk, cli.scenario)))?;
        with_ctrl_c_after(cli, mode, sim.with_faults(faults), SystemClock)
    }
}

/// Honour the firmware test knobs (test-only):
///
/// - `POWERZ_SIM_HEAD=<n>`: HEAD answers with old-firmware field `n`.
/// - `POWERZ_SIM_APP_VERSION=<v>`: the application info block reports `v`.
fn sim_firmware_knobs(mut sim: SimDevice) -> Result<SimDevice, CliError> {
    if let Ok(n) = std::env::var("POWERZ_SIM_HEAD") {
        let field = n
            .parse::<u32>()
            .map_err(|e| CliError::Usage(format!("invalid POWERZ_SIM_HEAD {n:?}: {e}")))?;
        sim = sim.with_head_field(field);
    }
    if let Ok(v) = std::env::var("POWERZ_SIM_APP_VERSION") {
        sim = sim.with_app_version(&v);
    }
    Ok(sim)
}

/// Honour `POWERZ_SIM_FAULT=<plan>` (test-only, [`FaultPlan::parse`] syntax).
fn sim_faults() -> Result<FaultPlan, CliError> {
    match std::env::var("POWERZ_SIM_FAULT") {
        Ok(spec) => FaultPlan::parse(&spec)
            .map_err(|e| CliError::Usage(format!("invalid POWERZ_SIM_FAULT: {e}"))),
        Err(_) => Ok(FaultPlan::new()),
    }
}

/// Honour `POWERZ_SIM_CTRL_C_AFTER=N` (test-only): stand in for Ctrl-C once
/// the simulator has answered N GET_DATA requests.
fn with_ctrl_c_after<C: Clock>(
    cli: &Cli,
    mode: DeviceMode,
    sim: SimDevice,
    clock: C,
) -> Result<(), CliError> {
    match std::env::var("POWERZ_SIM_CTRL_C_AFTER") {
        Ok(n) => {
            let after = n.parse::<usize>().map_err(|e| {
                CliError::Usage(format!("invalid POWERZ_SIM_CTRL_C_AFTER {n:?}: {e}"))
            })?;
            let t = CtrlCAfter {
                inner: sim,
                remaining: after,
                stop: stop_flag(),
            };
            with_clock(cli, mode, t, clock)
        }
        Err(_) => with_clock(cli, mode, sim, clock),
    }
}

/// Transport wrapper that sets the Ctrl-C flag once `remaining` GET_DATA
/// requests have been answered.
struct CtrlCAfter<T> {
    inner: T,
    remaining: usize,
    stop: Arc<AtomicBool>,
}

impl<T: Transport> Transport for CtrlCAfter<T> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let reply = self.inner.transact(request, timeout)?;
        if frame::is_get_data(request) {
            self.remaining = self.remaining.saturating_sub(1);
            if self.remaining == 0 {
                self.stop.store(true, Ordering::SeqCst);
            }
        }
        Ok(reply)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }
}

/// [`with_clock`] on the system clock.
fn with_transport<T: Transport>(cli: &Cli, mode: DeviceMode, t: T) -> Result<(), CliError> {
    with_clock(cli, mode, t, SystemClock)
}

/// Wrap `t` for `--trace` if asked, open a session on `clock`, send CONNECT
/// where the pipe needs it and run.
fn with_clock<T: Transport, C: Clock>(
    cli: &Cli,
    mode: DeviceMode,
    t: T,
    clock: C,
) -> Result<(), CliError> {
    if cli.trace {
        start(
            cli,
            mode,
            Session::with_clock(TraceTransport::new(t, std::io::stderr()), clock),
        )
    } else {
        start(cli, mode, Session::with_clock(t, clock))
    }
}

/// CONNECT (bulk only) then dispatch.
fn start<T: Transport, C: Clock>(
    cli: &Cli,
    mode: DeviceMode,
    mut session: Session<T, C>,
) -> Result<(), CliError> {
    if needs_connect(cli.transport) {
        session.connect()?;
    }
    dispatch(cli, mode, session)
}

/// Run one device subcommand on an open session; `mode` is the opened
/// meter's mode, from its vendor interface's USB string.
fn dispatch<T: Transport, C: Clock>(
    cli: &Cli,
    mode: DeviceMode,
    mut session: Session<T, C>,
) -> Result<(), CliError> {
    let mut out = std::io::stdout().lock();
    match &cli.command {
        Command::List { .. } => unreachable!("list is handled before opening a device"),
        Command::Trigger { .. } | Command::Detect { .. } | Command::Console { .. } => {
            unreachable!("trigger, detect and console are handled before opening a device")
        }
        Command::Info { json } => info_in_mode(&mut session, mode, *json, &mut out),
        Command::Read { json } => read(&mut session, *json, &mut out),
        Command::Watch { interval } => {
            let stop = ctrl_c_flag()?;
            let opts = WatchOptions {
                interval: Duration::from_millis(*interval),
                tty: out.is_terminal(),
            };
            let stop_for_sleep = Arc::clone(&stop);
            watch(
                &mut session,
                &opts,
                &stop,
                &mut |d| sleep_unless_stopped(d, &stop_for_sleep),
                &mut out,
            )
        }
        Command::Stream {
            rate,
            csv,
            duration,
        } => {
            let rate = rate_from_sps(*rate)
                .ok_or_else(|| CliError::Usage(format!("invalid rate {rate}")))?;
            let duration = duration
                .map(Duration::try_from_secs_f64)
                .transpose()
                .map_err(|e| CliError::Usage(format!("invalid --duration: {e}")))?;
            let stop = ctrl_c_flag()?;
            // Created on the first write, i.e. only once the queue started.
            let file = LazyFile::new(csv.clone());
            // Stamped when the first record arrives (see StreamOptions).
            let start_unix_ms = 0;
            let opts = StreamOptions {
                rate,
                duration,
                start_unix_ms,
            };
            stream(
                &mut session,
                &opts,
                &stop,
                &mut std::io::BufWriter::new(file),
                &mut std::io::stderr().lock(),
            )
            .map(|_| ())
        }
        Command::Pd { json } => {
            let stop = ctrl_c_flag()?;
            match &cli.pd_file.save {
                Some(path) => {
                    // Created on the first write, i.e. once the meter answered.
                    let mut file = std::io::BufWriter::new(LazyFile::new(path.clone()));
                    pd_save(&mut session, *json, &stop, &mut out, &mut file)
                }
                None => pd(&mut session, *json, &stop, &mut out),
            }
        }
    }
}

/// Sleep for `d` in short slices, returning early once `stop` is set, so a
/// long `watch --interval` still honours Ctrl-C promptly.
fn sleep_unless_stopped(d: Duration, stop: &AtomicBool) {
    const SLICE: Duration = Duration::from_millis(50);
    // `None`: no deadline (a hold until Ctrl-C), not an overflow panic.
    let deadline = std::time::Instant::now().checked_add(d);
    while !stop.load(Ordering::SeqCst) {
        let now = std::time::Instant::now();
        let slice = match deadline {
            Some(deadline) if now >= deadline => return,
            Some(deadline) => SLICE.min(deadline - now),
            None => SLICE,
        };
        std::thread::sleep(slice);
    }
}

/// A file that is created (truncating any old one) on its first write, so a
/// command that fails before writing leaves an existing file untouched.
struct LazyFile {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
}

impl LazyFile {
    fn new(path: std::path::PathBuf) -> LazyFile {
        LazyFile { path, file: None }
    }

    fn file(&mut self) -> std::io::Result<&mut std::fs::File> {
        if self.file.is_none() {
            let f = std::fs::File::create(&self.path).map_err(|e| {
                std::io::Error::new(
                    e.kind(),
                    format!("cannot create {}: {e}", self.path.display()),
                )
            })?;
            self.file = Some(f);
        }
        Ok(self.file.as_mut().expect("just created"))
    }
}

impl std::io::Write for LazyFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file()?.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(f) => f.flush(),
            None => Ok(()),
        }
    }
}

/// The process-wide stop flag (set by Ctrl-C).
fn stop_flag() -> Arc<AtomicBool> {
    static STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    Arc::clone(STOP.get_or_init(|| Arc::new(AtomicBool::new(false))))
}

/// Install a Ctrl-C (and SIGTERM / SIGHUP) handler that sets the returned
/// flag, so `stream` always gets to stop the meter's queue.
fn ctrl_c_flag() -> Result<Arc<AtomicBool>, CliError> {
    let stop = stop_flag();
    let flag = Arc::clone(&stop);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))
        .map_err(|e| CliError::Device(format!("cannot install Ctrl-C handler: {e}")))?;
    Ok(stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_unless_stopped_returns_at_once_when_already_stopped() {
        let stop = AtomicBool::new(true);
        let t = std::time::Instant::now();
        sleep_unless_stopped(Duration::from_secs(10), &stop);
        assert!(t.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn hold_until_ctrl_c_ends_at_once_when_stopped() {
        // `--hold` left out holds without a deadline; that must not overflow.
        let stop = AtomicBool::new(true);
        let t = std::time::Instant::now();
        sleep_unless_stopped(Duration::MAX, &stop);
        assert!(t.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn lazy_file_is_not_created_until_written() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("powerz-lazy-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut f = LazyFile::new(path.clone());
        f.flush().unwrap();
        assert!(!path.exists());
        f.write_all(b"x").unwrap();
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }
}
