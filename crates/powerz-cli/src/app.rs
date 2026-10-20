//! Running a parsed command line against real devices: discovery, device
//! selection, opening the chosen pipe, Ctrl-C and exit codes.

use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use powerz_protocol::session::Session;
use powerz_protocol::transport::bulk::BulkTransport;
use powerz_protocol::transport::cdc::CdcTransport;
use powerz_protocol::transport::hid::HidTransport;
use powerz_protocol::transport::{self, Transport};

use crate::args::{Cli, Command, TransportKind, needs_connect, rate_from_sps, validate};
use crate::commands::{
    StreamOptions, WatchOptions, info, list, read, select_device, stream, watch,
};
use crate::error::CliError;
use crate::trace::TraceTransport;

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

/// Validate, discover, select the meter, open the chosen pipe and dispatch.
fn try_run(cli: &Cli) -> Result<(), CliError> {
    validate(cli)?;
    let devices = transport::discovery::list_devices()?;
    if let Command::List { json } = cli.command {
        return list(
            &devices,
            json,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr().lock(),
        );
    }
    let device = select_device(&devices, cli.device.as_deref())?;
    match cli.transport {
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

/// Wrap `t` for `--trace` if asked, open a session, send CONNECT where the
/// pipe needs it and run.
fn with_transport<T: Transport>(cli: &Cli, t: T) -> Result<(), CliError> {
    if cli.trace {
        start(
            cli,
            Session::open(TraceTransport::new(t, std::io::stderr())),
        )
    } else {
        start(cli, Session::open(t))
    }
}

/// CONNECT (bulk only) then dispatch.
fn start<T: Transport>(cli: &Cli, mut session: Session<T>) -> Result<(), CliError> {
    if needs_connect(cli.transport) {
        session.connect()?;
    }
    dispatch(cli, session)
}

/// Run one device subcommand on an open session.
fn dispatch<T: Transport>(cli: &Cli, mut session: Session<T>) -> Result<(), CliError> {
    let mut out = std::io::stdout().lock();
    match &cli.command {
        Command::List { .. } => unreachable!("list is handled before opening a device"),
        Command::Info { json } => info(&mut session, *json, &mut out),
        Command::Read { json } => read(&mut session, *json, &mut out),
        Command::Watch { interval } => {
            let stop = ctrl_c_flag()?;
            let opts = WatchOptions {
                interval: Duration::from_millis(*interval),
                tty: out.is_terminal(),
            };
            watch(
                &mut session,
                &opts,
                &stop,
                &mut std::thread::sleep,
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
            let file = std::fs::File::create(csv)
                .map_err(|e| CliError::Device(format!("cannot create {}: {e}", csv.display())))?;
            let start_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
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
    }
}

/// Install a Ctrl-C handler that sets the returned flag.
fn ctrl_c_flag() -> Result<Arc<AtomicBool>, CliError> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))
        .map_err(|e| CliError::Device(format!("cannot install Ctrl-C handler: {e}")))?;
    Ok(stop)
}
