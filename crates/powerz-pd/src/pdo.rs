//! Power data objects (USB PD r3.2 §6.4.1): source and sink PDOs and APDOs.

use crate::field::{Field, Flag, amps, bit, bits, flag, num, reserved, volts, watts};

/// The supply type field (bits 31..30) and, for an APDO, its subtype
/// (bits 29..28).
fn kind_fields(raw: u32) -> Vec<Field> {
    let ty = bits(raw, 31, 30);
    let name = ["Fixed Supply", "Battery", "Variable Supply", "Augmented"][ty as usize & 3];
    let mut out = vec![Field::new("Supply Type", 31, 30, u64::from(ty), name)];
    if ty == 3 {
        let sub = bits(raw, 29, 28);
        let (label, flag) = match sub {
            0 => ("SPR PPS", None),
            1 => ("EPR AVS", None),
            2 => ("SPR AVS", None),
            _ => ("Reserved", Some(Flag::Reserved)),
        };
        out.push(Field::new("APDO Type", 29, 28, u64::from(sub), label).flagged(flag));
    }
    out
}

fn pdo_node(index: usize, raw: u32, summary: String, mut children: Vec<Field>) -> Field {
    let mut all = kind_fields(raw);
    all.append(&mut children);
    Field::new(format!("PDO {index}"), 31, 0, u64::from(raw), summary).with_children(all)
}

impl Battery {
    fn decode(raw: u32) -> Battery {
        Battery {
            raw,
            max_voltage_mv: bits(raw, 29, 20) * 50,
            min_voltage_mv: bits(raw, 19, 10) * 50,
            power_mw: bits(raw, 9, 0) * 250,
        }
    }

    fn fields(&self, sink: bool) -> Vec<Field> {
        let r = self.raw;
        vec![
            num("Maximum Voltage", r, 29, 20, |v| volts(v * 50)),
            num("Minimum Voltage", r, 19, 10, |v| volts(v * 50)),
            num(
                if sink {
                    "Operational Power"
                } else {
                    "Maximum Allowable Power"
                },
                r,
                9,
                0,
                |v| watts(v * 250),
            ),
        ]
    }

    fn summary(&self) -> String {
        format!(
            "Battery {}-{} {}",
            volts(self.min_voltage_mv),
            volts(self.max_voltage_mv),
            watts(self.power_mw)
        )
    }
}

impl Variable {
    fn decode(raw: u32) -> Variable {
        Variable {
            raw,
            max_voltage_mv: bits(raw, 29, 20) * 50,
            min_voltage_mv: bits(raw, 19, 10) * 50,
            current_ma: bits(raw, 9, 0) * 10,
        }
    }

    fn fields(&self, sink: bool) -> Vec<Field> {
        let r = self.raw;
        vec![
            num("Maximum Voltage", r, 29, 20, |v| volts(v * 50)),
            num("Minimum Voltage", r, 19, 10, |v| volts(v * 50)),
            num(
                if sink {
                    "Operational Current"
                } else {
                    "Maximum Current"
                },
                r,
                9,
                0,
                |v| amps(v * 10),
            ),
        ]
    }

    fn summary(&self) -> String {
        format!(
            "Variable {}-{} {}",
            volts(self.min_voltage_mv),
            volts(self.max_voltage_mv),
            amps(self.current_ma)
        )
    }
}

impl Pps {
    fn decode(raw: u32, sink: bool) -> Pps {
        Pps {
            raw,
            power_limited: !sink && bit(raw, 27),
            // Bits 24..17: bit 25 is reserved in an SPR PPS APDO.
            max_voltage_mv: bits(raw, 24, 17) * 100,
            min_voltage_mv: bits(raw, 15, 8) * 100,
            max_current_ma: bits(raw, 6, 0) * 50,
        }
    }

    fn fields(&self, sink: bool) -> Vec<Field> {
        let r = self.raw;
        let mut out = if sink {
            vec![reserved(r, 27, 25)]
        } else {
            vec![flag("PPS Power Limited", r, 27), reserved(r, 26, 25)]
        };
        out.extend([
            num("Maximum Voltage", r, 24, 17, |v| volts(v * 100)),
            reserved(r, 16, 16),
            num("Minimum Voltage", r, 15, 8, |v| volts(v * 100)),
            reserved(r, 7, 7),
            num("Maximum Current", r, 6, 0, |v| amps(v * 50)),
        ]);
        out
    }

    fn summary(&self) -> String {
        format!(
            "PPS {}-{} {}",
            volts(self.min_voltage_mv),
            volts(self.max_voltage_mv),
            amps(self.max_current_ma)
        )
    }
}

impl SprAvs {
    fn decode(raw: u32, sink: bool) -> SprAvs {
        SprAvs {
            raw,
            peak_current: if sink { 0 } else { bits(raw, 27, 26) as u8 },
            max_current_15v_ma: bits(raw, 19, 10) * 10,
            max_current_20v_ma: bits(raw, 9, 0) * 10,
        }
    }

    fn fields(&self, sink: bool) -> Vec<Field> {
        let r = self.raw;
        let mut out = if sink {
            vec![reserved(r, 27, 20)]
        } else {
            vec![
                num("Peak Current", r, 27, 26, |v| v.to_string()),
                reserved(r, 25, 20),
            ]
        };
        out.extend([
            num("Maximum Current 15V", r, 19, 10, |v| amps(v * 10)),
            num("Maximum Current 20V", r, 9, 0, |v| amps(v * 10)),
        ]);
        out
    }

    fn summary(&self) -> String {
        format!(
            "SPR AVS 9-15 V {}, 15-20 V {}",
            amps(self.max_current_15v_ma),
            amps(self.max_current_20v_ma)
        )
    }
}

impl EprAvs {
    fn decode(raw: u32, sink: bool) -> EprAvs {
        EprAvs {
            raw,
            peak_current: if sink { 0 } else { bits(raw, 27, 26) as u8 },
            max_voltage_mv: bits(raw, 25, 17) * 100,
            min_voltage_mv: bits(raw, 15, 8) * 100,
            pdp_w: bits(raw, 7, 0),
        }
    }

    fn fields(&self, sink: bool) -> Vec<Field> {
        let r = self.raw;
        let mut out = if sink {
            vec![reserved(r, 27, 26)]
        } else {
            vec![num("Peak Current", r, 27, 26, |v| v.to_string())]
        };
        out.extend([
            num("Maximum Voltage", r, 25, 17, |v| volts(v * 100)),
            reserved(r, 16, 16),
            num("Minimum Voltage", r, 15, 8, |v| volts(v * 100)),
            num("PDP", r, 7, 0, |v| format!("{v} W")),
        ]);
        out
    }

    fn summary(&self) -> String {
        format!(
            "EPR AVS {}-{} {} W",
            volts(self.min_voltage_mv),
            volts(self.max_voltage_mv),
            self.pdp_w
        )
    }
}

/// The reserved APDO subtype's remaining bits, flagged when set.
fn reserved_apdo_fields(raw: u32) -> Vec<Field> {
    vec![reserved(raw, 27, 0)]
}

/// Fixed supply source PDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedSource {
    /// The 32 PDO bits.
    pub raw: u32,
    /// Dual-Role Power (bit 29).
    pub dual_role_power: bool,
    /// USB Suspend Supported (bit 28).
    pub usb_suspend_supported: bool,
    /// Unconstrained Power (bit 27).
    pub unconstrained_power: bool,
    /// USB Communications Capable (bit 26).
    pub usb_communications_capable: bool,
    /// Dual-Role Data (bit 25).
    pub dual_role_data: bool,
    /// Unchunked Extended Messages Supported (bit 24).
    pub unchunked_extended_messages: bool,
    /// EPR Capable (bit 23).
    pub epr_capable: bool,
    /// Peak Current (bits 21..20).
    pub peak_current: u8,
    /// Voltage, mV (bits 19..10, 50 mV units).
    pub voltage_mv: u32,
    /// Maximum Current, mA (bits 9..0, 10 mA units).
    pub max_current_ma: u32,
}

/// Fixed supply sink PDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedSink {
    /// The 32 PDO bits.
    pub raw: u32,
    /// Dual-Role Power (bit 29).
    pub dual_role_power: bool,
    /// Higher Capability (bit 28).
    pub higher_capability: bool,
    /// Unconstrained Power (bit 27).
    pub unconstrained_power: bool,
    /// USB Communications Capable (bit 26).
    pub usb_communications_capable: bool,
    /// Dual-Role Data (bit 25).
    pub dual_role_data: bool,
    /// Fast Role Swap required USB Type-C current (bits 24..23).
    pub fast_role_swap: u8,
    /// Voltage, mV (bits 19..10, 50 mV units).
    pub voltage_mv: u32,
    /// Operational Current, mA (bits 9..0, 10 mA units).
    pub operational_current_ma: u32,
}

/// Battery supply PDO (source or sink).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// The 32 PDO bits.
    pub raw: u32,
    /// Maximum Voltage, mV (bits 29..20, 50 mV units).
    pub max_voltage_mv: u32,
    /// Minimum Voltage, mV (bits 19..10, 50 mV units).
    pub min_voltage_mv: u32,
    /// Maximum (source) or Operational (sink) Power, mW (bits 9..0, 250 mW units).
    pub power_mw: u32,
}

/// Variable supply PDO (source or sink).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variable {
    /// The 32 PDO bits.
    pub raw: u32,
    /// Maximum Voltage, mV (bits 29..20, 50 mV units).
    pub max_voltage_mv: u32,
    /// Minimum Voltage, mV (bits 19..10, 50 mV units).
    pub min_voltage_mv: u32,
    /// Maximum (source) or Operational (sink) Current, mA (bits 9..0, 10 mA units).
    pub current_ma: u32,
}

/// SPR Programmable Power Supply APDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pps {
    /// The 32 APDO bits.
    pub raw: u32,
    /// PPS Power Limited (bit 27, source only).
    pub power_limited: bool,
    /// Maximum Voltage, mV (bits 24..17, 100 mV units).
    pub max_voltage_mv: u32,
    /// Minimum Voltage, mV (bits 15..8, 100 mV units).
    pub min_voltage_mv: u32,
    /// Maximum Current, mA (bits 6..0, 50 mA units).
    pub max_current_ma: u32,
}

/// SPR Adjustable Voltage Supply APDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SprAvs {
    /// The 32 APDO bits.
    pub raw: u32,
    /// Peak Current (bits 27..26).
    pub peak_current: u8,
    /// Maximum Current for 9..15 V, mA (bits 19..10, 10 mA units).
    pub max_current_15v_ma: u32,
    /// Maximum Current for 15..20 V, mA (bits 9..0, 10 mA units).
    pub max_current_20v_ma: u32,
}

/// EPR Adjustable Voltage Supply APDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EprAvs {
    /// The 32 APDO bits.
    pub raw: u32,
    /// Peak Current (bits 27..26).
    pub peak_current: u8,
    /// Maximum Voltage, mV (bits 25..17, 100 mV units).
    pub max_voltage_mv: u32,
    /// Minimum Voltage, mV (bits 15..8, 100 mV units).
    pub min_voltage_mv: u32,
    /// PDP, W (bits 7..0, 1 W units).
    pub pdp_w: u32,
}

/// A source power data object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourcePdo {
    /// Fixed supply.
    Fixed(FixedSource),
    /// Battery.
    Battery(Battery),
    /// Variable supply.
    Variable(Variable),
    /// SPR PPS APDO.
    Pps(Pps),
    /// SPR AVS APDO.
    SprAvs(SprAvs),
    /// EPR AVS APDO.
    EprAvs(EprAvs),
    /// Reserved APDO subtype, kept raw.
    Reserved(u32),
}

impl SourcePdo {
    /// Decode one source PDO.
    pub fn decode(raw: u32) -> SourcePdo {
        match (bits(raw, 31, 30), bits(raw, 29, 28)) {
            (0, _) => SourcePdo::Fixed(FixedSource {
                raw,
                dual_role_power: bit(raw, 29),
                usb_suspend_supported: bit(raw, 28),
                unconstrained_power: bit(raw, 27),
                usb_communications_capable: bit(raw, 26),
                dual_role_data: bit(raw, 25),
                unchunked_extended_messages: bit(raw, 24),
                epr_capable: bit(raw, 23),
                peak_current: bits(raw, 21, 20) as u8,
                voltage_mv: bits(raw, 19, 10) * 50,
                max_current_ma: bits(raw, 9, 0) * 10,
            }),
            (1, _) => SourcePdo::Battery(Battery::decode(raw)),
            (2, _) => SourcePdo::Variable(Variable::decode(raw)),
            (_, 0) => SourcePdo::Pps(Pps::decode(raw, false)),
            (_, 1) => SourcePdo::EprAvs(EprAvs::decode(raw, false)),
            (_, 2) => SourcePdo::SprAvs(SprAvs::decode(raw, false)),
            _ => SourcePdo::Reserved(raw),
        }
    }

    /// Short text for the packet table, such as "Fixed 5.00 V 3.00 A".
    pub(crate) fn summary(&self) -> String {
        match self {
            SourcePdo::Fixed(f) => {
                format!("Fixed {} {}", volts(f.voltage_mv), amps(f.max_current_ma))
            }
            SourcePdo::Battery(b) => format!(
                "Battery {}-{} {}",
                volts(b.min_voltage_mv),
                volts(b.max_voltage_mv),
                watts(b.power_mw)
            ),
            SourcePdo::Variable(v) => format!(
                "Variable {}-{} {}",
                volts(v.min_voltage_mv),
                volts(v.max_voltage_mv),
                amps(v.current_ma)
            ),
            SourcePdo::Pps(p) => format!(
                "PPS {}-{} {}",
                volts(p.min_voltage_mv),
                volts(p.max_voltage_mv),
                amps(p.max_current_ma)
            ),
            SourcePdo::SprAvs(a) => format!(
                "SPR AVS {} / {}",
                amps(a.max_current_15v_ma),
                amps(a.max_current_20v_ma)
            ),
            SourcePdo::EprAvs(a) => format!(
                "EPR AVS {}-{} {} W",
                volts(a.min_voltage_mv),
                volts(a.max_voltage_mv),
                a.pdp_w
            ),
            SourcePdo::Reserved(raw) => format!("Reserved {raw:#010X}"),
        }
    }

    /// The `PDO <index>` node of the field tree (index counts from 1).
    pub fn field(&self, index: usize) -> Field {
        match self {
            SourcePdo::Fixed(f) => {
                let r = f.raw;
                pdo_node(
                    index,
                    r,
                    format!("Fixed {} {}", volts(f.voltage_mv), amps(f.max_current_ma)),
                    vec![
                        flag("Dual-Role Power", r, 29),
                        flag("USB Suspend Supported", r, 28),
                        flag("Unconstrained Power", r, 27),
                        flag("USB Communications Capable", r, 26),
                        flag("Dual-Role Data", r, 25),
                        flag("Unchunked Extended Messages Supported", r, 24),
                        flag("EPR Capable", r, 23),
                        reserved(r, 22, 22),
                        num("Peak Current", r, 21, 20, |v| v.to_string()),
                        num("Voltage", r, 19, 10, |v| volts(v * 50)),
                        num("Maximum Current", r, 9, 0, |v| amps(v * 10)),
                    ],
                )
            }
            SourcePdo::Battery(b) => pdo_node(index, b.raw, b.summary(), b.fields(false)),
            SourcePdo::Variable(v) => pdo_node(index, v.raw, v.summary(), v.fields(false)),
            SourcePdo::Pps(p) => pdo_node(index, p.raw, p.summary(), p.fields(false)),
            SourcePdo::SprAvs(a) => pdo_node(index, a.raw, a.summary(), a.fields(false)),
            SourcePdo::EprAvs(a) => pdo_node(index, a.raw, a.summary(), a.fields(false)),
            SourcePdo::Reserved(r) => pdo_node(
                index,
                *r,
                "Reserved APDO".to_string(),
                reserved_apdo_fields(*r),
            ),
        }
    }
}

/// A sink power data object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SinkPdo {
    /// Fixed supply.
    Fixed(FixedSink),
    /// Battery.
    Battery(Battery),
    /// Variable supply.
    Variable(Variable),
    /// SPR PPS APDO.
    Pps(Pps),
    /// SPR AVS APDO.
    SprAvs(SprAvs),
    /// EPR AVS APDO.
    EprAvs(EprAvs),
    /// Reserved APDO subtype, kept raw.
    Reserved(u32),
}

impl SinkPdo {
    /// Decode one sink PDO.
    pub fn decode(raw: u32) -> SinkPdo {
        match (bits(raw, 31, 30), bits(raw, 29, 28)) {
            (0, _) => SinkPdo::Fixed(FixedSink {
                raw,
                dual_role_power: bit(raw, 29),
                higher_capability: bit(raw, 28),
                unconstrained_power: bit(raw, 27),
                usb_communications_capable: bit(raw, 26),
                dual_role_data: bit(raw, 25),
                fast_role_swap: bits(raw, 24, 23) as u8,
                voltage_mv: bits(raw, 19, 10) * 50,
                operational_current_ma: bits(raw, 9, 0) * 10,
            }),
            (1, _) => SinkPdo::Battery(Battery::decode(raw)),
            (2, _) => SinkPdo::Variable(Variable::decode(raw)),
            (_, 0) => SinkPdo::Pps(Pps::decode(raw, true)),
            (_, 1) => SinkPdo::EprAvs(EprAvs::decode(raw, true)),
            (_, 2) => SinkPdo::SprAvs(SprAvs::decode(raw, true)),
            _ => SinkPdo::Reserved(raw),
        }
    }

    /// The `PDO <index>` node of the field tree (index counts from 1).
    pub fn field(&self, index: usize) -> Field {
        match self {
            SinkPdo::Fixed(f) => {
                let r = f.raw;
                pdo_node(
                    index,
                    r,
                    format!(
                        "Fixed {} {}",
                        volts(f.voltage_mv),
                        amps(f.operational_current_ma)
                    ),
                    vec![
                        flag("Dual-Role Power", r, 29),
                        flag("Higher Capability", r, 28),
                        flag("Unconstrained Power", r, 27),
                        flag("USB Communications Capable", r, 26),
                        flag("Dual-Role Data", r, 25),
                        num("Fast Role Swap Required Current", r, 24, 23, |v| {
                            [
                                "Not supported",
                                "Default USB Power",
                                "1.5 A @ 5 V",
                                "3.0 A @ 5 V",
                            ][v as usize & 3]
                                .to_string()
                        }),
                        reserved(r, 22, 20),
                        num("Voltage", r, 19, 10, |v| volts(v * 50)),
                        num("Operational Current", r, 9, 0, |v| amps(v * 10)),
                    ],
                )
            }
            SinkPdo::Battery(b) => pdo_node(index, b.raw, b.summary(), b.fields(true)),
            SinkPdo::Variable(v) => pdo_node(index, v.raw, v.summary(), v.fields(true)),
            SinkPdo::Pps(p) => pdo_node(index, p.raw, p.summary(), p.fields(true)),
            SinkPdo::SprAvs(a) => pdo_node(index, a.raw, a.summary(), a.fields(true)),
            SinkPdo::EprAvs(a) => pdo_node(index, a.raw, a.summary(), a.fields(true)),
            SinkPdo::Reserved(r) => pdo_node(
                index,
                *r,
                "Reserved APDO".to_string(),
                reserved_apdo_fields(*r),
            ),
        }
    }
}
