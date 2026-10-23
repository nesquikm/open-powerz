//! Running a parsed command line against real devices: discovery, device
//! selection, opening the chosen pipe, Ctrl-C and exit codes.

use std::io::{IsTerminal, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use powerz_protocol::frame;
use powerz_protocol::pdm::{self, PdmSession};
use powerz_protocol::session::{Clock, Session, SystemClock};
use powerz_protocol::transport::bulk::BulkTransport;
use powerz_protocol::transport::cdc::CdcTransport;
use powerz_protocol::transport::discovery::{DeviceEntry, Model, PID_KM003C};
use powerz_protocol::transport::hid::HidTransport;
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{
    FaultPlan, PdScenario, Persona, SIM_SERIAL, SimCharger, SimClock, SimDevice, SimPdm,
};

use crate::args::{
    Cli, Command, PdAction, PdDecode, TransportKind, TriggerProtocol, needs_connect, rate_from_sps,
    validate,
};
use crate::commands::{
    PdRequestOptions, PdSettingsChange, StreamOptions, WatchOptions, info, list, pd, pd_decode,
    pd_save, read, select_device, stream, trigger_pd_list, trigger_pd_request, trigger_pd_reset,
    trigger_pd_settings, watch,
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
    if let Command::Trigger { protocol } = &cli.command {
        return run_trigger(cli, protocol);
    }
    if cli.transport == TransportKind::Sim {
        return run_sim(cli);
    }
    let devices = transport::discovery::list_devices()?;
    let device = select_device(&devices, cli.device.as_deref())?;
    match cli.transport {
        TransportKind::Sim => unreachable!("the simulator is handled above"),
        TransportKind::Bulk => with_transport(cli, BulkTransport::open(device)?),
        TransportKind::Cdc => {
            let serial = device.serial.as_deref().ok_or_else(|| {
                CliError::NoDevice(
                    "the meter reports no serial number; cannot find its CDC port".into(),
                )
            })?;
            with_transport(cli, CdcTransport::open(serial)?)
        }
        TransportKind::Hid => with_transport(cli, HidTransport::open(device.serial.as_deref())?),
    }
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

/// `trigger`: open the meter's serial port (or, with `--transport sim`, the
/// simulated meter with the charger `POWERZ_SIM_CHARGER` names) and run.
fn run_trigger(cli: &Cli, protocol: &TriggerProtocol) -> Result<(), CliError> {
    // Before any command reaches the meter: a Ctrl-C must never kill the
    // process between a high-voltage request and the return to 5 V and
    // `pdm close`; it only sets the stop flag.
    ctrl_c_flag()?;
    if cli.transport != TransportKind::Sim {
        let port = pdm::open_port(cli.device.as_deref())?;
        return trigger_on(cli, protocol, port, SystemClock);
    }
    select_demo(cli)?;
    let charger = sim_charger()?;
    if fake_sim_clock() {
        let clock = SimClock::new(FAKE_CLOCK_EPOCH_MS);
        let sim = SimPdm::new(charger, clock.clone());
        trigger_on(cli, protocol, sim.open_port(), clock)
    } else {
        let sim = SimPdm::new(charger, SimClock::new(SystemClock.now_ms()));
        trigger_on(cli, protocol, sim.open_port(), SystemClock)
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

/// Honour `POWERZ_SIM_CHARGER=<name>` (test-only): `epr-140w` (default)
/// or `pps-65w`.
fn sim_charger() -> Result<SimCharger, CliError> {
    match std::env::var("POWERZ_SIM_CHARGER").as_deref() {
        Err(_) | Ok("epr-140w") => Ok(SimCharger::Epr140W),
        Ok("pps-65w") => Ok(SimCharger::Pps65W),
        Ok(other) => Err(CliError::Usage(format!(
            "invalid POWERZ_SIM_CHARGER {other:?}: expected epr-140w or pps-65w"
        ))),
    }
}

/// Wrap `port` for `--trace` if asked, open the PDM session and run.
fn trigger_on<P: Read + Write, C: Clock>(
    cli: &Cli,
    protocol: &TriggerProtocol,
    port: P,
    clock: C,
) -> Result<(), CliError> {
    if cli.trace {
        let port = TracePort::new(port, std::io::stderr());
        run_trigger_session(protocol, PdmSession::open_with_clock(port, clock)?)
    } else {
        run_trigger_session(protocol, PdmSession::open_with_clock(port, clock)?)
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
                // No --hold: hold until Ctrl-C.
                hold: hold.map_or(Duration::MAX, Duration::from_secs),
                json: *json,
            };
            // Ctrl-C (handler installed by run_trigger) ends the hold early;
            // the meter still goes back to 5 V.
            let stop = stop_flag();
            let mut hold = |d: Duration| sleep_unless_stopped(d, &stop);
            trigger_pd_request(session, &opts, &mut hold, &mut out)
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
    }
}

/// `--transport sim`: open the demo device (bulk persona, `--scenario`)
/// without touching USB; `--device` must name its synthetic serial.
fn run_sim(cli: &Cli) -> Result<(), CliError> {
    select_demo(cli)?;
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
        with_ctrl_c_after(cli, sim.with_faults(faults), clock)
    } else {
        let sim = with_pd(SimDevice::new(Persona::Bulk, cli.scenario));
        with_ctrl_c_after(cli, sim.with_faults(faults), SystemClock)
    }
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
fn with_ctrl_c_after<C: Clock>(cli: &Cli, sim: SimDevice, clock: C) -> Result<(), CliError> {
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
            with_clock(cli, t, clock)
        }
        Err(_) => with_clock(cli, sim, clock),
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
fn with_transport<T: Transport>(cli: &Cli, t: T) -> Result<(), CliError> {
    with_clock(cli, t, SystemClock)
}

/// Wrap `t` for `--trace` if asked, open a session on `clock`, send CONNECT
/// where the pipe needs it and run.
fn with_clock<T: Transport, C: Clock>(cli: &Cli, t: T, clock: C) -> Result<(), CliError> {
    if cli.trace {
        start(
            cli,
            Session::with_clock(TraceTransport::new(t, std::io::stderr()), clock),
        )
    } else {
        start(cli, Session::with_clock(t, clock))
    }
}

/// CONNECT (bulk only) then dispatch.
fn start<T: Transport, C: Clock>(cli: &Cli, mut session: Session<T, C>) -> Result<(), CliError> {
    if needs_connect(cli.transport) {
        session.connect()?;
    }
    dispatch(cli, session)
}

/// Run one device subcommand on an open session.
fn dispatch<T: Transport, C: Clock>(cli: &Cli, mut session: Session<T, C>) -> Result<(), CliError> {
    let mut out = std::io::stdout().lock();
    match &cli.command {
        Command::List { .. } => unreachable!("list is handled before opening a device"),
        Command::Trigger { .. } => unreachable!("trigger is handled before opening a device"),
        Command::Info { json } => info(&mut session, *json, &mut out),
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
