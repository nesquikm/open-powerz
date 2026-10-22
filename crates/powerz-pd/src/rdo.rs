//! Request data objects (USB PD r3.2 §6.4.2).

use crate::capture::Issue;
use crate::field::{Field, Flag};
use crate::field::{amps, bit, bits, flag, num, reserved, volts, watts};
use crate::pdo::SourcePdo;

/// Fixed or variable supply RDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedRdo {
    /// The 32 RDO bits.
    pub raw: u32,
    /// Object Position (bits 31..28).
    pub position: u8,
    /// GiveBack (bit 27).
    pub give_back: bool,
    /// Capability Mismatch (bit 26).
    pub capability_mismatch: bool,
    /// USB Communications Capable (bit 25).
    pub usb_communications_capable: bool,
    /// No USB Suspend (bit 24).
    pub no_usb_suspend: bool,
    /// Unchunked Extended Messages Supported (bit 23).
    pub unchunked_extended_messages: bool,
    /// EPR Capable (bit 22).
    pub epr_capable: bool,
    /// Operating Current, mA (bits 19..10, 10 mA units).
    pub operating_current_ma: u32,
    /// Maximum (or minimum, with GiveBack) Operating Current, mA (bits 9..0).
    pub max_current_ma: u32,
}

/// Battery RDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryRdo {
    /// The 32 RDO bits.
    pub raw: u32,
    /// Object Position (bits 31..28).
    pub position: u8,
    /// GiveBack (bit 27).
    pub give_back: bool,
    /// Capability Mismatch (bit 26).
    pub capability_mismatch: bool,
    /// Operating Power, mW (bits 19..10, 250 mW units).
    pub operating_power_mw: u32,
    /// Maximum Operating Power, mW (bits 9..0, 250 mW units).
    pub max_power_mw: u32,
}

/// PPS RDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpsRdo {
    /// The 32 RDO bits.
    pub raw: u32,
    /// Object Position (bits 31..28).
    pub position: u8,
    /// Capability Mismatch (bit 26).
    pub capability_mismatch: bool,
    /// Output Voltage, mV (bits 20..9, 20 mV units).
    pub voltage_mv: u32,
    /// Operating Current, mA (bits 6..0, 50 mA units).
    pub current_ma: u32,
}

/// AVS RDO (SPR or EPR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvsRdo {
    /// The 32 RDO bits.
    pub raw: u32,
    /// Object Position (bits 31..28).
    pub position: u8,
    /// Capability Mismatch (bit 26).
    pub capability_mismatch: bool,
    /// Output Voltage, mV (bits 20..9, 25 mV units).
    pub voltage_mv: u32,
    /// Operating Current, mA (bits 6..0, 50 mA units).
    pub current_ma: u32,
}

/// A request data object, typed by the PDO it points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rdo {
    /// Against a fixed supply PDO.
    Fixed(FixedRdo),
    /// Against a variable supply PDO (fixed layout).
    Variable(FixedRdo),
    /// Against a battery PDO.
    Battery(BatteryRdo),
    /// Against a PPS APDO.
    Pps(PpsRdo),
    /// Against an SPR or EPR AVS APDO.
    Avs(AvsRdo),
    /// The PDO it points at is unknown: no caps, or a bad position.
    Unresolved {
        /// Object Position (bits 31..28).
        position: u8,
        /// The 32 RDO bits.
        raw: u32,
    },
}

/// A Request or EPR_Request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// The request data object.
    pub rdo: Rdo,
    /// The PDO it requests: from the remembered caps (Request) or the
    /// message's own copy (EPR_Request); `None` when unknown.
    pub pdo: Option<SourcePdo>,
}

impl FixedRdo {
    fn decode(raw: u32) -> FixedRdo {
        FixedRdo {
            raw,
            position: bits(raw, 31, 28) as u8,
            give_back: bit(raw, 27),
            capability_mismatch: bit(raw, 26),
            usb_communications_capable: bit(raw, 25),
            no_usb_suspend: bit(raw, 24),
            unchunked_extended_messages: bit(raw, 23),
            epr_capable: bit(raw, 22),
            operating_current_ma: bits(raw, 19, 10) * 10,
            max_current_ma: bits(raw, 9, 0) * 10,
        }
    }
}

impl Rdo {
    /// Decode `raw` as a request for `pdo`; `None` leaves it unresolved.
    pub fn decode(raw: u32, pdo: Option<&SourcePdo>) -> Rdo {
        let position = bits(raw, 31, 28) as u8;
        let capability_mismatch = bit(raw, 26);
        match pdo {
            Some(SourcePdo::Fixed(_)) => Rdo::Fixed(FixedRdo::decode(raw)),
            Some(SourcePdo::Variable(_)) => Rdo::Variable(FixedRdo::decode(raw)),
            Some(SourcePdo::Battery(_)) => Rdo::Battery(BatteryRdo {
                raw,
                position,
                give_back: bit(raw, 27),
                capability_mismatch,
                operating_power_mw: bits(raw, 19, 10) * 250,
                max_power_mw: bits(raw, 9, 0) * 250,
            }),
            Some(SourcePdo::Pps(_)) => Rdo::Pps(PpsRdo {
                raw,
                position,
                capability_mismatch,
                voltage_mv: bits(raw, 20, 9) * 20,
                current_ma: bits(raw, 6, 0) * 50,
            }),
            Some(SourcePdo::SprAvs(_) | SourcePdo::EprAvs(_)) => Rdo::Avs(AvsRdo {
                raw,
                position,
                capability_mismatch,
                voltage_mv: bits(raw, 20, 9) * 25,
                current_ma: bits(raw, 6, 0) * 50,
            }),
            Some(SourcePdo::Reserved(_)) | None => Rdo::Unresolved { position, raw },
        }
    }

    /// The 32 RDO bits.
    pub fn raw(&self) -> u32 {
        match self {
            Rdo::Fixed(r) | Rdo::Variable(r) => r.raw,
            Rdo::Battery(r) => r.raw,
            Rdo::Pps(r) => r.raw,
            Rdo::Avs(r) => r.raw,
            Rdo::Unresolved { raw, .. } => *raw,
        }
    }

    /// Object Position (bits 31..28).
    pub fn position(&self) -> u8 {
        bits(self.raw(), 31, 28) as u8
    }

    /// The `RDO` node of the field tree.
    pub fn field(&self) -> Field {
        let r = self.raw();
        let position = num("Object Position", r, 31, 28, |v| v.to_string())
            .flagged((self.position() == 0).then_some(Flag::Invalid));
        let flags_25_22 = || {
            vec![
                flag("USB Communications Capable", r, 25),
                flag("No USB Suspend", r, 24),
                flag("Unchunked Extended Messages Supported", r, 23),
                flag("EPR Capable", r, 22),
            ]
        };
        let mut children = vec![position];
        let (label, rest) = match self {
            Rdo::Fixed(f) | Rdo::Variable(f) => {
                let mut v = vec![
                    flag("GiveBack (deprecated)", r, 27),
                    flag("Capability Mismatch", r, 26),
                ];
                v.extend(flags_25_22());
                v.extend([
                    reserved(r, 21, 20),
                    num("Operating Current", r, 19, 10, |v| amps(v * 10)),
                    num(
                        if f.give_back {
                            "Minimum Operating Current"
                        } else {
                            "Maximum Operating Current"
                        },
                        r,
                        9,
                        0,
                        |v| amps(v * 10),
                    ),
                ]);
                ("Fixed/Variable RDO", v)
            }
            Rdo::Battery(b) => {
                let mut v = vec![
                    flag("GiveBack (deprecated)", r, 27),
                    flag("Capability Mismatch", r, 26),
                ];
                v.extend(flags_25_22());
                v.extend([
                    reserved(r, 21, 20),
                    num("Operating Power", r, 19, 10, |v| watts(v * 250)),
                    num(
                        if b.give_back {
                            "Minimum Operating Power"
                        } else {
                            "Maximum Operating Power"
                        },
                        r,
                        9,
                        0,
                        |v| watts(v * 250),
                    ),
                ]);
                ("Battery RDO", v)
            }
            Rdo::Pps(_) | Rdo::Avs(_) => {
                let step = if matches!(self, Rdo::Pps(_)) { 20 } else { 25 };
                let mut v = vec![reserved(r, 27, 27), flag("Capability Mismatch", r, 26)];
                v.extend(flags_25_22());
                v.extend([
                    reserved(r, 21, 21),
                    num("Output Voltage", r, 20, 9, move |v| volts(v * step)),
                    reserved(r, 8, 7),
                    num("Operating Current", r, 6, 0, |v| amps(v * 50)),
                ]);
                (if step == 20 { "PPS RDO" } else { "AVS RDO" }, v)
            }
            Rdo::Unresolved { .. } => (
                "Unresolved RDO",
                vec![Field::new(
                    "Request Bits",
                    27,
                    0,
                    u64::from(bits(r, 27, 0)),
                    format!("{:#09X}", bits(r, 27, 0)),
                )],
            ),
        };
        children.extend(rest);
        Field::new("RDO", 31, 0, u64::from(r), label).with_children(children)
    }
}

impl Request {
    /// Resolve `raw` against the remembered `caps` (a Request).
    pub(crate) fn against_caps(raw: u32, caps: Option<&[SourcePdo]>) -> (Request, Vec<Issue>) {
        let position = bits(raw, 31, 28) as u8;
        let mut issues = Vec::new();
        let pdo = match caps {
            None => {
                issues.push(Issue::NoSourceCapabilities);
                None
            }
            Some(caps) => {
                let pdo = usize::from(position)
                    .checked_sub(1)
                    .and_then(|i| caps.get(i))
                    .copied();
                if pdo.is_none() {
                    issues.push(Issue::PositionOutOfRange {
                        position,
                        available: caps.len(),
                    });
                }
                pdo
            }
        };
        let rdo = Rdo::decode(raw, pdo.as_ref());
        (Request { rdo, pdo }, issues)
    }

    /// Decode `raw` against the message's own copy of the PDO (EPR_Request).
    pub(crate) fn with_copy(raw: u32, copy: u32) -> Request {
        let pdo = SourcePdo::decode(copy);
        Request {
            rdo: Rdo::decode(raw, Some(&pdo)),
            pdo: Some(pdo),
        }
    }

    /// Summary text after the message name, e.g. "#1 Fixed 5.00 V 3.00 A".
    pub(crate) fn summary(&self) -> String {
        let pos = self.rdo.position();
        match (&self.rdo, &self.pdo) {
            (Rdo::Fixed(f), Some(SourcePdo::Fixed(p))) => format!(
                "#{pos} Fixed {} {}",
                volts(p.voltage_mv),
                amps(f.operating_current_ma)
            ),
            (Rdo::Variable(f), Some(SourcePdo::Variable(p))) => format!(
                "#{pos} Variable {}-{} {}",
                volts(p.min_voltage_mv),
                volts(p.max_voltage_mv),
                amps(f.operating_current_ma)
            ),
            (Rdo::Battery(b), _) => format!("#{pos} Battery {}", watts(b.operating_power_mw)),
            (Rdo::Pps(p), _) => {
                format!("#{pos} PPS {} {}", volts(p.voltage_mv), amps(p.current_ma))
            }
            (Rdo::Avs(a), _) => {
                format!("#{pos} AVS {} {}", volts(a.voltage_mv), amps(a.current_ma))
            }
            _ => format!("#{pos} (unknown PDO)"),
        }
    }

    /// The `Copy of PDO` node (EPR_Request).
    pub(crate) fn copy_field(&self) -> Option<Field> {
        self.pdo.map(|p| {
            let mut f = p.field(usize::from(self.rdo.position()));
            f.name = "Copy of PDO".to_string();
            f
        })
    }
}
