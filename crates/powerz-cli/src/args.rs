//! Command-line arguments of `powerz` (clap definitions) and the argument
//! checks clap cannot express.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use powerz_protocol::decode::Rate;

use crate::error::CliError;

/// Which USB pipe to talk to the meter over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportKind {
    /// Vendor bulk interface: fastest, supports every command.
    Bulk,
    /// CDC virtual serial port.
    Cdc,
    /// HID interface: no driver needed, `read` and `watch` only.
    Hid,
}

/// Help heading of the flags accepted by every subcommand.
const GLOBAL: &str = "Global options";

/// Command-line arguments of `powerz`.
#[derive(Debug, Parser)]
#[command(
    name = "powerz",
    version,
    about = "POWER-Z KM003C / KM002C command-line tool (unofficial)",
    long_about = "POWER-Z KM003C / KM002C command-line tool (unofficial).\n\n\
                  Lists attached meters, shows device information, reads live \
                  VBUS/IBUS/D+/D-/CC values and records the ADC stream to CSV."
)]
pub struct Cli {
    /// Serial number of the meter to use when several are attached.
    #[arg(long, global = true, value_name = "SERIAL", help_heading = GLOBAL)]
    pub device: Option<String>,
    /// USB pipe to talk to the meter over.
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "PIPE",
        default_value = "bulk",
        help_heading = GLOBAL
    )]
    pub transport: TransportKind,
    /// Log every frame sent (`>`) and received (`<`) to stderr as hex.
    #[arg(long, global = true, help_heading = GLOBAL)]
    pub trace: bool,
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// `powerz` subcommands.
#[derive(Debug, Clone, PartialEq, Subcommand)]
pub enum Command {
    /// List attached meters with model, serial and USB bus/address.
    List {
        /// Print a JSON array instead of one line per meter.
        #[arg(long)]
        json: bool,
    },
    /// Show model, firmware and hardware versions, serial and UID.
    Info {
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
    /// Take one snapshot of VBUS, IBUS, power, temperature, D+/D- and CC1/CC2.
    Read {
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
    /// Show the `read` values live, refreshing until Ctrl-C.
    Watch {
        /// Refresh interval in milliseconds (minimum 50).
        #[arg(
            long,
            value_name = "MS",
            default_value_t = 200,
            value_parser = clap::value_parser!(u64).range(50..)
        )]
        interval: u64,
    },
    /// Record the ADC sample stream to a CSV file until Ctrl-C or --duration.
    Stream {
        /// Sample rate in samples per second: 2, 10, 50 or 1000.
        #[arg(long, value_name = "SPS", value_parser = parse_rate)]
        rate: u32,
        /// CSV file to write (created or overwritten).
        #[arg(long, value_name = "FILE")]
        csv: PathBuf,
        /// Stop after this many seconds of device time (fractions allowed).
        #[arg(long, value_name = "SECONDS")]
        duration: Option<f64>,
    },
}

impl Command {
    /// Subcommand name as typed on the command line.
    pub fn name(&self) -> &'static str {
        match self {
            Command::List { .. } => "list",
            Command::Info { .. } => "info",
            Command::Read { .. } => "read",
            Command::Watch { .. } => "watch",
            Command::Stream { .. } => "stream",
        }
    }
}

/// The queue rate whose samples-per-second is `sps`, if there is one.
pub(crate) fn rate_from_sps(sps: u32) -> Option<Rate> {
    Rate::ALL.into_iter().find(|r| r.sps() == sps)
}

/// Parse `--rate`: one of the queue rates 2, 10, 50, 1000.
fn parse_rate(s: &str) -> Result<u32, String> {
    s.parse::<u32>()
        .ok()
        .filter(|&n| rate_from_sps(n).is_some())
        .ok_or_else(|| format!("invalid rate {s:?}: expected 2, 10, 50 or 1000"))
}

/// Check argument combinations clap cannot express (e.g. `--transport hid`
/// is accepted only for `read` and `watch`). Runs before any device access.
pub fn validate(cli: &Cli) -> Result<(), CliError> {
    let hid_ok = matches!(cli.command, Command::Read { .. } | Command::Watch { .. });
    if cli.transport == TransportKind::Hid && !hid_ok {
        return Err(CliError::Usage(format!(
            "--transport hid supports only `read` and `watch`, not `{}`",
            cli.command.name()
        )));
    }
    Ok(())
}

/// Whether a session on `kind` must send CONNECT before device commands.
///
/// FW 2.0.6 REJECTs memory reads (and so `info` and auth) until CONNECT. On
/// CDC the byte 0x02 starts the ADCNEW push stream instead, and HID never
/// answers CONNECT, so only bulk sends it.
pub fn needs_connect(kind: TransportKind) -> bool {
    matches!(kind, TransportKind::Bulk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_is_sent_on_bulk_only() {
        // Memory read is REJECTed until CONNECT (FW 2.0.6); on CDC the byte
        // 0x02 starts the ADCNEW stream instead, and HID ignores CONNECT.
        assert!(needs_connect(TransportKind::Bulk));
        assert!(!needs_connect(TransportKind::Cdc));
        assert!(!needs_connect(TransportKind::Hid));
    }
}
