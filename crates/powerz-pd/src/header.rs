//! The 16-bit message header (USB PD r3.2 §6.2.1.1) and the names of every
//! control, data and extended message type.

use crate::field::{Field, Flag, yes_no};

/// SOP* kind of a message, from the meter's SOP code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sop {
    /// SOP: port to port.
    Sop,
    /// SOP': the cable plug next to the source (VCONN source).
    SopPrime,
    /// SOP'': the far cable plug.
    SopDoublePrime,
    /// SOP'_Debug.
    SopPrimeDebug,
    /// SOP''_Debug.
    SopDoublePrimeDebug,
    /// Any other code, kept raw.
    Other(u8),
}

impl Sop {
    /// Map the meter's SOP code (0 SOP, 1 SOP', 2 SOP'', 3 SOP'_Debug,
    /// 4 SOP''_Debug) to a kind.
    pub fn from_raw(code: u8) -> Sop {
        match code {
            0 => Sop::Sop,
            1 => Sop::SopPrime,
            2 => Sop::SopDoublePrime,
            3 => Sop::SopPrimeDebug,
            4 => Sop::SopDoublePrimeDebug,
            n => Sop::Other(n),
        }
    }

    /// True for the cable SOP kinds, where header bit 8 is Cable Plug and
    /// bit 5 is reserved.
    pub fn is_cable(self) -> bool {
        matches!(
            self,
            Sop::SopPrime | Sop::SopDoublePrime | Sop::SopPrimeDebug | Sop::SopDoublePrimeDebug
        )
    }
}

/// Port Power Role (bit 8 on SOP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PowerRole {
    /// 0.
    Sink,
    /// 1.
    Source,
}

/// Port Data Role (bit 5 on SOP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataRole {
    /// 0.
    Ufp,
    /// 1.
    Dfp,
}

/// Cable Plug (bit 8 on SOP'/SOP'').
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CablePlug {
    /// 0: sent by a DFP or UFP port.
    Port,
    /// 1: sent by a cable plug or VPD.
    CablePlug,
}

/// Specification Revision (bits 7..6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpecRevision {
    /// 00b: revision 1.0.
    Rev1,
    /// 01b: revision 2.0.
    Rev2,
    /// 10b: revision 3.x.
    Rev3,
    /// 11b: reserved.
    Reserved,
}

impl SpecRevision {
    fn from_bits(bits: u16) -> SpecRevision {
        match bits & 3 {
            0 => SpecRevision::Rev1,
            1 => SpecRevision::Rev2,
            2 => SpecRevision::Rev3,
            _ => SpecRevision::Reserved,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SpecRevision::Rev1 => "Revision 1.0",
            SpecRevision::Rev2 => "Revision 2.0",
            SpecRevision::Rev3 => "Revision 3.x",
            SpecRevision::Reserved => "Reserved",
        }
    }
}

/// Message family: a control message has no data objects, a data message
/// has some, an extended message has the Extended bit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageKind {
    /// Control message.
    Control,
    /// Data message.
    Data,
    /// Extended message.
    Extended,
}

/// A message type: family plus 5-bit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageType {
    kind: MessageKind,
    code: u8,
}

impl MessageType {
    /// The type `code` (low 5 bits) of family `kind`.
    pub fn new(kind: MessageKind, code: u8) -> MessageType {
        MessageType {
            kind,
            code: code & 0x1F,
        }
    }

    /// Message family.
    pub fn kind(self) -> MessageKind {
        self.kind
    }

    /// 5-bit type code.
    pub fn code(self) -> u8 {
        self.code
    }

    /// Spec name of the type, or "Reserved".
    pub fn name(self) -> &'static str {
        self.lookup().unwrap_or("Reserved")
    }

    /// True when revision 3.2 defines no message for this code.
    pub fn is_reserved(self) -> bool {
        self.lookup().is_none()
    }

    fn lookup(self) -> Option<&'static str> {
        match self.kind {
            MessageKind::Control => control_name(self.code),
            MessageKind::Data => data_name(self.code),
            MessageKind::Extended => extended_name(self.code),
        }
    }
}

/// Control message types (Table 6.5).
fn control_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "GoodCRC",
        0x02 => "GotoMin",
        0x03 => "Accept",
        0x04 => "Reject",
        0x05 => "Ping",
        0x06 => "PS_RDY",
        0x07 => "Get_Source_Cap",
        0x08 => "Get_Sink_Cap",
        0x09 => "DR_Swap",
        0x0A => "PR_Swap",
        0x0B => "VCONN_Swap",
        0x0C => "Wait",
        0x0D => "Soft_Reset",
        0x0E => "Data_Reset",
        0x0F => "Data_Reset_Complete",
        0x10 => "Not_Supported",
        0x11 => "Get_Source_Cap_Extended",
        0x12 => "Get_Status",
        0x13 => "FR_Swap",
        0x14 => "Get_PPS_Status",
        0x15 => "Get_Country_Codes",
        0x16 => "Get_Sink_Cap_Extended",
        0x17 => "Get_Source_Info",
        0x18 => "Get_Revision",
        _ => return None,
    })
}

/// Data message types (Table 6.6).
fn data_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "Source_Capabilities",
        0x02 => "Request",
        0x03 => "BIST",
        0x04 => "Sink_Capabilities",
        0x05 => "Battery_Status",
        0x06 => "Alert",
        0x07 => "Get_Country_Info",
        0x08 => "Enter_USB",
        0x09 => "EPR_Request",
        0x0A => "EPR_Mode",
        0x0B => "Source_Info",
        0x0C => "Revision",
        0x0F => "Vendor_Defined",
        _ => return None,
    })
}

/// Extended message types (Table 6.53).
fn extended_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "Source_Capabilities_Extended",
        0x02 => "Status",
        0x03 => "Get_Battery_Cap",
        0x04 => "Get_Battery_Status",
        0x05 => "Battery_Capabilities",
        0x06 => "Get_Manufacturer_Info",
        0x07 => "Manufacturer_Info",
        0x08 => "Security_Request",
        0x09 => "Security_Response",
        0x0A => "Firmware_Update_Request",
        0x0B => "Firmware_Update_Response",
        0x0C => "PPS_Status",
        0x0D => "Country_Info",
        0x0E => "Country_Codes",
        0x0F => "Sink_Capabilities_Extended",
        0x10 => "Extended_Control",
        0x11 => "EPR_Source_Capabilities",
        0x12 => "EPR_Sink_Capabilities",
        0x1E => "Vendor_Defined_Extended",
        _ => return None,
    })
}

/// A decoded message header.
///
/// Bit 8 is Port Power Role on SOP and Cable Plug on SOP'/SOP''; bit 5 is
/// Port Data Role on SOP and reserved on SOP'/SOP''. The variant that does
/// not apply is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The 16 header bits.
    pub raw: u16,
    /// Extended (bit 15).
    pub extended: bool,
    /// Number of Data Objects (bits 14..12).
    pub num_data_objects: u8,
    /// MessageID (bits 11..9).
    pub message_id: u8,
    /// Port Power Role (bit 8, SOP only).
    pub port_power_role: Option<PowerRole>,
    /// Cable Plug (bit 8, SOP'/SOP'' only).
    pub cable_plug: Option<CablePlug>,
    /// Specification Revision (bits 7..6).
    pub spec_revision: SpecRevision,
    /// Port Data Role (bit 5, SOP only).
    pub port_data_role: Option<DataRole>,
    /// Message Type (bits 4..0) with its family.
    pub message_type: MessageType,
    /// The SOP kind the header was read on.
    pub sop: Sop,
}

impl Header {
    /// Decode a header received on `sop`.
    pub fn decode(raw: u16, sop: Sop) -> Header {
        let extended = raw & 0x8000 != 0;
        let num_data_objects = ((raw >> 12) & 7) as u8;
        let bit8 = raw & 0x0100 != 0;
        let bit5 = raw & 0x0020 != 0;
        let kind = if extended {
            MessageKind::Extended
        } else if num_data_objects == 0 {
            MessageKind::Control
        } else {
            MessageKind::Data
        };
        let cable = sop.is_cable();
        Header {
            raw,
            extended,
            num_data_objects,
            message_id: ((raw >> 9) & 7) as u8,
            port_power_role: (!cable).then_some(if bit8 {
                PowerRole::Source
            } else {
                PowerRole::Sink
            }),
            cable_plug: cable.then_some(if bit8 {
                CablePlug::CablePlug
            } else {
                CablePlug::Port
            }),
            spec_revision: SpecRevision::from_bits(raw >> 6),
            port_data_role: (!cable).then_some(if bit5 { DataRole::Dfp } else { DataRole::Ufp }),
            message_type: MessageType::new(kind, (raw & 0x1F) as u8),
            sop,
        }
    }

    /// The "Header" node of the field tree, one child per header field.
    pub fn field(&self) -> Field {
        let raw = self.raw;
        let bit = |n: u16| u64::from((raw >> n) & 1);
        let mut children = vec![
            Field::new("Extended", 15, 15, bit(15), yes_no(self.extended)),
            Field::new(
                "Number of Data Objects",
                14,
                12,
                u64::from(self.num_data_objects),
                self.num_data_objects.to_string(),
            ),
            Field::new(
                "MessageID",
                11,
                9,
                u64::from(self.message_id),
                self.message_id.to_string(),
            ),
        ];
        match (self.port_power_role, self.cable_plug) {
            (_, Some(plug)) => children.push(Field::new(
                "Cable Plug",
                8,
                8,
                bit(8),
                match plug {
                    CablePlug::Port => "From a DFP or UFP port",
                    CablePlug::CablePlug => "From a Cable Plug or VPD",
                },
            )),
            (role, None) => children.push(Field::new(
                "Port Power Role",
                8,
                8,
                bit(8),
                match role {
                    Some(PowerRole::Source) => "Source",
                    _ => "Sink",
                },
            )),
        }
        children.push(
            Field::new(
                "Specification Revision",
                7,
                6,
                u64::from((raw >> 6) & 3),
                self.spec_revision.label(),
            )
            .flagged((self.spec_revision == SpecRevision::Reserved).then_some(Flag::Reserved)),
        );
        match self.port_data_role {
            Some(role) => children.push(Field::new(
                "Port Data Role",
                5,
                5,
                bit(5),
                match role {
                    DataRole::Dfp => "DFP",
                    DataRole::Ufp => "UFP",
                },
            )),
            None => children.push(
                Field::new("Reserved", 5, 5, bit(5), "Reserved (shall be 0)")
                    .flagged((bit(5) != 0).then_some(Flag::Reserved)),
            ),
        }
        let ty = self.message_type;
        children.push(
            Field::new(
                "Message Type",
                4,
                0,
                u64::from(ty.code()),
                format!("{} ({:?})", ty.name(), ty.kind()),
            )
            .flagged(ty.is_reserved().then_some(Flag::Reserved)),
        );
        Field::new(
            "Header",
            15,
            0,
            u64::from(raw),
            format!("{:#06X} {}", raw, ty.name()),
        )
        .with_children(children)
    }
}
