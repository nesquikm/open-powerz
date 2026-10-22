//! Vendor defined messages (USB PD r3.2 §6.4.4).

/// Command Type of a structured VDM (bits 7..6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    /// 00b: request.
    Req,
    /// 01b: acknowledge.
    Ack,
    /// 10b: not acknowledged.
    Nak,
    /// 11b: busy.
    Busy,
}

/// Command of a structured VDM (bits 4..0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdmCommand {
    /// 1.
    DiscoverIdentity,
    /// 2.
    DiscoverSvids,
    /// 3.
    DiscoverModes,
    /// 4.
    EnterMode,
    /// 5.
    ExitMode,
    /// 6.
    Attention,
    /// 16..31: defined by the SVID.
    SvidSpecific(u8),
    /// 0 and 7..15.
    Reserved(u8),
}

/// Structured VDM header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredVdmHeader {
    /// The 32 header bits.
    pub raw: u32,
    /// SVID (bits 31..16).
    pub svid: u16,
    /// Structured VDM Version major (bits 14..13).
    pub major: u8,
    /// Structured VDM Version minor (bits 12..11).
    pub minor: u8,
    /// Object Position (bits 10..8).
    pub object_position: u8,
    /// Command Type (bits 7..6).
    pub command_type: CommandType,
    /// Command (bits 4..0).
    pub command: VdmCommand,
}

/// Unstructured VDM header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnstructuredVdmHeader {
    /// The 32 header bits.
    pub raw: u32,
    /// Vendor ID (bits 31..16).
    pub svid: u16,
    /// Vendor use (bits 14..0).
    pub vendor_use: u16,
}

/// A VDM header, by VDM Type (bit 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdmHeader {
    /// VDM Type 1.
    Structured(StructuredVdmHeader),
    /// VDM Type 0.
    Unstructured(UnstructuredVdmHeader),
}

/// ID Header VDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdHeader {
    /// The 32 VDO bits.
    pub raw: u32,
    /// USB Communications Capable as USB Host (bit 31).
    pub usb_host: bool,
    /// USB Communications Capable as a USB Device (bit 30).
    pub usb_device: bool,
    /// SOP Product Type (UFP) or SOP' Product Type (Cable Plug/VPD) (bits 29..27).
    pub product_type_ufp: u8,
    /// Modal Operation Supported (bit 26).
    pub modal: bool,
    /// Product Type (DFP) (bits 25..23).
    pub product_type_dfp: u8,
    /// Connector Type (bits 22..21).
    pub connector_type: u8,
    /// USB Vendor ID (bits 15..0).
    pub vid: u16,
}

/// Product VDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductVdo {
    /// The 32 VDO bits.
    pub raw: u32,
    /// USB Product ID (bits 31..16).
    pub pid: u16,
    /// bcdDevice (bits 15..0).
    pub bcd_device: u16,
}

/// Passive Cable VDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassiveCableVdo {
    /// The 32 VDO bits.
    pub raw: u32,
    /// USB Type-C plug to USB Type-C/Captive (bits 19..18).
    pub plug_type: u8,
    /// EPR Capable (bit 17).
    pub epr_capable: bool,
    /// Cable Latency (bits 16..13).
    pub latency: u8,
    /// Cable Termination Type (bits 12..11).
    pub termination: u8,
    /// Maximum VBUS Voltage (bits 10..9).
    pub max_vbus_voltage: u8,
    /// VBUS Current Handling Capability (bits 6..5).
    pub vbus_current: u8,
    /// USB Highest Speed (bits 2..0).
    pub usb_speed: u8,
}

/// Active Cable VDO 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveCableVdo {
    /// The 32 VDO bits.
    pub raw: u32,
    /// VDO Version (bits 23..21).
    pub vdo_version: u8,
    /// Cable Termination Type (bits 12..11).
    pub termination: u8,
    /// VBUS Current Handling Capability (bits 6..5).
    pub vbus_current: u8,
    /// VBUS Through Cable (bit 4).
    pub vbus_through_cable: bool,
    /// SOP'' Controller Present (bit 3).
    pub sop_double_prime_present: bool,
    /// USB Highest Speed (bits 2..0).
    pub usb_speed: u8,
}

/// Active Cable VDO 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveCableVdo2 {
    /// The 32 VDO bits.
    pub raw: u32,
}

/// UFP VDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UfpVdo {
    /// The 32 VDO bits.
    pub raw: u32,
    /// UFP VDO Version (bits 31..29).
    pub vdo_version: u8,
    /// Device Capability (bits 27..24).
    pub device_capability: u8,
    /// Alternate Modes (bits 5..3).
    pub alternate_modes: u8,
    /// USB Highest Speed (bits 2..0).
    pub usb_speed: u8,
}

/// DFP VDO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DfpVdo {
    /// The 32 VDO bits.
    pub raw: u32,
    /// DFP VDO Version (bits 31..29).
    pub vdo_version: u8,
    /// Host Capability (bits 26..24).
    pub host_capability: u8,
    /// Port Number (bits 4..0).
    pub port_number: u8,
}

/// A product type VDO of a Discover Identity ACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductTypeVdo {
    /// Passive cable.
    PassiveCable(PassiveCableVdo),
    /// Active cable, VDO 1.
    ActiveCable(ActiveCableVdo),
    /// Active cable, VDO 2.
    ActiveCable2(ActiveCableVdo2),
    /// UFP.
    Ufp(UfpVdo),
    /// DFP.
    Dfp(DfpVdo),
    /// A VDO the product types do not explain, kept raw.
    Other(u32),
}

/// Body of a Discover Identity ACK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverIdentity {
    /// ID Header VDO.
    pub id_header: IdHeader,
    /// Cert Stat VDO (the XID).
    pub cert_stat: Option<u32>,
    /// Product VDO.
    pub product: Option<ProductVdo>,
    /// Product type VDOs.
    pub product_type_vdos: Vec<ProductTypeVdo>,
}

/// What follows the VDM header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdmBody {
    /// Discover Identity ACK.
    DiscoverIdentity(DiscoverIdentity),
    /// Discover SVIDs ACK: the SVIDs, without the zero terminators.
    DiscoverSvids(Vec<u16>),
    /// Discover Modes ACK: the mode VDOs.
    DiscoverModes(Vec<u32>),
    /// Any other VDOs, raw.
    Objects(Vec<u32>),
}

/// A Vendor_Defined message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vdm {
    /// The VDM header.
    pub header: VdmHeader,
    /// The VDOs after it.
    pub body: VdmBody,
}

use crate::field::{Field, Flag};
use crate::field::{bit, bits, flag, num, reserved, yes_no};
use crate::header::Sop;

impl CommandType {
    fn from_bits(v: u32) -> CommandType {
        match v & 3 {
            0 => CommandType::Req,
            1 => CommandType::Ack,
            2 => CommandType::Nak,
            _ => CommandType::Busy,
        }
    }

    /// Short spec name: REQ, ACK, NAK or BUSY.
    pub fn name(self) -> &'static str {
        match self {
            CommandType::Req => "REQ",
            CommandType::Ack => "ACK",
            CommandType::Nak => "NAK",
            CommandType::Busy => "BUSY",
        }
    }
}

impl VdmCommand {
    fn from_bits(v: u32) -> VdmCommand {
        let v = (v & 0x1F) as u8;
        match v {
            1 => VdmCommand::DiscoverIdentity,
            2 => VdmCommand::DiscoverSvids,
            3 => VdmCommand::DiscoverModes,
            4 => VdmCommand::EnterMode,
            5 => VdmCommand::ExitMode,
            6 => VdmCommand::Attention,
            16..=31 => VdmCommand::SvidSpecific(v),
            _ => VdmCommand::Reserved(v),
        }
    }

    /// Spec name of the command.
    pub fn name(self) -> String {
        match self {
            VdmCommand::DiscoverIdentity => "Discover Identity".into(),
            VdmCommand::DiscoverSvids => "Discover SVIDs".into(),
            VdmCommand::DiscoverModes => "Discover Modes".into(),
            VdmCommand::EnterMode => "Enter Mode".into(),
            VdmCommand::ExitMode => "Exit Mode".into(),
            VdmCommand::Attention => "Attention".into(),
            VdmCommand::SvidSpecific(v) => format!("SVID Specific ({v})"),
            VdmCommand::Reserved(v) => format!("Reserved ({v})"),
        }
    }
}

impl VdmHeader {
    /// Decode a VDM header by its VDM Type bit.
    pub fn decode(raw: u32) -> VdmHeader {
        let svid = bits(raw, 31, 16) as u16;
        if bit(raw, 15) {
            VdmHeader::Structured(StructuredVdmHeader {
                raw,
                svid,
                major: bits(raw, 14, 13) as u8,
                minor: bits(raw, 12, 11) as u8,
                object_position: bits(raw, 10, 8) as u8,
                command_type: CommandType::from_bits(bits(raw, 7, 6)),
                command: VdmCommand::from_bits(bits(raw, 4, 0)),
            })
        } else {
            VdmHeader::Unstructured(UnstructuredVdmHeader {
                raw,
                svid,
                vendor_use: bits(raw, 14, 0) as u16,
            })
        }
    }

    fn field(&self) -> Field {
        match self {
            VdmHeader::Structured(h) => {
                let r = h.raw;
                let flag_cmd =
                    matches!(h.command, VdmCommand::Reserved(_)).then_some(Flag::Reserved);
                Field::new(
                    "VDM Header",
                    31,
                    0,
                    u64::from(r),
                    format!("{} {}", h.command.name(), h.command_type.name()),
                )
                .with_children(vec![
                    num("SVID", r, 31, 16, |v| format!("{v:#06X}")),
                    num("VDM Type", r, 15, 15, |_| "Structured".into()),
                    num("Structured VDM Version (Major)", r, 14, 13, |v| match v {
                        0 => "Version 1.0".into(),
                        1 => "Version 2.x".into(),
                        _ => "Reserved".into(),
                    }),
                    num("Structured VDM Version (Minor)", r, 12, 11, |v| match v {
                        0 => "x.0".into(),
                        1 => "x.1".into(),
                        _ => "Reserved".into(),
                    }),
                    num("Object Position", r, 10, 8, |v| v.to_string()),
                    num("Command Type", r, 7, 6, |_| h.command_type.name().into()),
                    reserved(r, 5, 5),
                    num("Command", r, 4, 0, |_| h.command.name()).flagged(flag_cmd),
                ])
            }
            VdmHeader::Unstructured(u) => {
                let r = u.raw;
                Field::new(
                    "VDM Header",
                    31,
                    0,
                    u64::from(r),
                    format!("Unstructured, SVID {:#06X}", u.svid),
                )
                .with_children(vec![
                    num("SVID", r, 31, 16, |v| format!("{v:#06X}")),
                    num("VDM Type", r, 15, 15, |_| "Unstructured".into()),
                    num("Vendor Use", r, 14, 0, |v| format!("{v:#06X}")),
                ])
            }
        }
    }
}

impl IdHeader {
    fn decode(raw: u32) -> IdHeader {
        IdHeader {
            raw,
            usb_host: bit(raw, 31),
            usb_device: bit(raw, 30),
            product_type_ufp: bits(raw, 29, 27) as u8,
            modal: bit(raw, 26),
            product_type_dfp: bits(raw, 25, 23) as u8,
            connector_type: bits(raw, 22, 21) as u8,
            vid: bits(raw, 15, 0) as u16,
        }
    }

    fn field(&self, cable: bool) -> Field {
        let r = self.raw;
        let ufp = if cable {
            num("SOP' Product Type (Cable Plug/VPD)", r, 29, 27, |v| {
                match v {
                    0 => "Not a Cable Plug/VPD",
                    3 => "Passive Cable",
                    4 => "Active Cable",
                    6 => "VCONN-Powered USB Device",
                    _ => "Reserved",
                }
                .into()
            })
        } else {
            num("SOP Product Type (UFP)", r, 29, 27, |v| {
                match v {
                    0 => "Not a UFP",
                    1 => "PDUSB Hub",
                    2 => "PDUSB Peripheral",
                    3 => "PSD",
                    _ => "Reserved",
                }
                .into()
            })
        };
        Field::new(
            "ID Header",
            31,
            0,
            u64::from(r),
            format!("VID {:#06X}", self.vid),
        )
        .with_children(vec![
            flag("USB Communications Capable as USB Host", r, 31),
            flag("USB Communications Capable as a USB Device", r, 30),
            ufp,
            flag("Modal Operation Supported", r, 26),
            num("Product Type (DFP)", r, 25, 23, |v| {
                match v {
                    0 => "Not a DFP",
                    1 => "PDUSB Hub",
                    2 => "PDUSB Host",
                    3 => "Power Brick",
                    _ => "Reserved",
                }
                .into()
            }),
            num("Connector Type", r, 22, 21, |v| {
                match v {
                    2 => "USB Type-C Receptacle",
                    3 => "USB Type-C Plug",
                    _ => "Reserved",
                }
                .into()
            }),
            reserved(r, 20, 16),
            num("USB Vendor ID", r, 15, 0, |v| format!("{v:#06X}")),
        ])
    }
}

fn usb_speed(cable: bool) -> impl Fn(u32) -> String {
    move |v| {
        match v {
            0 => "USB 2.0 only",
            1 => "USB 3.2 Gen1",
            2 => "USB 3.2 Gen2 / USB4 Gen2",
            3 => "USB4 Gen3",
            4 if cable => "USB4 Gen4",
            _ => "Reserved",
        }
        .into()
    }
}

fn plug_type(v: u32) -> String {
    match v {
        2 => "USB Type-C",
        3 => "Captive",
        _ => "Reserved",
    }
    .into()
}

fn termination(v: u32) -> String {
    match v {
        0 => "VCONN not required",
        1 => "VCONN required",
        2 => "One end active, one end passive, VCONN required",
        _ => "Both ends active, VCONN required",
    }
    .into()
}

fn max_vbus(v: u32) -> String {
    match v {
        0 => "20 V",
        1 => "30 V (deprecated)",
        2 => "40 V (deprecated)",
        _ => "50 V",
    }
    .into()
}

fn vbus_current(v: u32) -> String {
    match v {
        1 => "3 A",
        2 => "5 A",
        _ => "Reserved",
    }
    .into()
}

fn latency(v: u32) -> String {
    match v {
        1..=8 => format!("<{} ns (~{} m)", v * 10, v),
        9 => "1000 ns (~100 m)".into(),
        10 => "2000 ns (~200 m)".into(),
        11 => "3000 ns (~300 m)".into(),
        _ => "Reserved".into(),
    }
}

/// Fields common to the passive and active cable VDO 1 (bits 31..9).
fn cable_common(r: u32) -> Vec<Field> {
    vec![
        num("HW Version", r, 31, 28, |v| v.to_string()),
        num("Firmware Version", r, 27, 24, |v| v.to_string()),
        num("VDO Version", r, 23, 21, |v| format!("1.{v}")),
        reserved(r, 20, 20),
        num("USB Type-C plug to", r, 19, 18, plug_type),
        flag("EPR Capable", r, 17),
        num("Cable Latency", r, 16, 13, latency),
        num("Cable Termination Type", r, 12, 11, termination),
        num("Maximum VBUS Voltage", r, 10, 9, max_vbus),
    ]
}

impl ProductTypeVdo {
    fn field(&self, position: usize) -> Field {
        let node = |name: &str, raw: u32, children: Vec<Field>| {
            Field::new(name, 31, 0, u64::from(raw), format!("{raw:#010X}")).with_children(children)
        };
        match self {
            ProductTypeVdo::PassiveCable(c) => {
                let r = c.raw;
                let mut ch = cable_common(r);
                ch.extend([
                    reserved(r, 8, 7),
                    num("VBUS Current Handling", r, 6, 5, vbus_current),
                    reserved(r, 4, 3),
                    num("USB Highest Speed", r, 2, 0, usb_speed(true)),
                ]);
                node("Passive Cable VDO", r, ch)
            }
            ProductTypeVdo::ActiveCable(c) => {
                let r = c.raw;
                let mut ch = cable_common(r);
                ch.extend([
                    num("SBU Supported", r, 8, 8, |v| {
                        if v == 0 { "Supported" } else { "Not supported" }.into()
                    }),
                    num("SBU Type", r, 7, 7, |v| {
                        if v == 0 { "Passive" } else { "Active" }.into()
                    }),
                    num("VBUS Current Handling", r, 6, 5, vbus_current),
                    flag("VBUS Through Cable", r, 4),
                    flag("SOP'' Controller Present", r, 3),
                    num("USB Highest Speed", r, 2, 0, usb_speed(true)),
                ]);
                node("Active Cable VDO 1", r, ch)
            }
            ProductTypeVdo::ActiveCable2(c) => {
                let r = c.raw;
                node(
                    "Active Cable VDO 2",
                    r,
                    vec![
                        num("Maximum Operating Temperature", r, 31, 24, |v| {
                            format!("{v} °C")
                        }),
                        num("Shutdown Temperature", r, 23, 16, |v| format!("{v} °C")),
                        reserved(r, 15, 15),
                        num("U3/CLd Power", r, 14, 12, |v| v.to_string()),
                        num("U3 to U0 Transition Mode", r, 11, 11, |v| {
                            if v == 0 { "Direct" } else { "Through U3S" }.into()
                        }),
                        num("Physical Connection", r, 10, 10, |v| {
                            if v == 0 { "Copper" } else { "Optical" }.into()
                        }),
                        num("Active Element", r, 9, 9, |v| {
                            if v == 0 {
                                "Active Redriver"
                            } else {
                                "Active Retimer"
                            }
                            .into()
                        }),
                        num("USB4 Supported", r, 8, 8, |v| yes_no(v == 0).into()),
                        num("USB 2.0 Hub Hops Consumed", r, 7, 6, |v| v.to_string()),
                        num("USB 2.0 Supported", r, 5, 5, |v| yes_no(v == 0).into()),
                        num("USB 3.2 Supported", r, 4, 4, |v| yes_no(v == 0).into()),
                        num("USB Lanes Supported", r, 3, 3, |v| {
                            if v == 0 { "One lane" } else { "Two lanes" }.into()
                        }),
                        flag("Optically Isolated Active Cable", r, 2),
                        flag("USB4 Asymmetric Mode Supported", r, 1),
                        num("USB Gen", r, 0, 0, |v| {
                            if v == 0 { "Gen1" } else { "Gen2 or higher" }.into()
                        }),
                    ],
                )
            }
            ProductTypeVdo::Ufp(u) => {
                let r = u.raw;
                node(
                    "UFP VDO",
                    r,
                    vec![
                        num("UFP VDO Version", r, 31, 29, |v| format!("1.{v}")),
                        reserved(r, 28, 28),
                        num("Device Capability", r, 27, 24, |v| {
                            let names = ["USB2.0", "USB2.0 Billboard", "USB3.2", "USB4"];
                            let caps: Vec<&str> = (0..4)
                                .filter(|i| v & (1 << i) != 0)
                                .map(|i| names[i])
                                .collect();
                            if caps.is_empty() {
                                "None".into()
                            } else {
                                caps.join(", ")
                            }
                        }),
                        num("Connector Type (Legacy)", r, 23, 22, |v| v.to_string()),
                        reserved(r, 21, 11),
                        num("VCONN Power", r, 10, 8, |v| v.to_string()),
                        flag("VCONN Required", r, 7),
                        flag("VBUS Required", r, 6),
                        num("Alternate Modes", r, 5, 3, |v| format!("{v:#05b}")),
                        num("USB Highest Speed", r, 2, 0, usb_speed(false)),
                    ],
                )
            }
            ProductTypeVdo::Dfp(d) => {
                let r = d.raw;
                node(
                    "DFP VDO",
                    r,
                    vec![
                        num("DFP VDO Version", r, 31, 29, |v| format!("1.{v}")),
                        reserved(r, 28, 27),
                        num("Host Capability", r, 26, 24, |v| {
                            let names = ["USB2.0", "USB3.2", "USB4"];
                            let caps: Vec<&str> = (0..3)
                                .filter(|i| v & (1 << i) != 0)
                                .map(|i| names[i])
                                .collect();
                            if caps.is_empty() {
                                "None".into()
                            } else {
                                caps.join(", ")
                            }
                        }),
                        num("Connector Type (Legacy)", r, 23, 22, |v| v.to_string()),
                        reserved(r, 21, 5),
                        num("Port Number", r, 4, 0, |v| v.to_string()),
                    ],
                )
            }
            ProductTypeVdo::Other(raw) => raw_vdo(position, *raw),
        }
    }
}

fn raw_vdo(position: usize, raw: u32) -> Field {
    Field::new(
        format!("VDO {position}"),
        31,
        0,
        u64::from(raw),
        format!("{raw:#010X}"),
    )
}

impl DiscoverIdentity {
    /// Decode the VDOs after the VDM header of a Discover Identity ACK.
    fn decode(vdos: &[u32], sop: Sop) -> Option<DiscoverIdentity> {
        let (&id, rest) = vdos.split_first()?;
        let id_header = IdHeader::decode(id);
        let cert_stat = rest.first().copied();
        let product = rest.get(1).map(|&raw| ProductVdo {
            raw,
            pid: bits(raw, 31, 16) as u16,
            bcd_device: bits(raw, 15, 0) as u16,
        });
        let tail = rest.get(2..).unwrap_or_default();
        let cable = sop.is_cable();
        let product_type_vdos = tail
            .iter()
            .enumerate()
            .map(|(i, &raw)| product_type_vdo(&id_header, cable, i, raw))
            .collect();
        Some(DiscoverIdentity {
            id_header,
            cert_stat,
            product,
            product_type_vdos,
        })
    }

    fn fields(&self, cable: bool, out: &mut Vec<Field>) {
        out.push(self.id_header.field(cable));
        if let Some(x) = self.cert_stat {
            out.push(
                Field::new("Cert Stat VDO", 31, 0, u64::from(x), format!("{x:#010X}"))
                    .with_children(vec![num("XID", x, 31, 0, |v| format!("{v:#X}"))]),
            );
        }
        if let Some(p) = self.product {
            let r = p.raw;
            out.push(
                Field::new(
                    "Product VDO",
                    31,
                    0,
                    u64::from(r),
                    format!("PID {:#06X}", p.pid),
                )
                .with_children(vec![
                    num("USB Product ID", r, 31, 16, |v| format!("{v:#06X}")),
                    num("bcdDevice", r, 15, 0, |v| format!("{v:#06X}")),
                ]),
            );
        }
        // VDO positions count from 1 after the VDM header: ID 1, cert 2, product 3.
        out.extend(
            self.product_type_vdos
                .iter()
                .enumerate()
                .map(|(i, p)| p.field(i + 4)),
        );
    }
}

/// The product type VDO at `index` (0 = first after the Product VDO).
fn product_type_vdo(id: &IdHeader, cable: bool, index: usize, raw: u32) -> ProductTypeVdo {
    if cable {
        return match (id.product_type_ufp, index) {
            (3, 0) => ProductTypeVdo::PassiveCable(PassiveCableVdo {
                raw,
                plug_type: bits(raw, 19, 18) as u8,
                epr_capable: bit(raw, 17),
                latency: bits(raw, 16, 13) as u8,
                termination: bits(raw, 12, 11) as u8,
                max_vbus_voltage: bits(raw, 10, 9) as u8,
                vbus_current: bits(raw, 6, 5) as u8,
                usb_speed: bits(raw, 2, 0) as u8,
            }),
            (4, 0) => ProductTypeVdo::ActiveCable(ActiveCableVdo {
                raw,
                vdo_version: bits(raw, 23, 21) as u8,
                termination: bits(raw, 12, 11) as u8,
                vbus_current: bits(raw, 6, 5) as u8,
                vbus_through_cable: bit(raw, 4),
                sop_double_prime_present: bit(raw, 3),
                usb_speed: bits(raw, 2, 0) as u8,
            }),
            (4, 1) => ProductTypeVdo::ActiveCable2(ActiveCableVdo2 { raw }),
            _ => ProductTypeVdo::Other(raw),
        };
    }
    // SOP: a UFP VDO for a hub or peripheral, then (after a pad when both
    // are present) a DFP VDO for a hub, host or power brick.
    let has_ufp = matches!(id.product_type_ufp, 1 | 2);
    let has_dfp = matches!(id.product_type_dfp, 1..=3);
    let dfp_index = if has_ufp { 2 } else { 0 };
    if has_ufp && index == 0 {
        ProductTypeVdo::Ufp(UfpVdo {
            raw,
            vdo_version: bits(raw, 31, 29) as u8,
            device_capability: bits(raw, 27, 24) as u8,
            alternate_modes: bits(raw, 5, 3) as u8,
            usb_speed: bits(raw, 2, 0) as u8,
        })
    } else if has_dfp && index == dfp_index {
        ProductTypeVdo::Dfp(DfpVdo {
            raw,
            vdo_version: bits(raw, 31, 29) as u8,
            host_capability: bits(raw, 26, 24) as u8,
            port_number: bits(raw, 4, 0) as u8,
        })
    } else {
        ProductTypeVdo::Other(raw)
    }
}

impl Vdm {
    /// Decode a Vendor_Defined message from its data objects (`objects[0]`
    /// is the VDM header). `None` when there is no header.
    pub(crate) fn decode(objects: &[u32], sop: Sop) -> Option<Vdm> {
        let (&raw, vdos) = objects.split_first()?;
        let header = VdmHeader::decode(raw);
        let raw_body = || VdmBody::Objects(vdos.to_vec());
        let body = match header {
            VdmHeader::Structured(h) if h.command_type == CommandType::Ack => match h.command {
                VdmCommand::DiscoverIdentity => DiscoverIdentity::decode(vdos, sop)
                    .map(VdmBody::DiscoverIdentity)
                    .unwrap_or_else(raw_body),
                VdmCommand::DiscoverSvids => VdmBody::DiscoverSvids(
                    vdos.iter()
                        .flat_map(|v| [bits(*v, 31, 16) as u16, bits(*v, 15, 0) as u16])
                        .take_while(|&s| s != 0)
                        .collect(),
                ),
                VdmCommand::DiscoverModes => VdmBody::DiscoverModes(vdos.to_vec()),
                _ => raw_body(),
            },
            _ => raw_body(),
        };
        Some(Vdm { header, body })
    }

    /// One-line summary after the message type name.
    pub(crate) fn summary(&self) -> String {
        match &self.header {
            VdmHeader::Structured(h) => format!(
                "{} {} (SVID {:#06X})",
                h.command.name(),
                h.command_type.name(),
                h.svid
            ),
            VdmHeader::Unstructured(u) => format!("Unstructured (SVID {:#06X})", u.svid),
        }
    }

    /// The VDM header node, then one node per VDO.
    pub(crate) fn fields(&self, sop: Sop) -> Vec<Field> {
        let mut out = vec![self.header.field()];
        match &self.body {
            VdmBody::DiscoverIdentity(id) => id.fields(sop.is_cable(), &mut out),
            VdmBody::DiscoverSvids(svids) => {
                out.extend(svids.iter().enumerate().map(|(i, s)| {
                    // Two SVIDs per VDO: the first in bits 31..16.
                    let (hi, lo) = if i % 2 == 0 { (31, 16) } else { (15, 0) };
                    Field::new(
                        format!("SVID {}", i + 1),
                        hi,
                        lo,
                        u64::from(*s),
                        format!("{s:#06X}"),
                    )
                }));
            }
            VdmBody::DiscoverModes(modes) => {
                out.extend(modes.iter().enumerate().map(|(i, m)| {
                    Field::new(
                        format!("Mode {}", i + 1),
                        31,
                        0,
                        u64::from(*m),
                        format!("{m:#010X}"),
                    )
                }));
            }
            VdmBody::Objects(vdos) => {
                out.extend(vdos.iter().enumerate().map(|(i, v)| raw_vdo(i + 1, *v)));
            }
        }
        out
    }
}
