//! The raw PDM console (FR DGSQ6T): send any text line through the
//! session and collect the transcript.
//!
//! A line is parsed with [`Command::parse`] (unknown text is
//! [`Command::Raw`]), so the session's guard, pacing, GoodCRC refusal and
//! risk gate apply unchanged: a line classified above 5 V (including any
//! unparsable one) needs a [`HighVoltageConfirmation`] made for
//! `Command::parse(line)`.

use std::io::{Read, Write};
use std::time::Duration;

use super::caps::CapBlock;
use super::command::{Command, Protocol};
use super::reply::ReplyItem;
use super::risk::Risk;
use super::session::{HighVoltageConfirmation, PdmSession};
use super::{BlockTag, Error};
use crate::session::Clock;

/// How long the console listens for replies after each line.
pub const LISTEN: Duration = Duration::from_millis(300);

/// Which way a console line went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Written to the meter.
    Tx,
    /// Read from the meter.
    Rx,
}

/// One line of the console transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleEntry {
    /// Clock time, ms since the Unix epoch (the session's clock).
    pub at_ms: u64,
    /// Sent or received.
    pub direction: Direction,
    /// The text sent, the reply line, or the block's tag for a block.
    pub text: String,
    /// The parsed binary block, for a received block.
    pub block: Option<CapBlock>,
}

/// The command a console line sends (trimmed; unknown text is raw).
pub fn command_of(line: &str) -> Command {
    Command::parse(line.trim())
}

/// What a sent console line leaves behind that closing the session has to
/// undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// `entry pd`: the meter is in PD (its offers are in the session's
    /// capabilities).
    EnteredPd,
    /// A PD request or message classified above 5 V: it may have changed the
    /// contract, so it is ended through PD (`pd req=1`, Hard Reset as the
    /// fallback).
    PdContract,
    /// A legacy protocol was entered, or a legacy request above 5 V was sent:
    /// it is ended with `reset`.
    Legacy,
    /// Confirmed text the parser does not know: what it did is unknown. With
    /// PD entered it is treated as [`Effect::PdContract`]; otherwise
    /// `pdm close` ends it.
    RawLine,
}

/// What sending `command` (classified above 5 V when `above`) leaves to
/// undo; `None` when nothing.
pub fn effect_of(command: &Command, above: bool) -> Option<Effect> {
    match command {
        Command::Entry(Protocol::Pd) => Some(Effect::EnteredPd),
        Command::Entry(Protocol::Bc | Protocol::Apple | Protocol::List) => None,
        Command::Entry(_) => Some(Effect::Legacy),
        Command::PdRequest { .. } | Command::PdCmd(_) | Command::PdData(_) if above => {
            Some(Effect::PdContract)
        }
        Command::Raw(_) if above => Some(Effect::RawLine),
        Command::FixedVoltage { .. }
        | Command::Qc3Volt { .. }
        | Command::Qc3Inc(_)
        | Command::ScpRequest { .. }
        | Command::VfcpRequest { .. }
        | Command::TfcpRequest { .. }
        | Command::UfcsRequest { .. }
            if above =>
        {
            Some(Effect::Legacy)
        }
        _ => None,
    }
}

/// A console line that was written: its transcript, what it leaves to
/// undo, and the error that ended listening early, if any. Once the line is
/// written its effect is always reported, even when reading the replies
/// failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentLine {
    /// The sent line first, then every reply line and parsed block.
    pub entries: Vec<ConsoleEntry>,
    /// What closing the session has to undo for this line.
    pub effect: Option<Effect>,
    /// Reading the replies failed after the line was written.
    pub error: Option<Error>,
}

/// Why the console never sends `command` itself (`pdm open` / `pdm close`:
/// the session opens and closes itself, so `pdm close` is never sent twice),
/// or `None`. Judged on the normalised text (lowercase, no whitespace),
/// since the firmware matches prefixes.
pub fn console_refusal(command: &Command) -> Option<String> {
    let norm: String = String::from_utf8_lossy(&command.render())
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    (norm.starts_with("pdmopen") || norm.starts_with("pdmclose")).then(|| {
        "the console opens and closes the PDM session itself; `pdm open` and \
         `pdm close` are not sent by hand"
            .to_owned()
    })
}

/// Send `line` and collect what arrives for `listen` after it: the sent
/// line first, then every reply line and parsed block, each with its time.
/// `Err` only when nothing was written: refused for GoodCRC, for `pdm open`
/// / `pdm close`, and for a line classified above 5 V without a matching
/// `confirmation`. After the write, a read failure is in
/// [`SentLine::error`] next to the line's [`SentLine::effect`].
pub fn send_line<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    line: &str,
    confirmation: Option<HighVoltageConfirmation>,
    listen: Duration,
) -> Result<SentLine, Error> {
    let text = line.trim();
    let command = command_of(text);
    // GoodCRC and session control are refused whatever the confirmation,
    // before anything is written.
    if let Some(reason) = super::message::refusal(&command).or_else(|| console_refusal(&command)) {
        return Err(Error::Refused {
            command: text.to_owned(),
            reason,
        });
    }
    // The risk gate: above 5 V (unparsable text included) needs a
    // confirmation made for this exact command.
    let above = session.risk(&command) == Risk::Above5V;
    match (above, confirmation) {
        (true, Some(c)) => session.send_confirmed(&command, c)?,
        (true, None) => {
            return Err(Error::HighVoltageNotConfirmed {
                command: text.to_owned(),
            });
        }
        (false, _) => session.send(&command)?,
    }
    let mut sent = SentLine {
        entries: vec![ConsoleEntry {
            at_ms: session.now_ms(),
            direction: Direction::Tx,
            text: text.to_owned(),
            block: None,
        }],
        effect: effect_of(&command, above),
        error: None,
    };
    let entries = &mut sent.entries;
    let deadline = session.now_ms().saturating_add(duration_ms(listen));
    loop {
        let now = session.now_ms();
        if now >= deadline {
            break;
        }
        let item = match session.next_item(Duration::from_millis(deadline - now)) {
            Ok(item) => item,
            Err(Error::Timeout { .. }) => break,
            Err(e) => {
                sent.error = Some(e);
                break;
            }
        };
        let at_ms = session.now_ms();
        entries.push(match item {
            ReplyItem::Line(text) => ConsoleEntry {
                at_ms,
                direction: Direction::Rx,
                text,
                block: None,
            },
            ReplyItem::Block(block) => ConsoleEntry {
                at_ms,
                direction: Direction::Rx,
                text: block_text(&block),
                block: Some(block),
            },
        });
    }
    Ok(sent)
}

/// A block's tag as the meter sends it (`pdo:`, `ufcs:`, ...).
fn block_text(block: &CapBlock) -> String {
    match block.tag() {
        BlockTag::Pdo => "pdo:",
        BlockTag::Ufcs => "ufcs:",
        BlockTag::Scp => "scp:",
        BlockTag::Vfcp => "vfcp:",
        BlockTag::Tfcp => "tfcp:",
    }
    .to_owned()
}

/// `d` in whole milliseconds, saturating.
fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// A parsed block in words, for a transcript:
/// `pdo: #1 fixed 5.00 V 3.00 A, #2 fixed 9.00 V 3.00 A, ...`.
pub fn describe_block(block: &CapBlock) -> String {
    let parts: Vec<String> = match block {
        CapBlock::Pdo(pdo) => pdo
            .offers()
            .into_iter()
            .filter_map(|(position, pdo)| super::pd::Offer::from_pdo(position, pdo))
            .map(|o| {
                let mut s = format!(
                    "#{} {} {}",
                    o.position,
                    o.kind.name(),
                    range_v(o.min_voltage_mv, o.max_voltage_mv)
                );
                if let Some(ma) = o.max_current_ma {
                    s.push_str(&format!(" {}", amps(ma)));
                }
                if let Some(mw) = o.max_power_mw {
                    s.push_str(&format!(" {}", watts(mw)));
                }
                s
            })
            .collect(),
        CapBlock::Ufcs(u) => u
            .entries
            .iter()
            .map(|c| {
                format!(
                    "#{} {} {}",
                    c.index,
                    range_v(c.min_voltage_mv, c.max_voltage_mv),
                    range_a(c.min_current_ma, c.max_current_ma)
                )
            })
            .collect(),
        CapBlock::Scp(c) => vec![
            range_v(c.min_voltage_mv, c.max_voltage_mv),
            range_a(c.min_current_ma, c.max_current_ma),
            format!("max {}", watts(c.max_power_mw)),
        ],
        CapBlock::Vfcp(c) => vec![
            format!("{} {}", volts(c.num1_voltage_mv), amps(c.num1_current_ma)),
            format!("{} {}", volts(c.num2_voltage_mv), amps(c.num2_current_ma)),
            format!("max {}", watts(c.max_power_mw)),
        ],
        CapBlock::Tfcp(c) => vec![
            range_v(c.min_voltage_mv, c.max_voltage_mv),
            range_a(c.min_current_ma, c.max_current_ma),
            format!("max {}", watts(c.max_power_mw)),
        ],
    };
    format!("{} {}", block_text(block), parts.join(", "))
}

fn volts(mv: u32) -> String {
    format!("{:.2} V", f64::from(mv) / 1000.0)
}

fn amps(ma: u32) -> String {
    format!("{:.2} A", f64::from(ma) / 1000.0)
}

fn watts(mw: u32) -> String {
    format!("{:.1} W", f64::from(mw) / 1000.0)
}

/// `5.00 V` for a fixed voltage, `3.30-21.00 V` for a range.
fn range_v(min_mv: u32, max_mv: u32) -> String {
    if min_mv == max_mv {
        volts(min_mv)
    } else {
        format!(
            "{:.2}-{:.2} V",
            f64::from(min_mv) / 1000.0,
            f64::from(max_mv) / 1000.0
        )
    }
}

fn range_a(min_ma: u32, max_ma: u32) -> String {
    format!(
        "{:.2}-{:.2} A",
        f64::from(min_ma) / 1000.0,
        f64::from(max_ma) / 1000.0
    )
}
