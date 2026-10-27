//! Command-line arguments of `powerz` (clap definitions) and the argument
//! checks clap cannot express.

use std::path::PathBuf;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Parser, Subcommand, ValueEnum};
use powerz_protocol::decode::Rate;
use powerz_protocol::pdm::message::{Preset, SopTarget};
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

/// Where `powerz watch` and `powerz stream` get their values from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum WatchSource {
    /// Poll the meter over --transport: one ADC snapshot per refresh
    /// (`watch`), the ADC queue (`stream`).
    #[default]
    Poll,
    /// The ADCNEW push stream on the meter's CDC serial port: no sign-in,
    /// no temperature.
    Cdc,
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
    /// Show the meter's settings block (read-only): display, sample rate and
    /// raw calibration values, each tagged confirmed or inferred.
    Settings {
        /// Print one JSON object instead of tagged lines.
        #[arg(long, conflicts_with = "raw")]
        json: bool,
        /// Hex-dump the 180-byte block instead of decoding it.
        #[arg(long)]
        raw: bool,
    },
    /// List the meter's offline records or download them as CSV files
    /// (read-only; bulk pipe only).
    Records {
        /// What to do.
        #[command(subcommand)]
        action: RecordsAction,
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
        /// Where the values come from: `poll` asks for a snapshot each
        /// refresh, `cdc` reads the ADCNEW stream on the CDC serial port
        /// (whatever --transport says, except hid).
        #[arg(long, value_enum, value_name = "SOURCE", default_value = "poll")]
        source: WatchSource,
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
        /// Where the samples come from: `poll` reads the ADC queue over
        /// --transport, `cdc` records the ADCNEW stream on the CDC serial
        /// port at 50 or 1000 records/s (whatever --transport says, except
        /// hid) and reports gaps, dropped frames and CRC errors.
        #[arg(long, value_enum, value_name = "SOURCE", default_value = "poll")]
        source: WatchSource,
    },
    /// Capture USB PD traffic (messages and events) until Ctrl-C; bulk pipe only.
    #[command(
        arg = save_arg(),
        subcommands = [decode_command(), send_command()],
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
    /// Raw PDM text console: send command lines from stdin and print what
    /// was sent and received, with times.
    Console {
        /// Allow lines that can raise VBUS above 5 V (and any unknown text).
        #[arg(long)]
        allow_high_voltage: bool,
    },
    /// Work with firmware files (`info` needs no meter), probe the meter's
    /// firmware identity (`probe`, read-only), flash a firmware file
    /// (`flash`, needs `--i-understand`) and switch the meter's mode (`jump`,
    /// needs `--i-understand`).
    Firmware {
        /// What to do.
        #[command(subcommand)]
        action: FirmwareAction,
    },
}

/// `records` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum RecordsAction {
    /// Show the offline records stored on the meter: name, interval,
    /// points, duration, charge, energy and offset.
    List {
        /// Print one JSON object instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Download record N (as `list` numbers it) or every record as CSV files
    /// named `NN_<name>.csv`, in the `stream` CSV layout.
    #[command(group = clap::ArgGroup::new("which").required(true).args(["index", "all"]))]
    Get {
        /// Record number (1-based, as `list` shows it).
        #[arg(value_name = "N")]
        index: Option<usize>,
        /// Download every record that has samples.
        #[arg(long)]
        all: bool,
        /// Directory to write the CSV files into (must exist).
        #[arg(long, value_name = "DIR", default_value = ".")]
        out: PathBuf,
        /// Print one JSON object listing the written files instead of text lines.
        #[arg(long)]
        json: bool,
    },
}

/// `firmware` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum FirmwareAction {
    /// Show a firmware file's model, version, date, CRC, size and English
    /// release notes; no meter needed, nothing is decrypted.
    Info {
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
        /// Firmware container file to inspect.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Show the meter's USB identity, then send only CONNECT and HEAD over
    /// the vendor bulk pipe and print the HEAD field; read-only, in
    /// application or bootloader mode.
    Probe {
        /// Print one JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
    /// Flash a firmware container onto the meter through its bootloader
    /// (bulk pipe only). The file is inspected before any meter is opened;
    /// nothing is sent without `--i-understand`.
    Flash {
        /// Firmware container file to flash.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Accept the risk: a failed or interrupted flash leaves the meter in
        /// its bootloader until the flash is retried.
        #[arg(long)]
        i_understand: bool,
    },
    /// Switch the meter between application and bootloader mode with one
    /// write-only jump (JUMP_DFU or JUMP_APROM) over the vendor bulk pipe,
    /// then report the mode it comes back in; nothing is sent without
    /// `--i-understand`.
    Jump {
        /// The mode to switch to.
        #[arg(long, value_enum)]
        to: JumpTo,
        /// Accept the risk: in its bootloader the meter takes no
        /// measurements until it is jumped back or power-cycled.
        #[arg(long)]
        i_understand: bool,
    },
}

/// `firmware jump --to` targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum JumpTo {
    /// The bootloader (JUMP_DFU, from application mode).
    Bootloader,
    /// The application firmware (JUMP_APROM, from bootloader mode).
    Application,
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
    /// The source of `watch` and `stream`; `None` for other commands.
    pub fn source(&self) -> Option<WatchSource> {
        match self {
            Command::Watch { source, .. } | Command::Stream { source, .. } => Some(*source),
            _ => None,
        }
    }

    /// Subcommand name as typed on the command line.
    pub fn name(&self) -> &'static str {
        match self {
            Command::List { .. } => "list",
            Command::Info { .. } => "info",
            Command::Read { .. } => "read",
            Command::Settings { .. } => "settings",
            Command::Records { .. } => "records",
            Command::Watch { .. } => "watch",
            Command::Stream { .. } => "stream",
            Command::Pd { .. } => "pd",
            Command::Trigger { .. } => "trigger",
            Command::Detect { .. } => "detect",
            Command::Console { .. } => "console",
            Command::Firmware { .. } => "firmware",
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

/// `pd send [HEX] [--preset NAME] [--sop sop|sop1|sop2] ...` (FR DGSQ6T).
fn send_command() -> clap::Command {
    use clap::{Arg, ArgAction};
    clap::Command::new(SEND)
        .about(
            "Send a custom PD message through the meter (a control message on SOP \
             as `pd cmd=`, everything else as `pd data=`)",
        )
        .arg(
            Arg::new("hex")
                .value_name("HEX")
                .required_unless_present("preset")
                .conflicts_with("preset")
                .help("PD message as hex: header little-endian, then data objects little-endian; no SOP byte"),
        )
        .arg(
            Arg::new("preset")
                .long("preset")
                .value_name("NAME")
                .value_parser(PossibleValuesParser::new(Preset::ALL.map(Preset::name)))
                .help("Send a built-in message instead of HEX"),
        )
        .arg(
            Arg::new("sop")
                .long("sop")
                .value_name("TARGET")
                .value_parser(PossibleValuesParser::new(["sop", "sop1", "sop2"]))
                .default_value("sop")
                .help("Target: sop (the charger), sop1 (SOP', near cable plug), sop2 (SOP'', far plug)"),
        )
        .arg(
            Arg::new("allow_high_voltage")
                .long("allow-high-voltage")
                .action(ArgAction::SetTrue)
                .help("Allow a message that can raise VBUS above 5 V"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Print one JSON object instead of labelled lines"),
        )
}

const SAVE: &str = "save";
const DECODE: &str = "decode";
const SEND: &str = "send";

/// Arguments of `pd send`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdSend {
    /// The message as hex (no SOP byte), when no preset is given.
    pub hex: Option<String>,
    /// `--preset`.
    pub preset: Option<Preset>,
    /// `--sop`.
    pub sop: SopTarget,
    /// `--allow-high-voltage`.
    pub allow_high_voltage: bool,
    /// `--json`.
    pub json: bool,
}

/// What the `pd` subcommand does besides a plain live capture: `--save`
/// and `decode` are declared on `pd` and read back here, so
/// [`Command::Pd`] keeps only its `--json` flag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdFile {
    /// `pd --save FILE`: also write the live capture to FILE.
    pub save: Option<PathBuf>,
    /// `pd decode [--json] FILE`: decode a saved file instead of capturing.
    pub decode: Option<PdDecode>,
    /// `pd send ...`: send a custom PD message instead of capturing.
    pub send: Option<PdSend>,
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
        let send = pd.subcommand_matches(SEND).map(|m| PdSend {
            hex: m.get_one::<String>("hex").cloned(),
            preset: m
                .get_one::<String>("preset")
                .and_then(|n| Preset::from_name(n)),
            sop: m
                .get_one::<String>("sop")
                .and_then(|n| SopTarget::from_name(n))
                .unwrap_or(SopTarget::Sop),
            allow_high_voltage: m.get_flag("allow_high_voltage"),
            json: m.get_flag("json"),
        });
        Ok(PdFile {
            save: pd.get_one::<PathBuf>(SAVE).cloned(),
            decode,
            send,
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
    // The probe needs the vendor bulk pipe (a bootloader has no other).
    if let Command::Firmware { action } = &cli.command
        && matches!(
            action,
            FirmwareAction::Probe { .. }
                | FirmwareAction::Flash { .. }
                | FirmwareAction::Jump { .. }
        )
        && matches!(cli.transport, TransportKind::Hid | TransportKind::Cdc)
    {
        let name = match action {
            FirmwareAction::Flash { .. } => "flash",
            FirmwareAction::Jump { .. } => "jump",
            _ => "probe",
        };
        return Err(CliError::Usage(format!(
            "`firmware {name}` runs on the vendor bulk pipe only; use --transport bulk (or sim)"
        )));
    }
    if cli.command.source() == Some(WatchSource::Cdc) {
        // The ADCNEW stream exists only on the CDC port; HID has none.
        let name = cli.command.name();
        if cli.transport == TransportKind::Hid {
            return Err(CliError::Usage(format!(
                "`{name} --source cdc` reads the CDC serial port; it cannot run over --transport hid"
            )));
        }
        if let Command::Stream { rate, .. } = cli.command
            && !matches!(rate, 50 | 1000)
        {
            return Err(CliError::Usage(format!(
                "`stream --source cdc` records 50 or 1000 records/s, not {rate}"
            )));
        }
        return Ok(());
    }
    let hid_ok = matches!(cli.command, Command::Read { .. } | Command::Watch { .. });
    // `pd send` talks to the meter's serial port like `trigger`.
    if cli.pd_file.send.is_some() && cli.transport != TransportKind::Hid {
        return Ok(());
    }
    // `trigger` talks to the meter's serial port whatever `--transport` says
    // (bulk / cdc / sim); HID has no text session.
    if matches!(
        cli.command,
        Command::Trigger { .. } | Command::Detect { .. } | Command::Console { .. }
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
