//! Field trees of the small data objects (USB PD r3.2 §6.4: BDO, BSDO, ADO,
//! CCDO, EUDO, EPRMDO, SIDO, RMDO) and the extended-message data blocks
//! (§6.5: SCEDB, SDB, GBCDB, GBSDB, BCDB, MIDB, PPSSDB, SKEDB, ECDB).
//!
//! Data-object fields count bits inside the 32-bit object; data-block fields
//! count from bit 0 of the block's first byte. Block fields past the end of a
//! short block are left out.

use crate::extended::{EcdbType, EprAction, EprMode, ExtendedControl};
use crate::field::{Field, Flag};
use crate::field::{bit, bits, flag, num, reserved, volts};

/// Data message codes with a typed data object.
const BIST: u8 = 0x03;
const BATTERY_STATUS: u8 = 0x05;
const ALERT: u8 = 0x06;
const GET_COUNTRY_INFO: u8 = 0x07;
const ENTER_USB: u8 = 0x08;
pub(crate) const EPR_MODE: u8 = 0x0A;
const SOURCE_INFO: u8 = 0x0B;
const REVISION: u8 = 0x0C;

/// Extended message codes with a typed data block.
const SOURCE_CAPABILITIES_EXTENDED: u8 = 0x01;
const STATUS: u8 = 0x02;
const GET_BATTERY_CAP: u8 = 0x03;
const GET_BATTERY_STATUS: u8 = 0x04;
const BATTERY_CAPABILITIES: u8 = 0x05;
const MANUFACTURER_INFO: u8 = 0x07;
const PPS_STATUS: u8 = 0x0C;
const SINK_CAPABILITIES_EXTENDED: u8 = 0x0F;
pub(crate) const EXTENDED_CONTROL: u8 = 0x10;

/// A whole-object node named `name` with `children`.
fn object(name: &str, dobj: u32, children: Vec<Field>) -> Field {
    Field::new(name, 31, 0, u64::from(dobj), format!("{dobj:#010X}")).with_children(children)
}

/// Battery capacity in 0.1 Wh steps (0xFFFF: unknown).
fn capacity(v: u32) -> String {
    match v {
        0xFFFF => "Unknown".to_string(),
        v => format!("{}.{} Wh", v / 10, v % 10),
    }
}

fn charging_status(v: u32) -> &'static str {
    match v {
        0 => "Charging",
        1 => "Discharging",
        2 => "Idle",
        _ => "Reserved",
    }
}

/// The field tree of a data message's first data object, for the data
/// messages with a typed object (`None` for the others).
pub(crate) fn data_object_field(code: u8, dobj: u32) -> Option<Field> {
    Some(match code {
        BIST => {
            let mode = bits(dobj, 31, 28);
            let label = match mode {
                5 => Some("BIST Carrier Mode"),
                8 => Some("BIST Test Data"),
                9 => Some("BIST Shared Test Mode Entry"),
                10 => Some("BIST Shared Test Mode Exit"),
                _ => None,
            };
            object(
                "BDO",
                dobj,
                vec![
                    Field::new(
                        "BIST Mode",
                        31,
                        28,
                        u64::from(mode),
                        label.unwrap_or("Reserved"),
                    )
                    .flagged(label.is_none().then_some(Flag::Reserved)),
                    reserved(dobj, 27, 0),
                ],
            )
        }
        BATTERY_STATUS => {
            let invalid = bit(dobj, 8);
            let info = bits(dobj, 15, 8);
            object(
                "BSDO",
                dobj,
                vec![
                    num("Battery Present Capacity", dobj, 31, 16, capacity),
                    Field::new(
                        "Battery Info",
                        15,
                        8,
                        u64::from(info),
                        format!("{info:#04X}"),
                    )
                    .with_children(vec![
                        reserved(dobj, 15, 12),
                        num("Battery Charging Status", dobj, 11, 10, |v| {
                            charging_status(v).to_string()
                        }),
                        flag("Battery Present", dobj, 9),
                        flag("Invalid Battery Reference", dobj, 8)
                            .flagged(invalid.then_some(Flag::Invalid)),
                    ]),
                    reserved(dobj, 7, 0),
                ],
            )
        }
        ALERT => object(
            "ADO",
            dobj,
            vec![
                num("Type of Alert", dobj, 31, 24, |v| {
                    let names = [
                        (1, "Battery Status Change"),
                        (2, "OCP"),
                        (3, "OTP"),
                        (4, "Operating Condition Change"),
                        (5, "Source Input Change"),
                        (6, "OVP"),
                        (7, "Extended Alert Event"),
                    ];
                    set_bits(v, &names)
                }),
                num("Fixed Batteries", dobj, 23, 20, |v| format!("{v:#X}")),
                num("Hot Swappable Batteries", dobj, 19, 16, |v| {
                    format!("{v:#X}")
                }),
                reserved(dobj, 15, 4),
                num("Extended Alert Event Type", dobj, 3, 0, |v| {
                    match v {
                        0 => "None",
                        1 => "Power state change",
                        2 => "Power button press",
                        3 => "Power button release",
                        4 => "Controller initiated wake",
                        _ => "Reserved",
                    }
                    .to_string()
                }),
            ],
        ),
        GET_COUNTRY_INFO => object(
            "CCDO",
            dobj,
            vec![
                num("Country Code", dobj, 31, 16, |v| {
                    [(v >> 8) as u8, v as u8]
                        .iter()
                        // ISO 3166 letters; anything else shows as a dot.
                        .map(|&b| {
                            if b.is_ascii_graphic() {
                                char::from(b)
                            } else {
                                '.'
                            }
                        })
                        .collect()
                }),
                reserved(dobj, 15, 0),
            ],
        ),
        ENTER_USB => {
            let mode = bits(dobj, 30, 28);
            let label = match mode {
                0 => Some("USB 2.0"),
                1 => Some("USB 3.2"),
                2 => Some("USB4"),
                _ => None,
            };
            object(
                "EUDO",
                dobj,
                vec![
                    reserved(dobj, 31, 31),
                    Field::new(
                        "USB Mode",
                        30,
                        28,
                        u64::from(mode),
                        label.unwrap_or("Reserved"),
                    )
                    .flagged(label.is_none().then_some(Flag::Reserved)),
                    reserved(dobj, 27, 27),
                    flag("USB4 DRD", dobj, 26),
                    flag("USB3 DRD", dobj, 25),
                    reserved(dobj, 24, 24),
                    num("Cable Speed", dobj, 23, 21, |v| v.to_string()),
                    num("Cable Type", dobj, 20, 19, |v| v.to_string()),
                    num("Cable Current", dobj, 18, 17, |v| v.to_string()),
                    flag("PCIe Support", dobj, 16),
                    flag("DP Support", dobj, 15),
                    flag("TBT Support", dobj, 14),
                    flag("Host Present", dobj, 13),
                    reserved(dobj, 12, 0),
                ],
            )
        }
        EPR_MODE => {
            let mode = EprMode::decode(dobj);
            let data = u32::from(mode.data);
            let data_meaning = match mode.action {
                EprAction::Enter => format!("Sink Operational PDP {data} W"),
                EprAction::EnterFailed => match data {
                    0 => "Unknown cause",
                    1 => "Cable not EPR capable",
                    2 => "Source failed to become VCONN Source",
                    3 => "EPR Capable bit not set in RDO",
                    4 => "Source unable to enter EPR Mode",
                    5 => "EPR Capable bit not set in PDO",
                    _ => "Reserved",
                }
                .to_string(),
                _ => data.to_string(),
            };
            object(
                "EPRMDO",
                dobj,
                vec![
                    Field::new(
                        "Action",
                        31,
                        24,
                        u64::from(bits(dobj, 31, 24)),
                        mode.action.name(),
                    )
                    .flagged(
                        matches!(mode.action, EprAction::Reserved(_)).then_some(Flag::Reserved),
                    ),
                    Field::new("Data", 23, 16, u64::from(data), data_meaning),
                    reserved(dobj, 15, 0),
                ],
            )
        }
        SOURCE_INFO => object(
            "SIDO",
            dobj,
            vec![
                num("Port Type", dobj, 31, 31, |v| {
                    if v == 1 {
                        "Guaranteed Capability Port"
                    } else {
                        "Managed Capability Port"
                    }
                    .to_string()
                }),
                reserved(dobj, 30, 24),
                num("Port Maximum PDP", dobj, 23, 16, |v| format!("{v} W")),
                num("Port Present PDP", dobj, 15, 8, |v| format!("{v} W")),
                num("Port Reported PDP", dobj, 7, 0, |v| format!("{v} W")),
            ],
        ),
        REVISION => object(
            "RMDO",
            dobj,
            vec![
                num("Revision Major", dobj, 31, 28, |v| v.to_string()),
                num("Revision Minor", dobj, 27, 24, |v| v.to_string()),
                num("Version Major", dobj, 23, 20, |v| v.to_string()),
                num("Version Minor", dobj, 19, 16, |v| v.to_string()),
                reserved(dobj, 15, 0),
            ],
        ),
        _ => return None,
    })
}

/// Summary tail of a data message with a typed object (`None` otherwise).
pub(crate) fn data_summary(code: u8, dobj: u32) -> Option<String> {
    match code {
        EPR_MODE => Some(EprMode::decode(dobj).action.name().to_string()),
        REVISION => Some(format!(
            "{}.{} v{}.{}",
            bits(dobj, 31, 28),
            bits(dobj, 27, 24),
            bits(dobj, 23, 20),
            bits(dobj, 19, 16)
        )),
        _ => None,
    }
}

/// Names of the set bits of `v` from `names` (`(bit, name)`), or "None".
fn set_bits(v: u32, names: &[(u32, &str)]) -> String {
    let set: Vec<&str> = names
        .iter()
        .filter(|(n, _)| v >> n & 1 == 1)
        .map(|(_, name)| *name)
        .collect();
    if set.is_empty() {
        "None".to_string()
    } else {
        set.join(", ")
    }
}

/// Little-endian value of `len` bytes at `off`, if the block has them.
fn le(data: &[u8], off: usize, len: usize) -> Option<u64> {
    let bytes = data.get(off..off + len)?;
    Some(
        bytes
            .iter()
            .rev()
            .fold(0u64, |acc, &b| acc << 8 | u64::from(b)),
    )
}

/// A block field of `len` bytes at byte `off`, meaning from `show`.
fn bf(
    data: &[u8],
    name: &str,
    off: usize,
    len: usize,
    show: impl Fn(u64) -> String,
) -> Option<Field> {
    let v = le(data, off, len)?;
    let lo = (off * 8) as u16;
    let hi = ((off + len) * 8 - 1) as u16;
    Some(Field::new(name, hi, lo, v, show(v)))
}

fn hex(v: u64) -> String {
    format!("{v:#06X}")
}

fn watts(v: u64) -> String {
    format!("{v} W")
}

fn plain(v: u64) -> String {
    v.to_string()
}

/// The whole-block node named `name`.
fn block(name: &str, data: &[u8], children: Vec<Option<Field>>) -> Field {
    let hi = (data.len() * 8).saturating_sub(1) as u16;
    let shown: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
    Field::new(name, hi, 0, 0, shown.join(" "))
        .with_children(children.into_iter().flatten().collect())
}

/// A battery reference byte: 0..3 fixed batteries, 4..7 hot-swappable.
fn battery_ref(data: &[u8], name: &str) -> Option<Field> {
    let v = le(data, 0, 1)?;
    let (meaning, flag) = match v {
        0..=3 => (format!("Fixed battery {v}"), None),
        4..=7 => (format!("Hot-swappable battery {}", v - 4), None),
        _ => ("Invalid".to_string(), Some(Flag::Invalid)),
    };
    Some(Field::new(name, 7, 0, v, meaning).flagged(flag))
}

fn manufacturer_string(data: &[u8]) -> String {
    let s = data.get(4..).unwrap_or_default();
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    // Control characters would break the one-line summary.
    String::from_utf8_lossy(&s[..end])
        .chars()
        .map(|c| if c.is_control() { '.' } else { c })
        .collect()
}

/// The field tree of an extended message's data block, for the extended
/// messages with a typed block (`None` for the others).
pub(crate) fn data_block_field(code: u8, data: &[u8]) -> Option<Field> {
    Some(match code {
        SOURCE_CAPABILITIES_EXTENDED => block(
            "SCEDB",
            data,
            vec![
                bf(data, "VID", 0, 2, hex),
                bf(data, "PID", 2, 2, hex),
                bf(data, "XID", 4, 4, |v| format!("{v:#010X}")),
                bf(data, "FW Version", 8, 1, plain),
                bf(data, "HW Version", 9, 1, plain),
                bf(data, "Voltage Regulation", 10, 1, hex),
                bf(data, "Holdup Time", 11, 1, |v| format!("{v} ms")),
                bf(data, "Compliance", 12, 1, hex),
                bf(data, "Touch Current", 13, 1, hex),
                bf(data, "Peak Current1", 14, 2, hex),
                bf(data, "Peak Current2", 16, 2, hex),
                bf(data, "Peak Current3", 18, 2, hex),
                bf(data, "Touch Temp", 20, 1, plain),
                bf(data, "Source Inputs", 21, 1, hex),
                bf(data, "Number of Batteries/Battery Slots", 22, 1, hex),
                bf(data, "SPR Source PDP Rating", 23, 1, watts),
                bf(data, "EPR Source PDP Rating", 24, 1, watts),
            ],
        ),
        STATUS => block(
            "SDB",
            data,
            vec![
                bf(data, "Internal Temp", 0, 1, |v| match v {
                    0 => "Not supported".to_string(),
                    1 => "Less than 2 °C".to_string(),
                    v => format!("{v} °C"),
                }),
                bf(data, "Present Input", 1, 1, hex),
                bf(data, "Present Battery Input", 2, 1, hex),
                bf(data, "Event Flags", 3, 1, |v| {
                    set_bits(
                        v as u32,
                        &[(1, "OCP"), (2, "OTP"), (3, "OVP"), (4, "CF mode")],
                    )
                }),
                bf(data, "Temperature Status", 4, 1, |v| {
                    match (v >> 1) & 3 {
                        0 => "Not supported",
                        1 => "Normal",
                        2 => "Warning",
                        _ => "Over temperature",
                    }
                    .to_string()
                }),
                bf(data, "Power Status", 5, 1, hex),
                bf(data, "Power State Change", 6, 1, hex),
            ],
        ),
        GET_BATTERY_CAP => block("GBCDB", data, vec![battery_ref(data, "Battery Cap Ref")]),
        GET_BATTERY_STATUS => block("GBSDB", data, vec![battery_ref(data, "Battery Status Ref")]),
        BATTERY_CAPABILITIES => {
            let invalid = le(data, 8, 1).map(|v| v & 1);
            block(
                "BCDB",
                data,
                vec![
                    bf(data, "VID", 0, 2, hex),
                    bf(data, "PID", 2, 2, hex),
                    bf(data, "Battery Design Capacity", 4, 2, |v| {
                        capacity(v as u32)
                    }),
                    bf(data, "Battery Last Full Charge Capacity", 6, 2, |v| {
                        capacity(v as u32)
                    }),
                    invalid.map(|v| {
                        Field::new(
                            "Invalid Battery Reference",
                            64,
                            64,
                            v,
                            if v == 1 { "Invalid" } else { "Valid" },
                        )
                        .flagged((v == 1).then_some(Flag::Invalid))
                    }),
                ],
            )
        }
        MANUFACTURER_INFO => {
            let s = manufacturer_string(data);
            let end = data.len().max(4);
            block(
                "MIDB",
                data,
                vec![
                    bf(data, "Manufacturer VID", 0, 2, hex),
                    bf(data, "PID", 2, 2, hex),
                    (data.len() > 4)
                        .then(|| Field::new("Manufacturer String", (end * 8 - 1) as u16, 32, 0, s)),
                ],
            )
        }
        PPS_STATUS => block(
            "PPSSDB",
            data,
            vec![
                bf(data, "Output Voltage", 0, 2, |v| match v {
                    0xFFFF => "Not supported".to_string(),
                    v => volts(v as u32 * 20),
                }),
                bf(data, "Output Current", 2, 1, |v| match v {
                    0xFF => "Not supported".to_string(),
                    v => crate::field::amps(v as u32 * 50),
                }),
                bf(data, "Real Time Flags", 3, 1, |v| {
                    let ptf = match (v >> 1) & 3 {
                        0 => "PTF: Not supported",
                        1 => "PTF: Normal",
                        2 => "PTF: Warning",
                        _ => "PTF: Over temperature",
                    };
                    let omf = if v >> 3 & 1 == 1 {
                        "Current Limit mode"
                    } else {
                        "Constant Voltage mode"
                    };
                    format!("{ptf}, {omf}")
                }),
            ],
        ),
        SINK_CAPABILITIES_EXTENDED => block(
            "SKEDB",
            data,
            vec![
                bf(data, "VID", 0, 2, hex),
                bf(data, "PID", 2, 2, hex),
                bf(data, "XID", 4, 4, |v| format!("{v:#010X}")),
                bf(data, "FW Version", 8, 1, plain),
                bf(data, "HW Version", 9, 1, plain),
                bf(data, "SKEDB Version", 10, 1, plain),
                bf(data, "Load Step", 11, 1, hex),
                bf(data, "Sink Load Characteristics", 12, 2, hex),
                bf(data, "Compliance", 14, 1, hex),
                bf(data, "Touch Temp", 15, 1, plain),
                bf(data, "Battery Info", 16, 1, hex),
                bf(data, "Sink Modes", 17, 1, hex),
                bf(data, "SPR Sink Minimum PDP", 18, 1, watts),
                bf(data, "SPR Sink Operational PDP", 19, 1, watts),
                bf(data, "SPR Sink Maximum PDP", 20, 1, watts),
                bf(data, "EPR Sink Minimum PDP", 21, 1, watts),
                bf(data, "EPR Sink Operational PDP", 22, 1, watts),
                bf(data, "EPR Sink Maximum PDP", 23, 1, watts),
            ],
        ),
        EXTENDED_CONTROL => {
            let ty = le(data, 0, 1);
            let kind = ty.map(|v| ExtendedControl::kind_of(v as u8));
            block(
                "ECDB",
                data,
                vec![
                    kind.zip(ty).map(|(k, v)| {
                        Field::new("Type", 7, 0, v, k.name())
                            .flagged(matches!(k, EcdbType::Reserved(_)).then_some(Flag::Reserved))
                    }),
                    bf(data, "Data", 1, 1, plain),
                ],
            )
        }
        _ => return None,
    })
}

/// Summary tail of an extended message with a typed block (`None`
/// otherwise).
pub(crate) fn block_summary(code: u8, data: &[u8]) -> Option<String> {
    match code {
        MANUFACTURER_INFO => Some(manufacturer_string(data)),
        EXTENDED_CONTROL => ExtendedControl::decode(data).map(|c| c.kind.name().to_string()),
        _ => None,
    }
}
