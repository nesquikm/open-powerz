//! Extended message header (USB PD r3.2 §6.2.1.2) and the small typed data
//! objects and blocks: Extended_Control and EPR_Mode.

use crate::field::{Field, reserved, yes_no};

/// The 16-bit extended message header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedHeader {
    /// The 16 header bits.
    pub raw: u16,
    /// Chunked (bit 15).
    pub chunked: bool,
    /// Chunk Number (bits 14..11).
    pub chunk_number: u8,
    /// Request Chunk (bit 10).
    pub request_chunk: bool,
    /// Data Size, bytes (bits 8..0).
    pub data_size: u16,
}

impl ExtendedHeader {
    /// Decode the 16 extended header bits.
    pub fn decode(raw: u16) -> ExtendedHeader {
        ExtendedHeader {
            raw,
            chunked: raw & 0x8000 != 0,
            chunk_number: ((raw >> 11) & 0xF) as u8,
            request_chunk: raw & 0x0400 != 0,
            data_size: raw & 0x1FF,
        }
    }

    /// The "Extended Header" node of the field tree.
    pub fn field(&self) -> Field {
        let raw = self.raw;
        Field::new(
            "Extended Header",
            15,
            0,
            u64::from(raw),
            format!("{raw:#06X}"),
        )
        .with_children(vec![
            Field::new(
                "Chunked",
                15,
                15,
                u64::from(self.chunked),
                yes_no(self.chunked),
            ),
            Field::new(
                "Chunk Number",
                14,
                11,
                u64::from(self.chunk_number),
                self.chunk_number.to_string(),
            ),
            Field::new(
                "Request Chunk",
                10,
                10,
                u64::from(self.request_chunk),
                yes_no(self.request_chunk),
            ),
            reserved(u32::from(raw), 9, 9),
            Field::new(
                "Data Size",
                8,
                0,
                u64::from(self.data_size),
                format!("{} bytes", self.data_size),
            ),
        ])
    }
}

/// Type of an Extended_Control message (ECDB byte 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcdbType {
    /// 1: EPR_Get_Source_Cap.
    EprGetSourceCap,
    /// 2: EPR_Get_Sink_Cap.
    EprGetSinkCap,
    /// 3: EPR_KeepAlive.
    EprKeepAlive,
    /// 4: EPR_KeepAlive_Ack.
    EprKeepAliveAck,
    /// Any other value.
    Reserved(u8),
}

/// An Extended_Control message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedControl {
    /// ECDB Type.
    pub kind: EcdbType,
    /// ECDB Data.
    pub data: u8,
}

/// Action of an EPR_Mode message (EPRMDO bits 31..24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EprAction {
    /// 1: Enter.
    Enter,
    /// 2: Enter Acknowledged.
    EnterAcknowledged,
    /// 3: Enter Succeeded.
    EnterSucceeded,
    /// 4: Enter Failed.
    EnterFailed,
    /// 5: Exit.
    Exit,
    /// Any other value.
    Reserved(u8),
}

/// An EPR_Mode message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EprMode {
    /// Action.
    pub action: EprAction,
    /// Data (bits 23..16).
    pub data: u8,
}

impl EcdbType {
    /// Spec name of the type, or "Reserved".
    pub fn name(self) -> &'static str {
        match self {
            EcdbType::EprGetSourceCap => "EPR_Get_Source_Cap",
            EcdbType::EprGetSinkCap => "EPR_Get_Sink_Cap",
            EcdbType::EprKeepAlive => "EPR_KeepAlive",
            EcdbType::EprKeepAliveAck => "EPR_KeepAlive_Ack",
            EcdbType::Reserved(_) => "Reserved",
        }
    }

    /// The ECDB Type byte value.
    pub(crate) fn code(self) -> u8 {
        match self {
            EcdbType::EprGetSourceCap => 1,
            EcdbType::EprGetSinkCap => 2,
            EcdbType::EprKeepAlive => 3,
            EcdbType::EprKeepAliveAck => 4,
            EcdbType::Reserved(n) => n,
        }
    }
}

impl ExtendedControl {
    /// The ECDB Type for byte value `v`.
    pub(crate) fn kind_of(v: u8) -> EcdbType {
        match v {
            1 => EcdbType::EprGetSourceCap,
            2 => EcdbType::EprGetSinkCap,
            3 => EcdbType::EprKeepAlive,
            4 => EcdbType::EprKeepAliveAck,
            n => EcdbType::Reserved(n),
        }
    }

    /// Decode an ECDB (Type, Data); `None` when the block is shorter than
    /// two bytes.
    pub fn decode(data: &[u8]) -> Option<ExtendedControl> {
        match data {
            [ty, data, ..] => Some(ExtendedControl {
                kind: ExtendedControl::kind_of(*ty),
                data: *data,
            }),
            _ => None,
        }
    }

    /// The two ECDB bytes (Type, Data).
    pub(crate) fn bytes(self) -> [u8; 2] {
        [self.kind.code(), self.data]
    }
}

impl EprAction {
    /// Spec name of the action, or "Reserved".
    pub fn name(self) -> &'static str {
        match self {
            EprAction::Enter => "Enter",
            EprAction::EnterAcknowledged => "Enter_Acknowledged",
            EprAction::EnterSucceeded => "Enter_Succeeded",
            EprAction::EnterFailed => "Enter_Failed",
            EprAction::Exit => "Exit",
            EprAction::Reserved(_) => "Reserved",
        }
    }

    /// The Action byte value.
    pub(crate) fn code(self) -> u8 {
        match self {
            EprAction::Enter => 1,
            EprAction::EnterAcknowledged => 2,
            EprAction::EnterSucceeded => 3,
            EprAction::EnterFailed => 4,
            EprAction::Exit => 5,
            EprAction::Reserved(n) => n,
        }
    }
}

impl EprMode {
    /// Decode an EPR Mode data object.
    pub fn decode(dobj: u32) -> EprMode {
        let action = match (dobj >> 24) as u8 {
            1 => EprAction::Enter,
            2 => EprAction::EnterAcknowledged,
            3 => EprAction::EnterSucceeded,
            4 => EprAction::EnterFailed,
            5 => EprAction::Exit,
            n => EprAction::Reserved(n),
        };
        EprMode {
            action,
            data: (dobj >> 16) as u8,
        }
    }

    /// The EPR Mode data object (reserved bits 15..0 read as zero).
    pub(crate) fn object(self) -> u32 {
        u32::from(self.action.code()) << 24 | u32::from(self.data) << 16
    }
}
