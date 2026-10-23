//! Command-line arguments of `powerz` (clap definitions) and the argument
//! checks clap cannot express.

use std::path::PathBuf;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Parser, Subcommand, ValueEnum};
use powerz_protocol::decode::Rate;
use powerz_protocol::pdm::pd::{Emarker, PdType, SinkCaps};
use powerz_sim::Scenario;

use crate::error::CliError;

/// Which USB pipe to talk to the meter over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportKind {
    /// Vendor bulk interface: fastest, supports every command.
    Bulk,
    /// CDC virtual serial port: `read` and `watch` only
    Cdc,
    /// HID interface: no driver needed, `read` and `watch` only.
    Hid,
    /// Simulated KM003C demo device (no hardware); pick its data with --scenario.
    Sim,
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
    /// Scenario the demo device (`--transport sim`) plays.
    #[arg(
        long,
        global = true,
        value_name = "NAME",
        default_value = "idle",
        value_parser = scenario_parser(),
        help_heading = GLOBAL
    )]
    pub scenario: Scenario,
    /// Log every frame sent (`>`), received (`<`) or refused by the safety guard (`x`) to stderr as hex.
    #[arg(long, global = true, help_heading = GLOBAL)]
    pub trace: bool,
    /// `pd --save FILE` and `pd decode`, read from the `pd` subcommand.
    #[command(flatten)]
    pub pd_file: PdFile,
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
        /// Also list the simulated demo device (`--transport sim`).
        #[arg(long)]
        include_demo: bool,
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
    /// Capture USB PD traffic (messages and events) until Ctrl-C; bulk pipe only.
    #[command(
        arg = save_arg(),
        subcommands = [decode_command()],
        args_conflicts_with_subcommands = true
    )]
    Pd {
        /// Print one JSON object per record (JSON Lines) instead of text lines.
        #[arg(long)]
        json: bool,
    },
    /// Ask a charger for a voltage through the meter (protocol triggering).
    Trigger {
        /// Protocol to trigger.
        #[command(subcommand)]
        protocol: TriggerProtocol,
    },
    /// Detect the charger's fast-charging protocols (BC1.2, Apple, and the
    /// automatic list on request); never raises VBUS above 5 V.
    Detect {
        /// Also run the automatic protocol list (`entry list`).
        #[arg(long, conflicts_with = "list_plus")]
        list: bool,
        /// Run the extended list (`entry list+`, adds VOOC / MTK PE; needs a
        /// dummy load) instead.
        #[arg(long)]
        list_plus: bool,
        /// Seconds of silence after which the list ends with `reset`.
        #[arg(long, value_name = "N", default_value_t = 45,
              value_parser = clap::value_parser!(u64).range(1..))]
        stall_secs: u64,
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
}

/// `trigger` protocols.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum TriggerProtocol {
    /// USB Power Delivery: list offers, request a voltage.
    Pd {
        /// What to do.
        #[command(subcommand)]
        action: PdAction,
    },
    /// Quick Charge 2.0: request 5, 9, 12 or 20 V.
    Qc {
        /// What to do.
        #[command(subcommand)]
        action: QcAction,
    },
    /// Quick Charge 3.0: request a voltage (3.6-20 V, 200 mV steps) or step it
    /// (each run enters QC afresh, so steps count from the entry voltage).
    Qc3 {
        /// What to do.
        #[command(subcommand)]
        action: Qc3Action,
    },
    /// Huawei FCP: request 5, 9 or 12 V.
    Fcp {
        /// What to do.
        #[command(subcommand)]
        action: FixedAction,
    },
    /// Samsung AFC: request 5, 9 or 12 V.
    Afc {
        /// What to do.
        #[command(subcommand)]
        action: FixedAction,
    },
    /// Spreadtrum SFCP: request 5, 9 or 12 V.
    Sfcp {
        /// What to do.
        #[command(subcommand)]
        action: FixedAction,
    },
    /// Huawei SCP: show the charger's limits, or request a voltage and
    /// current inside them.
    Scp {
        /// What to do.
        #[command(subcommand)]
        action: LimitsAction,
    },
    /// vivo VFCP: show the charger's limits, or request a voltage and
    /// current inside them.
    Vfcp {
        /// What to do.
        #[command(subcommand)]
        action: LimitsAction,
    },
    /// Transsion TFCP: show the charger's limits, or request a voltage and
    /// current inside them.
    Tfcp {
        /// What to do.
        #[command(subcommand)]
        action: LimitsAction,
    },
    /// UFCS: list the charger's output capabilities by number, request a
    /// voltage and current inside one, or send a control message.
    Ufcs {
        /// What to do.
        #[command(subcommand)]
        action: UfcsAction,
    },
}

/// `trigger ufcs` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum UfcsAction {
    /// Enter UFCS and show its output capabilities, by number.
    List {
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
    /// Request a voltage and current inside capability N, hold it, then
    /// return to 5 V.
    Request {
        /// Capability number (1-based, as `list` shows it).
        #[arg(value_name = "N")]
        index: u8,
        /// Voltage, mV.
        #[arg(long, value_name = "MV")]
        voltage_mv: u32,
        /// Current, mA.
        #[arg(long, value_name = "MA")]
        current_ma: u32,
        #[command(flatten)]
        flags: RequestFlags,
    },
    /// Send UFCS control message N (by its number in the UFCS spec), then
    /// return to 5 V.
    Cmd {
        /// Control message number.
        #[arg(value_name = "N")]
        number: u8,
        #[command(flatten)]
        flags: RequestFlags,
    },
}

/// `trigger scp|vfcp|tfcp` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum LimitsAction {
    /// Enter the protocol and show the decoded limits of its capability block.
    List {
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
    /// Request a voltage and current inside the limits, hold it, then
    /// return to 5 V.
    Request {
        /// Voltage, mV.
        #[arg(long, value_name = "MV")]
        voltage_mv: u32,
        /// Current, mA.
        #[arg(long, value_name = "MA")]
        current_ma: u32,
        #[command(flatten)]
        flags: RequestFlags,
    },
}

/// Flags shared by every action that sends a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::Args)]
pub struct RequestFlags {
    /// Allow a request that raises VBUS above 5 V.
    #[arg(long)]
    pub allow_high_voltage: bool,
    /// Seconds to hold the voltage before returning to 5 V; without it
    /// the voltage is held until Ctrl-C.
    #[arg(long, value_name = "SECS")]
    pub hold: Option<u64>,
    /// Print one JSON object instead of labelled lines.
    #[arg(long)]
    pub json: bool,
}

/// `trigger qc` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum QcAction {
    /// Request a fixed voltage, hold it, then return to 5 V.
    Request {
        /// Volts: 5, 9, 12 or 20.
        #[arg(value_name = "VOLTS")]
        volts: u8,
        #[command(flatten)]
        flags: RequestFlags,
    },
}

/// `trigger fcp|afc|sfcp` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum FixedAction {
    /// Request a fixed voltage, hold it, then return to 5 V.
    Request {
        /// Volts: 5, 9 or 12.
        #[arg(value_name = "VOLTS")]
        volts: u8,
        #[command(flatten)]
        flags: RequestFlags,
    },
}

/// `trigger qc3` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Qc3Action {
    /// Request a voltage, hold it, then return to 5 V.
    Request {
        /// Voltage, mV (3600-20000 in 200 mV steps).
        #[arg(long, value_name = "MV")]
        voltage_mv: u32,
        #[command(flatten)]
        flags: RequestFlags,
    },
    /// Step the voltage up by N steps of 200 mV (always needs
    /// --allow-high-voltage).
    Inc {
        /// Number of steps.
        #[arg(value_name = "N")]
        count: u32,
        #[command(flatten)]
        flags: RequestFlags,
    },
    /// Step the voltage down by N steps of 200 mV.
    Dec {
        /// Number of steps.
        #[arg(value_name = "N")]
        count: u32,
        #[command(flatten)]
        flags: RequestFlags,
    },
}

/// `trigger pd` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum PdAction {
    /// Enter PD and list the charger's offers (SPR and EPR), decoded.
    List {
        /// Print one JSON object instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Request an offer by position (PPS / AVS with --voltage-mv and
    /// --current-ma), hold it, then return to 5 V.
    Request {
        /// Object position of the offer (see `trigger pd list`; EPR is 8 and up).
        #[arg(value_name = "POS")]
        position: u8,
        /// Voltage for a PPS / AVS offer, mV.
        #[arg(long, value_name = "MV")]
        voltage_mv: Option<u32>,
        /// Current, mA (required with --voltage-mv; default for a fixed offer: its maximum).
        #[arg(long, value_name = "MA")]
        current_ma: Option<u32>,
        /// Allow a request that raises VBUS above 5 V.
        #[arg(long)]
        allow_high_voltage: bool,
        /// Seconds to hold the voltage before returning to 5 V; without it
        /// the voltage is held until Ctrl-C.
        #[arg(long, value_name = "SECS")]
        hold: Option<u64>,
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
    /// Send a Hard Reset: the charger returns to 5 V and offers again; list
    /// the new offers and VBUS.
    Reset {
        /// Print one JSON object instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Show the meter's PD settings, or change them (a flag left out keeps
    /// the current value).
    Settings {
        /// PD type.
        #[arg(long = "type", value_name = "NAME",
              value_parser = PossibleValuesParser::new(PdType::all().iter().map(|v| v.name())))]
        pd_type: Option<String>,
        /// e-marker emulation (the reserved codes are shown but never set:
        /// what the firmware does with them is unknown).
        #[arg(long, value_name = "NAME",
              value_parser = PossibleValuesParser::new(
                  Emarker::all().iter().map(|v| v.name()).filter(|n| !n.starts_with("reserved"))))]
        em: Option<String>,
        /// Sink capabilities emulation.
        #[arg(long, value_name = "NAME",
              value_parser = PossibleValuesParser::new(SinkCaps::all().iter().map(|v| v.name())))]
        sink: Option<String>,
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
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
            Command::Pd { .. } => "pd",
            Command::Trigger { .. } => "trigger",
            Command::Detect { .. } => "detect",
        }
    }
}

/// `--save FILE` of `pd`.
fn save_arg() -> clap::Arg {
    clap::Arg::new(SAVE)
        .long(SAVE)
        .value_name("FILE")
        .value_parser(clap::value_parser!(PathBuf))
        .help("Also save the capture (status samples and records) to FILE as a pdcap file")
}

/// `pd decode [--json] FILE`.
fn decode_command() -> clap::Command {
    clap::Command::new(DECODE)
        .about("Print the records of a saved pdcap file as `pd` would; no meter needed")
        .arg(
            clap::Arg::new("json")
                .long("json")
                .action(clap::ArgAction::SetTrue)
                .help("Print one JSON object per record (JSON Lines) instead of text lines"),
        )
        .arg(
            clap::Arg::new("file")
                .value_name("FILE")
                .required(true)
                .value_parser(clap::value_parser!(PathBuf))
                .help("pdcap file to decode"),
        )
}

const SAVE: &str = "save";
const DECODE: &str = "decode";

/// What the `pd` subcommand does besides a plain live capture: `--save`
/// and `decode` are declared on `pd` and read back here, so
/// [`Command::Pd`] keeps only its `--json` flag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdFile {
    /// `pd --save FILE`: also write the live capture to FILE.
    pub save: Option<PathBuf>,
    /// `pd decode [--json] FILE`: decode a saved file instead of capturing.
    pub decode: Option<PdDecode>,
}

/// Arguments of `pd decode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdDecode {
    /// Print JSON Lines instead of text lines.
    pub json: bool,
    /// The pdcap file.
    pub file: PathBuf,
}

impl clap::FromArgMatches for PdFile {
    fn from_arg_matches(m: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let Some(pd) = m.subcommand_matches("pd") else {
            return Ok(PdFile::default());
        };
        let decode = pd.subcommand_matches(DECODE).and_then(|d| {
            d.get_one::<PathBuf>("file").map(|file| PdDecode {
                json: d.get_flag("json"),
                file: file.clone(),
            })
        });
        Ok(PdFile {
            save: pd.get_one::<PathBuf>(SAVE).cloned(),
            decode,
        })
    }

    fn update_from_arg_matches(&mut self, m: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = PdFile::from_arg_matches(m)?;
        Ok(())
    }
}

/// Adds no arguments of its own: they sit on the `pd` subcommand.
impl clap::Args for PdFile {
    fn augment_args(cmd: clap::Command) -> clap::Command {
        cmd
    }

    fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
        cmd
    }
}

/// The queue rate whose samples-per-second is `sps`, if there is one.
pub(crate) fn rate_from_sps(sps: u32) -> Option<Rate> {
    Rate::ALL.into_iter().find(|r| r.sps() == sps)
}

/// Parse `--scenario`: one of the demo device's scenario names.
fn scenario_parser() -> impl TypedValueParser<Value = Scenario> {
    PossibleValuesParser::new(Scenario::ALL.map(Scenario::name))
        .map(|name| Scenario::from_name(&name).unwrap_or(Scenario::Idle))
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
    if cli.pd_file.decode.is_some() {
        // `pd decode` reads a file and opens no device.
        return Ok(());
    }
    let hid_ok = matches!(cli.command, Command::Read { .. } | Command::Watch { .. });
    // `trigger` talks to the meter's serial port whatever `--transport` says
    // (bulk / cdc / sim); HID has no text session.
    if matches!(
        cli.command,
        Command::Trigger { .. } | Command::Detect { .. }
    ) && cli.transport != TransportKind::Hid
    {
        return Ok(());
    }
    if cli.transport == TransportKind::Hid && !hid_ok {
        return Err(CliError::Usage(format!(
            "--transport hid supports only `read` and `watch`, not `{}`",
            cli.command.name()
        )));
    }
    // FW 2.0.6 answers no memory read over CDC (it needs CONNECT, whose byte
    // starts the ADCNEW stream there), so device info and auth need bulk.
    if cli.transport == TransportKind::Cdc && !hid_ok {
        return Err(CliError::Usage(format!(
            "--transport cdc supports only `read` and `watch`, not `{}`; use --transport bulk",
            cli.command.name()
        )));
    }
    Ok(())
}

/// Whether a session on `kind` must send CONNECT before device commands.
///
/// FW 2.0.6 REJECTs memory reads (and so `info` and auth) until CONNECT. On
/// CDC the byte 0x02 starts the ADCNEW push stream instead, and HID never
/// answers CONNECT, so only bulk (and the simulator's bulk persona) sends it.
pub fn needs_connect(kind: TransportKind) -> bool {
    matches!(kind, TransportKind::Bulk | TransportKind::Sim)
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

    fn pd_action(args: &[&str]) -> Result<PdAction, clap::Error> {
        let cli = Cli::try_parse_from(["powerz", "trigger", "pd"].iter().chain(args))?;
        match cli.command {
            Command::Trigger {
                protocol: TriggerProtocol::Pd { action },
            } => Ok(action),
            other => panic!("parsed {other:?}"),
        }
    }

    // ---- AC-DGSQ6R.4: a stall limit of zero would end every list at once ----

    #[test]
    fn stall_secs_must_be_at_least_one() {
        let parse =
            |v: &str| Cli::try_parse_from(["powerz", "detect", "--list", "--stall-secs", v]);
        assert!(parse("0").is_err());
        assert!(parse("1").is_ok());
    }

    // ---- AC-DGSQ6P.6: a request holds until Ctrl-C unless --hold is given ----

    #[test]
    fn request_holds_until_ctrl_c_by_default() {
        match pd_action(&["request", "4"]).unwrap() {
            PdAction::Request { hold, .. } => assert_eq!(hold, None),
            other => panic!("{other:?}"),
        }
        match pd_action(&["request", "4", "--hold", "0"]).unwrap() {
            PdAction::Request { hold, .. } => assert_eq!(hold, Some(0)),
            other => panic!("{other:?}"),
        }
    }

    // ---- AC-DGSQ6P.4: reserved e-marker codes can be read, never written ----

    #[test]
    fn reserved_emarker_values_cannot_be_set() {
        for name in ["reserved-1", "reserved-2"] {
            assert!(pd_action(&["settings", "--em", name]).is_err(), "{name}");
        }
        assert!(pd_action(&["settings", "--em", "la135"]).is_ok());
    }
}
