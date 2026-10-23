//! Charger protocol detection on top of the PDM session (FR DGSQ6R).
//!
//! - [`bc12`], [`apple`]: `entry bc` / `entry apple`, the reply mapped to a
//!   [`Support`]. Doc: `true` / `false` \[doc\]; seen on the device:
//!   `entry bc` → only `ok` (→ [`Support::Unknown`]), `entry apple` →
//!   `fail` on a non-Apple charger (→ [`Support::NotSupported`]).
//!   \[device\] (session 2)
//! - [`list`]: `entry list` (or `entry list+`), each `<name>:<result>
//!   [<mV>mV,<mA>mA]` line reported as it arrives; a stall longer than
//!   [`ListOptions::stall_limit`] ends with `reset` and an incomplete result.
//!   The end-of-list form is unknown. \[inferred\]
//! - [`hint`]: a pure D+ / D- lookup of likely charging modes (a hint only).
//!   Nominal levels from the vendor manual \[doc\]; the ±200 mV windows
//!   (inclusive) are ours. \[inferred\]

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::Error;
use super::command::{Command, Protocol};
use super::reply::ReplyItem;
use super::session::{PdmSession, REPLY_TIMEOUT};
use crate::session::Clock;

/// Default stall limit of [`list`]: the device was seen silent for 40-45 s
/// after `vfcp:null`. \[device\]
pub const DEFAULT_STALL_LIMIT: Duration = Duration::from_secs(45);

/// How long [`bc12`] / [`apple`] wait for a deciding line: on the device
/// `entry bc` sent only `ok`, then nothing within 8 s. \[device\]
pub const SINGLE_REPLY_TIMEOUT: Duration = Duration::from_secs(8);

/// What a detection says about one protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// The charger supports it.
    Supported,
    /// The charger does not support it.
    NotSupported,
    /// The reply did not say.
    Unknown,
}

impl Support {
    /// Machine name: `supported`, `not-supported` or `unknown`.
    pub fn name(self) -> &'static str {
        match self {
            Support::Supported => "supported",
            Support::NotSupported => "not-supported",
            Support::Unknown => "unknown",
        }
    }
}

/// Map one reply line of `entry bc` / `entry apple` to a result; `None`
/// for lines that do not decide (e.g. `ok`).
pub fn single_reply(line: &str) -> Option<Support> {
    match line.trim() {
        "true" => Some(Support::Supported),
        "false" | "fail" => Some(Support::NotSupported),
        _ => None,
    }
}

/// Lines that can decide a single-reply detection.
const DECIDING: [&str; 3] = ["true", "false", "fail"];

/// Send `entry <protocol>` and wait up to [`SINGLE_REPLY_TIMEOUT`] for a
/// deciding line; no deciding line → [`Support::Unknown`].
fn single<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    protocol: Protocol,
) -> Result<Support, Error> {
    session.send(&Command::Entry(protocol))?;
    match session.wait_for(&DECIDING, SINGLE_REPLY_TIMEOUT) {
        // A prefix-only match (`trueish`) decides nothing.
        Ok(line) => Ok(single_reply(&line).unwrap_or(Support::Unknown)),
        Err(Error::Timeout { .. }) => Ok(Support::Unknown),
        Err(e) => Err(e),
    }
}

/// `entry bc`: BC1.2 detection.
pub fn bc12<P: Read + Write, C: Clock>(session: &mut PdmSession<P, C>) -> Result<Support, Error> {
    single(session, Protocol::Bc)
}

/// `entry apple`: Apple detection.
pub fn apple<P: Read + Write, C: Clock>(session: &mut PdmSession<P, C>) -> Result<Support, Error> {
    single(session, Protocol::Apple)
}

/// One line of the automatic list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    /// Protocol name as the device prints it (`bc1.2`, `ufcs`, ...).
    pub name: String,
    /// `true` → supported, `null` / `false` → not supported, else unknown.
    pub support: Support,
    /// VBUS reported with the line, mV.
    pub vbus_mv: Option<u32>,
    /// IBUS reported with the line, mA.
    pub ibus_ma: Option<u32>,
}

/// Parse one `<name>:<result>[ <mV>mV,<mA>mA]` line; `None` for anything
/// else (`starting`, `ok`, empty lines).
pub fn parse_list_line(line: &str) -> Option<ListEntry> {
    let (name, rest) = line.trim().split_once(':')?;
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let support = match parts.next()? {
        "true" => Support::Supported,
        "null" | "false" => Support::NotSupported,
        _ => Support::Unknown,
    };
    let (vbus_mv, ibus_ma) = parts
        .next()
        .and_then(parse_readings)
        .map_or((None, None), |(v, i)| (Some(v), Some(i)));
    Some(ListEntry {
        name: name.to_string(),
        support,
        vbus_mv,
        ibus_ma,
    })
}

/// `<mV>mV,<mA>mA` → (mV, mA); `None` unless both parse.
fn parse_readings(text: &str) -> Option<(u32, u32)> {
    let (v, i) = text.split_once(',')?;
    let v = v.strip_suffix("mV")?.parse().ok()?;
    let i = i.strip_suffix("mA")?.parse().ok()?;
    Some((v, i))
}

/// Options of [`list`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListOptions {
    /// Send `entry list+` (adds VOOC / MTK PE; needs a dummy load) instead
    /// of `entry list`.
    pub plus: bool,
    /// Silence longer than this ends the list with `reset`.
    pub stall_limit: Duration,
}

impl Default for ListOptions {
    fn default() -> Self {
        ListOptions {
            plus: false,
            stall_limit: DEFAULT_STALL_LIMIT,
        }
    }
}

/// Result of [`list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListResult {
    /// Every entry, in arrival order.
    pub entries: Vec<ListEntry>,
    /// `false` when the list ended in a stall, a cut-off or a stop (and
    /// `reset`). Always `false` for now: how a full list ends is not known
    /// yet, so nothing can mark it complete. \[inferred\]
    pub complete: bool,
}

/// Most entries one automatic list reports before it is cut off.
pub const MAX_LIST_ENTRIES: usize = 64;
/// Most reply items (entries, other lines, blocks) one list reads.
pub const MAX_LIST_ITEMS: usize = 256;

/// How often a waiting list checks its stop flag.
const STOP_POLL: Duration = Duration::from_millis(250);

/// `entry list` / `entry list+`; `on_entry` is called for each entry as
/// it arrives.
pub fn list<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    options: &ListOptions,
    on_entry: &mut dyn FnMut(&ListEntry),
) -> Result<ListResult, Error> {
    list_until(session, options, on_entry, &AtomicBool::new(false))
}

/// [`list`] that also ends (with `reset`, as incomplete) soon after `stop`
/// is set, e.g. by Ctrl-C, instead of waiting out the stall limit.
pub fn list_until<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    options: &ListOptions,
    on_entry: &mut dyn FnMut(&ListEntry),
    stop: &AtomicBool,
) -> Result<ListResult, Error> {
    let protocol = if options.plus {
        Protocol::ListPlus
    } else {
        Protocol::List
    };
    session.send(&Command::Entry(protocol))?;
    let mut entries = Vec::new();
    // A meter that never stops talking cannot hold the list open: after
    // MAX_LIST_ENTRIES entries (or MAX_LIST_ITEMS items of any kind) it is
    // ended with `reset` like a stall.
    'items: for _ in 0..MAX_LIST_ITEMS {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        // Every item (line or block) restarts the stall timer; the wait is
        // sliced so a stop request is seen within STOP_POLL.
        let mut silent = Duration::ZERO;
        let item = loop {
            if stop.load(Ordering::SeqCst) || silent >= options.stall_limit {
                break 'items;
            }
            let slice = STOP_POLL.min(options.stall_limit - silent);
            match session.next_item(slice) {
                Ok(item) => break item,
                Err(Error::Timeout { .. }) => silent += slice,
                Err(e) => return Err(e),
            }
        };
        if let ReplyItem::Line(line) = item
            && let Some(entry) = parse_list_line(&line)
        {
            on_entry(&entry);
            entries.push(entry);
        }
    }
    // Stalled (or cut off): the end-of-list form is unknown, so `reset`
    // ends it.
    session.send(&Command::Reset)?;
    // Keep the partial result even if `reset` gets no `ok`: it is marked
    // incomplete anyway, and the caller closes the session either way.
    match session.wait_for(&["ok"], REPLY_TIMEOUT) {
        Ok(_) | Err(Error::Timeout { .. }) => {}
        Err(e) => return Err(e),
    }
    Ok(ListResult {
        entries,
        complete: false,
    })
}

/// Likely charging modes for a D+ / D- pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeMode {
    /// Apple 2.4 A (D+ 2.7 V, D- 2.7 V).
    Apple2400mA,
    /// Apple 2.1 A (D+ 2.7 V, D- 2.0 V).
    Apple2100mA,
    /// Apple 1 A (D+ 2.0 V, D- 2.7 V).
    Apple1000mA,
    /// Samsung 2 A (D+ 1.2 V, D- 1.2 V).
    Samsung2000mA,
    /// BC1.2 dedicated charging port (D+ and D- shorted).
    Dcp,
    /// Standard downstream port, or nothing (D+ and D- near 0 V).
    Sdp,
    /// QC2 5 V / FCP / AFC (D+ 0.6 V, D- 0 V).
    Qc2At5V,
    /// QC2 9 V (D+ 3.3 V, D- 0.6 V).
    Qc2At9V,
    /// QC2 12 V (D+ 0.6 V, D- 0.6 V).
    Qc2At12V,
    /// QC3.0 (D+ 0.6 V, D- 3.3 V).
    Qc3,
    /// USB 2.0 full speed (D+ 3.3 V, D- 0 V).
    Usb2FullSpeed,
}

impl ChargeMode {
    /// Display name (`Apple 2.4 A`, `Samsung 2 A`, `DCP`, ...).
    pub fn name(self) -> &'static str {
        match self {
            ChargeMode::Apple2400mA => "Apple 2.4 A",
            ChargeMode::Apple2100mA => "Apple 2.1 A",
            ChargeMode::Apple1000mA => "Apple 1 A",
            ChargeMode::Samsung2000mA => "Samsung 2 A",
            ChargeMode::Dcp => "DCP",
            ChargeMode::Sdp => "SDP 0.5 A",
            ChargeMode::Qc2At5V => "QC2 5 V / FCP / AFC",
            ChargeMode::Qc2At9V => "QC2 9 V",
            ChargeMode::Qc2At12V => "QC2 12 V",
            ChargeMode::Qc3 => "QC3.0",
            ChargeMode::Usb2FullSpeed => "USB 2.0 full speed",
        }
    }
}

/// The D+ / D- hint: likely modes, never a detection result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// D+, mV.
    pub dp_mv: u32,
    /// D-, mV.
    pub dm_mv: u32,
    /// Likely modes (empty when nothing matches).
    pub modes: Vec<ChargeMode>,
}

impl Hint {
    /// One line labelled as a hint, e.g. `hint: Apple 2.4 A (D+ 2.700 V, D- 2.700 V)`.
    pub fn label(&self) -> String {
        let modes = if self.modes.is_empty() {
            "no known mode".to_string()
        } else {
            self.modes
                .iter()
                .map(|m| m.name())
                .collect::<Vec<_>>()
                .join(" / ")
        };
        format!(
            "hint: {modes} (D+ {}.{:03} V, D- {}.{:03} V)",
            self.dp_mv / 1000,
            self.dp_mv % 1000,
            self.dm_mv / 1000,
            self.dm_mv % 1000
        )
    }
}

/// Likely charging modes for D+ = `dp_mv`, D- = `dm_mv`. Pure.
pub fn hint(dp_mv: u32, dm_mv: u32) -> Hint {
    let modes = HINT_TABLE
        .iter()
        .filter(|(dp, dm, _)| near(dp_mv, *dp) && near(dm_mv, *dm))
        .map(|(_, _, mode)| *mode)
        .collect();
    Hint {
        dp_mv,
        dm_mv,
        modes,
    }
}

/// Half-width of a level window, mV (inclusive). \[inferred\]
const HINT_WINDOW_MV: u32 = 200;

/// `value` within [`HINT_WINDOW_MV`] of the nominal `level`.
fn near(value: u32, level: u32) -> bool {
    value.abs_diff(level) <= HINT_WINDOW_MV
}

/// Nominal (D+ mV, D- mV) → mode, in display order.
const HINT_TABLE: [(u32, u32, ChargeMode); 12] = [
    (2700, 2700, ChargeMode::Apple2400mA),   // [doc]
    (2700, 2000, ChargeMode::Apple2100mA),   // [inferred] widely published
    (2000, 2700, ChargeMode::Apple1000mA),   // [inferred] widely published
    (1200, 1200, ChargeMode::Samsung2000mA), // [doc]
    (600, 0, ChargeMode::Qc2At5V),           // [doc] QC2 5 V / FCP / AFC 9 V
    (600, 0, ChargeMode::Dcp),               // [doc] DCP 1.5 A shares this row
    (600, 600, ChargeMode::Dcp),             // [inferred] D+ / D- shorted
    (600, 600, ChargeMode::Qc2At12V),        // [doc]
    (0, 0, ChargeMode::Sdp),                 // [doc] SDP 0.5 A
    (3300, 0, ChargeMode::Usb2FullSpeed),    // [doc]
    (600, 3300, ChargeMode::Qc3),            // [doc]
    (3300, 600, ChargeMode::Qc2At9V),        // [doc]
];

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(dp: u32, dm: u32) -> Vec<ChargeMode> {
        hint(dp, dm).modes
    }

    // ---- AC-DGSQ6R.1: reply forms of entry bc / entry apple ----

    #[test]
    fn doc_reply_forms_map_to_supported_and_not_supported() {
        assert_eq!(single_reply("true"), Some(Support::Supported));
        assert_eq!(single_reply("false"), Some(Support::NotSupported));
    }

    #[test]
    fn session_2_fail_means_not_supported_and_ok_decides_nothing() {
        assert_eq!(single_reply("fail"), Some(Support::NotSupported));
        assert_eq!(single_reply("ok"), None);
        assert_eq!(single_reply(""), None);
        assert_eq!(single_reply("cc1 attach"), None);
    }

    #[test]
    fn support_names() {
        assert_eq!(Support::Supported.name(), "supported");
        assert_eq!(Support::NotSupported.name(), "not-supported");
        assert_eq!(Support::Unknown.name(), "unknown");
    }

    // ---- AC-DGSQ6R.2: automatic list line format ----

    #[test]
    fn list_line_with_vbus_and_ibus() {
        assert_eq!(
            parse_list_line("bc1.2:true 5203mV,0mA"),
            Some(ListEntry {
                name: "bc1.2".into(),
                support: Support::Supported,
                vbus_mv: Some(5203),
                ibus_ma: Some(0),
            })
        );
    }

    #[test]
    fn list_line_null_is_not_supported() {
        assert_eq!(
            parse_list_line("ufcs:null"),
            Some(ListEntry {
                name: "ufcs".into(),
                support: Support::NotSupported,
                vbus_mv: None,
                ibus_ma: None,
            })
        );
        assert_eq!(
            parse_list_line("vfcp:null").map(|e| e.support),
            Some(Support::NotSupported)
        );
    }

    #[test]
    fn list_accepts_any_name_and_unknown_results() {
        let e = parse_list_line("mtkpe:true 9012mV,1500mA").expect("entry");
        assert_eq!(e.name, "mtkpe");
        assert_eq!(e.support, Support::Supported);
        assert_eq!((e.vbus_mv, e.ibus_ma), (Some(9012), Some(1500)));
        let e = parse_list_line("qc:false").expect("entry");
        assert_eq!(e.support, Support::NotSupported);
        let e = parse_list_line("vooc:maybe").expect("entry");
        assert_eq!(e.name, "vooc");
        assert_eq!(e.support, Support::Unknown);
    }

    #[test]
    fn list_ignores_non_entry_lines() {
        assert_eq!(parse_list_line("starting"), None);
        assert_eq!(parse_list_line("ok"), None);
        assert_eq!(parse_list_line(""), None);
        assert_eq!(parse_list_line(":true"), None);
    }

    #[test]
    fn list_line_with_malformed_readings_keeps_the_result() {
        let e = parse_list_line("scp:true garbage").expect("entry");
        assert_eq!(e.support, Support::Supported);
        assert_eq!((e.vbus_mv, e.ibus_ma), (None, None));
    }

    #[test]
    fn default_stall_limit_is_45_s() {
        assert_eq!(DEFAULT_STALL_LIMIT, Duration::from_secs(45));
        assert_eq!(
            ListOptions::default(),
            ListOptions {
                plus: false,
                stall_limit: Duration::from_secs(45)
            }
        );
    }

    // ---- AC-DGSQ6R.3: D+ / D- hint table ----

    #[test]
    fn nominal_points_of_the_table() {
        assert!(modes(2700, 2700).contains(&ChargeMode::Apple2400mA));
        assert!(modes(2700, 2000).contains(&ChargeMode::Apple2100mA));
        assert!(modes(2000, 2700).contains(&ChargeMode::Apple1000mA));
        assert!(modes(1200, 1200).contains(&ChargeMode::Samsung2000mA));
        assert!(modes(600, 600).contains(&ChargeMode::Dcp));
        assert!(modes(600, 600).contains(&ChargeMode::Qc2At12V));
        assert!(modes(0, 0).contains(&ChargeMode::Sdp));
        assert!(modes(600, 0).contains(&ChargeMode::Qc2At5V));
        assert!(modes(3300, 600).contains(&ChargeMode::Qc2At9V));
        assert!(modes(600, 3300).contains(&ChargeMode::Qc3));
        assert!(modes(3300, 0).contains(&ChargeMode::Usb2FullSpeed));
    }

    #[test]
    fn apple_and_samsung_points_are_exclusive() {
        assert_eq!(modes(2700, 2700), [ChargeMode::Apple2400mA]);
        assert_eq!(modes(2700, 2000), [ChargeMode::Apple2100mA]);
        assert_eq!(modes(2000, 2700), [ChargeMode::Apple1000mA]);
        assert_eq!(modes(1200, 1200), [ChargeMode::Samsung2000mA]);
    }

    #[test]
    fn window_is_200_mv_inclusive() {
        // Apple 2.4 A: 2500..=2900 on both lines.
        assert!(modes(2500, 2500).contains(&ChargeMode::Apple2400mA));
        assert!(modes(2900, 2900).contains(&ChargeMode::Apple2400mA));
        assert!(!modes(2499, 2700).contains(&ChargeMode::Apple2400mA));
        assert!(!modes(2700, 2901).contains(&ChargeMode::Apple2400mA));
        // Samsung 2 A: 1000..=1400.
        assert!(modes(1000, 1400).contains(&ChargeMode::Samsung2000mA));
        assert!(!modes(999, 1200).contains(&ChargeMode::Samsung2000mA));
        assert!(!modes(1200, 1401).contains(&ChargeMode::Samsung2000mA));
        // Apple 2.1 A: D- 1800..=2200.
        assert!(modes(2700, 1800).contains(&ChargeMode::Apple2100mA));
        assert!(modes(2700, 2200).contains(&ChargeMode::Apple2100mA));
        assert!(!modes(2700, 1799).contains(&ChargeMode::Apple2100mA));
        assert!(!modes(2700, 2201).contains(&ChargeMode::Apple2100mA));
        // 0 V: 0..=200.
        assert!(modes(200, 200).contains(&ChargeMode::Sdp));
        assert!(!modes(201, 0).contains(&ChargeMode::Sdp));
        // 0.6 V: 400..=800; 3.3 V: 3100..=3500.
        assert!(modes(400, 3500).contains(&ChargeMode::Qc3));
        assert!(modes(800, 3100).contains(&ChargeMode::Qc3));
        assert!(!modes(399, 3300).contains(&ChargeMode::Qc3));
        assert!(!modes(600, 3099).contains(&ChargeMode::Qc3));
    }

    #[test]
    fn between_levels_nothing_matches() {
        assert!(modes(1600, 1600).is_empty());
        assert!(modes(300, 300).is_empty());
        assert!(modes(5000, 5000).is_empty());
    }

    #[test]
    fn session_2_baseline_is_no_apple_or_samsung_match() {
        // PD-only charger idle: D+ 13 mV, D- 33 mV. [device]
        let m = modes(13, 33);
        for bad in [
            ChargeMode::Apple2400mA,
            ChargeMode::Apple2100mA,
            ChargeMode::Apple1000mA,
            ChargeMode::Samsung2000mA,
        ] {
            assert!(!m.contains(&bad), "{m:?}");
        }
        assert_eq!(m, [ChargeMode::Sdp]);
    }

    #[test]
    fn mode_names() {
        assert_eq!(ChargeMode::Apple2400mA.name(), "Apple 2.4 A");
        assert_eq!(ChargeMode::Apple2100mA.name(), "Apple 2.1 A");
        assert_eq!(ChargeMode::Apple1000mA.name(), "Apple 1 A");
        assert_eq!(ChargeMode::Samsung2000mA.name(), "Samsung 2 A");
        assert_eq!(ChargeMode::Dcp.name(), "DCP");
    }

    #[test]
    fn label_says_hint_and_names_the_modes() {
        let h = hint(2700, 2700);
        assert_eq!((h.dp_mv, h.dm_mv), (2700, 2700));
        let label = h.label();
        assert!(label.starts_with("hint"), "{label}");
        assert!(label.contains("Apple 2.4 A"), "{label}");
        let none = hint(1600, 1600).label();
        assert!(none.starts_with("hint"), "{none}");
    }
}
