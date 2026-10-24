//! Custom PD messages for `pd cmd=` / `pd data=` (FR DGSQ6T).
//!
//! A [`PdMessage`] is a typed PD control or data message for SOP, SOP' or
//! SOP''. [`PdMessage::to_command`] renders it as the device's command:
//! a control message on SOP as `pd cmd=<type>`, everything else as
//! `pd data=<hex>` (SOP byte, PD header LE, data objects LE). \[doc\]
//! The firmware only uses the header's Extended, Number of Data Objects and
//! Message Type fields; Message ID and the roles come from its own state
//! machine. \[doc\]
//!
//! GoodCRC (control type 1) is never sent: the meter's PD stack sends it
//! itself, and a hand-made one has "strange effects". \[doc\]
//!
//! Cable replies (a Discover Identity ACK on SOP' / SOP'') decode into a
//! [`CableIdentity`] with the PD decoder.

use std::io::{Read, Write};
use std::time::Duration;

use super::Error;
use super::command::Command;
use super::reply::ReplyItem;
use super::session::PdmSession;
use crate::pd::{
    Capture, CommandType, DiscoverIdentity, Input, Message as PdBody, ProductTypeVdo, Vdm, VdmBody,
    VdmCommand, VdmHeader,
};
use crate::session::Clock;

/// Control message type GoodCRC. \[doc\]
const GOOD_CRC: u8 = 1;
/// How long a cable gets to answer Discover Identity.
pub const CABLE_WAIT: Duration = Duration::from_millis(1500);
/// PD header bit 15: Extended.
const HEADER_EXTENDED: u16 = 0x8000;
/// PD header bits 6..7 = 0b10: Spec Revision 3.0.
const HEADER_SPEC_REV_3: u16 = 0x0080;

/// Which SOP* a message is addressed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SopTarget {
    /// SOP: the port partner (the charger).
    Sop,
    /// SOP': the cable plug next to the meter.
    SopPrime,
    /// SOP'': the far cable plug.
    SopDoublePrime,
}

impl SopTarget {
    /// The `pd data=` SOP byte: 0 SOP, 1 SOP', 2 SOP''. \[doc\]
    pub fn code(self) -> u8 {
        match self {
            SopTarget::Sop => 0,
            SopTarget::SopPrime => 1,
            SopTarget::SopDoublePrime => 2,
        }
    }

    /// Parse a CLI name: `sop`, `sop1` (SOP'), `sop2` (SOP'').
    pub fn from_name(name: &str) -> Option<SopTarget> {
        match name {
            "sop" => Some(SopTarget::Sop),
            "sop1" => Some(SopTarget::SopPrime),
            "sop2" => Some(SopTarget::SopDoublePrime),
            _ => None,
        }
    }
}

/// Why the builder refused a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessageError {
    /// GoodCRC is sent by the meter's own PD stack, never by hand.
    #[error("refusing GoodCRC: the meter's PD stack sends it itself")]
    GoodCrc,
    /// A data message needs at least one data object.
    #[error("a data message needs at least one data object")]
    NoDataObjects,
    /// More than 7 data objects.
    #[error("{count} data objects: a PD message carries at most 7")]
    TooManyDataObjects {
        /// How many were given.
        count: usize,
    },
    /// The message type does not fit the 5-bit field.
    #[error("message type {ty} does not fit the 5-bit field")]
    BadType {
        /// The type given.
        ty: u8,
    },
    /// A control message type USB PD reserves (0, 0x19..=0x1F).
    #[error("control message type {ty} is reserved in USB PD")]
    ReservedControlType {
        /// The type given.
        ty: u8,
    },
    /// The hex text is not a PD header plus whole data objects.
    #[error("not a PD message in hex: {reason}")]
    BadHex {
        /// What was wrong.
        reason: String,
    },
}

/// A PD control or data message to send through the meter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdMessage {
    /// Where it goes.
    pub sop: SopTarget,
    /// The Message Type (5 bits): a control type when `data_objects` is
    /// empty, a data type otherwise.
    pub msg_type: u8,
    /// The data objects, empty for a control message.
    pub data_objects: Vec<u32>,
}

impl PdMessage {
    /// A control message (no data objects).
    pub fn control(sop: SopTarget, msg_type: u8) -> PdMessage {
        PdMessage {
            sop,
            msg_type,
            data_objects: Vec::new(),
        }
    }

    /// A data message with `data_objects`.
    pub fn data(sop: SopTarget, msg_type: u8, data_objects: Vec<u32>) -> PdMessage {
        PdMessage {
            sop,
            msg_type,
            data_objects,
        }
    }

    /// Parse `hex` (PD header LE, then data objects LE; no SOP byte) for
    /// `sop`. Only the header's Message Type and Number of Data Objects
    /// are kept.
    pub fn from_hex(sop: SopTarget, hex: &str) -> Result<PdMessage, MessageError> {
        let bad = |reason: &str| MessageError::BadHex {
            reason: reason.to_string(),
        };
        let hex = hex.trim();
        if !hex.len().is_multiple_of(2) {
            return Err(bad("odd number of hex digits"));
        }
        let bytes = parse_hex_bytes(hex).ok_or_else(|| bad("not hex"))?;
        let [lo, hi, rest @ ..] = bytes.as_slice() else {
            return Err(bad("shorter than the 2-byte PD header"));
        };
        let header = u16::from_le_bytes([*lo, *hi]);
        if header & HEADER_EXTENDED != 0 {
            return Err(bad("extended messages are not supported"));
        }
        let count = usize::from((header >> 12) & 0x7);
        if rest.len() != count * 4 {
            return Err(MessageError::BadHex {
                reason: format!(
                    "the header says {count} data objects, {} bytes follow",
                    rest.len()
                ),
            });
        }
        let data_objects = rest
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        Ok(PdMessage {
            sop,
            msg_type: (header & 0x1F) as u8,
            data_objects,
        })
    }

    /// A Vendor_Defined (data type 15) message to a cable plug (SOP' /
    /// SOP'') whose first object is a structured VDM header for a Discover
    /// Identity REQ, whatever its SVID, version or other objects: the
    /// cable's ACK is worth waiting for.
    pub fn is_cable_discover_identity(&self) -> bool {
        const VENDOR_DEFINED: u8 = 15;
        if self.sop == SopTarget::Sop || self.msg_type != VENDOR_DEFINED {
            return false;
        }
        matches!(
            self.data_objects.first().map(|o| VdmHeader::decode(*o)),
            Some(VdmHeader::Structured(h))
                if h.command_type == CommandType::Req && h.command == VdmCommand::DiscoverIdentity
        )
    }

    /// The 16-bit PD header the message is rendered with.
    pub fn header(&self) -> u16 {
        // Only Extended, Number of Data Objects and Message Type matter to
        // the firmware; Spec Revision 3.0 as in the doc's examples. [doc]
        let count = (self.data_objects.len().min(7) as u16) << 12;
        count | HEADER_SPEC_REV_3 | (u16::from(self.msg_type) & 0x1F)
    }

    /// Render as the device's command; GoodCRC and malformed messages are
    /// refused with a reason.
    pub fn to_command(&self) -> Result<Command, MessageError> {
        if self.msg_type > 0x1F {
            return Err(MessageError::BadType { ty: self.msg_type });
        }
        if self.data_objects.is_empty() {
            match self.msg_type {
                GOOD_CRC => return Err(MessageError::GoodCrc),
                // Control messages defined by USB PD r3.x; GotoMin (2) is
                // deprecated, so type 2 without objects is a Request
                // missing its RDO.
                3..=0x18 => {}
                0 | 0x19..=0x1F => {
                    return Err(MessageError::ReservedControlType { ty: self.msg_type });
                }
                _ => return Err(MessageError::NoDataObjects),
            }
        }
        if self.data_objects.len() > 7 {
            return Err(MessageError::TooManyDataObjects {
                count: self.data_objects.len(),
            });
        }
        if self.data_objects.is_empty() && self.sop == SopTarget::Sop {
            return Ok(Command::PdCmd(self.msg_type));
        }
        let mut bytes = Vec::with_capacity(3 + 4 * self.data_objects.len());
        bytes.push(self.sop.code());
        bytes.extend(self.header().to_le_bytes());
        for object in &self.data_objects {
            bytes.extend(object.to_le_bytes());
        }
        Ok(Command::PdData(bytes))
    }
}

/// Built-in messages offered by the builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Discover Identity REQ to the cable plug (SOP').
    DiscoverIdentityCable,
}

impl Preset {
    /// Every preset.
    pub const ALL: [Preset; 1] = [Preset::DiscoverIdentityCable];

    /// The CLI name (`discover-identity-cable`).
    pub fn name(self) -> &'static str {
        match self {
            Preset::DiscoverIdentityCable => "discover-identity-cable",
        }
    }

    /// Parse a CLI name.
    pub fn from_name(name: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The message the preset sends.
    pub fn message(self) -> PdMessage {
        match self {
            // Vendor_Defined (data type 15) with one structured VDM header:
            // SVID 0xFF00 (PD SID), structured, VDM version 2.1, REQ,
            // Discover Identity. [doc]
            Preset::DiscoverIdentityCable => {
                PdMessage::data(SopTarget::SopPrime, 15, vec![0xFF00_A801])
            }
        }
    }
}

/// A cable's e-marker details from its Discover Identity ACK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CableIdentity {
    /// The plug that answered.
    pub sop: SopTarget,
    /// USB Vendor ID (ID Header).
    pub vid: u16,
    /// USB Product ID (Product VDO), when present.
    pub pid: Option<u16>,
    /// Passive (`false`) or active (`true`) cable.
    pub active: bool,
    /// Maximum VBUS voltage, V (passive cable VDO), when known.
    pub max_voltage_v: Option<u32>,
    /// VBUS current handling, mA (3000 or 5000), when known.
    pub max_current_ma: Option<u32>,
    /// EPR capable (passive cable VDO).
    pub epr_capable: bool,
    /// The decoded identity, for the field tree.
    pub identity: DiscoverIdentity,
}

/// Maximum VBUS Voltage field (bits 10..9) of a cable VDO, in V:
/// 20, 30, 40, 50. \[doc\] (USB PD r3.2 Passive / Active Cable VDO)
fn cable_max_voltage_v(raw: u32) -> u32 {
    match (raw >> 9) & 0x3 {
        0 => 20,
        1 => 30,
        2 => 40,
        _ => 50,
    }
}

/// VBUS Current Handling Capability (bits 6..5) of a cable VDO, in mA:
/// 1 = 3 A, 2 = 5 A, others reserved. \[doc\]
fn cable_current_ma(code: u8) -> Option<u32> {
    match code {
        1 => Some(3000),
        2 => Some(5000),
        _ => None,
    }
}

/// SOP' Product Type (ID Header bits 29..27): passive cable. \[doc\]
const PRODUCT_PASSIVE_CABLE: u8 = 3;
/// SOP' Product Type: active cable. \[doc\]
const PRODUCT_ACTIVE_CABLE: u8 = 4;

/// Decode one PD message (meter SOP code, header, bytes after the header)
/// as a cable's Discover Identity ACK; `None` for anything else.
pub fn cable_identity_from_message(sop: u8, header: u16, body: &[u8]) -> Option<CableIdentity> {
    let target = match sop {
        1 => SopTarget::SopPrime,
        2 => SopTarget::SopDoublePrime,
        _ => return None,
    };
    let decoded = Capture::new().feed(Input::Message { sop, header, body })?;
    let PdBody::Vdm(Vdm {
        body: VdmBody::DiscoverIdentity(identity),
        ..
    }) = decoded.message
    else {
        return None;
    };
    let product_type = identity.id_header.product_type_ufp;
    if !matches!(product_type, PRODUCT_PASSIVE_CABLE | PRODUCT_ACTIVE_CABLE) {
        return None;
    }
    let (max_voltage_v, max_current_ma, epr_capable) = match identity.product_type_vdos.first() {
        Some(ProductTypeVdo::PassiveCable(c)) => (
            Some(cable_max_voltage_v(c.raw)),
            cable_current_ma(c.vbus_current),
            c.epr_capable,
        ),
        // Active Cable VDO 1 keeps Maximum VBUS Voltage in bits 10..9 and
        // EPR Capable in bit 17 like the passive one. [doc]
        Some(ProductTypeVdo::ActiveCable(c)) => (
            Some(cable_max_voltage_v(c.raw)),
            cable_current_ma(c.vbus_current),
            c.raw & (1 << 17) != 0,
        ),
        _ => (None, None, false),
    };
    Some(CableIdentity {
        sop: target,
        vid: identity.id_header.vid,
        pid: identity.product.map(|p| p.pid),
        active: product_type == PRODUCT_ACTIVE_CABLE,
        max_voltage_v,
        max_current_ma,
        epr_capable,
        identity,
    })
}

/// Parse a received PD message line: `pd data=<hex>` (or bare hex) with
/// the SOP byte, the header LE, then the data objects LE. The meter's
/// receive form may repeat the two header bytes and append a 4-byte CRC
/// (the vendor doc's receive example); both are told apart by length.
/// \[inferred\] (receive form not yet checked on a device)
fn received_message(line: &str) -> Option<(u8, u16, Vec<u8>)> {
    let hex = line.trim();
    let hex = hex.strip_prefix("pd data=").unwrap_or(hex).trim();
    let bytes = parse_hex_bytes(hex)?;
    let [sop, lo, hi, rest @ ..] = bytes.as_slice() else {
        return None;
    };
    let header = u16::from_le_bytes([*lo, *hi]);
    let objects = usize::from((header >> 12) & 0x7) * 4;
    let body = match rest.len().checked_sub(objects)? {
        // Objects only, or objects + CRC.
        0 | 4 => rest,
        // Repeated header, then objects (+ CRC).
        2 | 6 if rest.get(..2) == Some([*lo, *hi].as_slice()) => rest.get(2..)?,
        _ => return None,
    };
    Some((*sop, header, body.get(..objects)?.to_vec()))
}

/// Read replies until a cable's Discover Identity ACK arrives; `Ok(None)`
/// when none came within `timeout`.
pub fn await_cable_identity<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    timeout: Duration,
) -> Result<Option<CableIdentity>, Error> {
    let deadline = session
        .now_ms()
        .saturating_add(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX));
    loop {
        let now = session.now_ms();
        if now >= deadline {
            return Ok(None);
        }
        match session.next_item(Duration::from_millis(deadline - now)) {
            Ok(ReplyItem::Line(line)) => {
                if let Some(id) = received_message(&line)
                    .and_then(|(sop, header, body)| cable_identity_from_message(sop, header, &body))
                {
                    return Ok(Some(id));
                }
            }
            Ok(ReplyItem::Block(_)) => {}
            Err(Error::Timeout { .. }) => return Ok(None),
            Err(e) => return Err(e),
        }
    }
}

/// Hex digits (either case, nothing else: no sign, no `0x`, no spaces) as
/// bytes; `None` for an odd count or any other character. Empty text is no
/// bytes.
pub fn parse_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    hex.as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|p| u8::from_str_radix(p, 16).ok())
        })
        .collect()
}

/// Why `command` is never sent (GoodCRC as `pd cmd=1` or as a `pd data`
/// control message, in any spelling the meter might accept), or `None`
/// when it may be.
pub fn refusal(command: &Command) -> Option<String> {
    match command {
        Command::PdCmd(ty) => pd_cmd_refusal(u64::from(*ty)),
        Command::PdData(bytes) => pd_data_refusal(bytes),
        Command::Raw(text) => raw_refusal(text),
        _ => None,
    }
}

/// `pd cmd=<n>`: GoodCRC when the low 5 bits (the Message Type field) are 1,
/// so a value the firmware might truncate is refused too. \[inferred\]
fn pd_cmd_refusal(n: u64) -> Option<String> {
    (n & 0x1F == u64::from(GOOD_CRC)).then(|| MessageError::GoodCrc.to_string())
}

/// `pd data=`: SOP byte, then the header LE; a control message (no data
/// objects, not extended) of type GoodCRC is refused, and so is anything
/// too short to carry a header (fail closed).
fn pd_data_refusal(bytes: &[u8]) -> Option<String> {
    let [_, lo, hi, ..] = bytes else {
        return Some(
            "a pd data line shorter than a SOP byte and a PD header: cannot tell whether \
             it is GoodCRC, which is never sent by hand"
                .to_owned(),
        );
    };
    let header = u16::from_le_bytes([*lo, *hi]);
    let good_crc = header & HEADER_EXTENDED == 0
        && (header >> 12) & 0x7 == 0
        && header & 0x1F == u16::from(GOOD_CRC);
    good_crc.then(|| MessageError::GoodCrc.to_string())
}

/// A raw line the parser did not recognise: normalised (lowercase, no
/// whitespace), a `pd cmd` / `pd data` line is judged by its value read
/// leniently; one whose value cannot be read with confidence is refused,
/// since the firmware's own parsing of it is unknown. \[inferred\]
fn raw_refusal(text: &str) -> Option<String> {
    let norm: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if let Some(rest) = norm.strip_prefix("pdcmd") {
        return match rest.strip_prefix('=').and_then(lenient_number) {
            Some(n) => pd_cmd_refusal(n),
            None => Some(unsure(text)),
        };
    }
    if let Some(rest) = norm.strip_prefix("pddata") {
        return match rest.strip_prefix('=').and_then(parse_hex_bytes) {
            Some(bytes) => pd_data_refusal(&bytes),
            None => Some(unsure(text)),
        };
    }
    None
}

/// A non-negative number: decimal (leading zeros and a `+` allowed) or
/// `0x` hex; `None` for anything else (a `-` sign included).
fn lenient_number(s: &str) -> Option<u64> {
    let s = s.strip_prefix('+').unwrap_or(s);
    let (digits, radix) = match s.strip_prefix("0x") {
        Some(hex) => (hex, 16),
        None => (s, 10),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

/// The reason for refusing a `pd cmd` / `pd data` line that may be GoodCRC.
fn unsure(text: &str) -> String {
    format!(
        "cannot tell whether `{text}` is GoodCRC, which is never sent by hand; \
         write it as `pd cmd=<decimal>` or `pd data=<HEX>`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_of(command: &Command) -> String {
        let text = String::from_utf8(command.render()).unwrap();
        text.strip_prefix("pd data=")
            .unwrap_or_else(|| panic!("not pd data: {text}"))
            .to_string()
    }

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    // ---- AC-DGSQ6T.1: control and data messages for SOP, SOP', SOP'' ----

    #[test]
    fn sop_codes_follow_the_doc() {
        assert_eq!(SopTarget::Sop.code(), 0);
        assert_eq!(SopTarget::SopPrime.code(), 1);
        assert_eq!(SopTarget::SopDoublePrime.code(), 2);
    }

    #[test]
    fn a_control_message_on_sop_is_pd_cmd() {
        // Soft_Reset (13) on SOP.
        let c = PdMessage::control(SopTarget::Sop, 13).to_command().unwrap();
        assert_eq!(c, Command::PdCmd(13));
        assert_eq!(c.render(), b"pd cmd=13");
    }

    #[test]
    fn a_control_message_to_a_cable_is_pd_data_with_its_sop_byte() {
        let c = PdMessage::control(SopTarget::SopPrime, 13)
            .to_command()
            .unwrap();
        // The doc's soft reset is `008D00`; on SOP' the SOP byte is 01.
        assert_eq!(hex_of(&c), "018D00");
        let c = PdMessage::control(SopTarget::SopDoublePrime, 13)
            .to_command()
            .unwrap();
        assert_eq!(hex_of(&c), "028D00");
    }

    #[test]
    fn a_data_message_renders_header_and_objects_little_endian_in_uppercase() {
        // Request, fixed 9 V, RDO position 2 (the doc's `0082102CB10423`).
        let c = PdMessage::data(SopTarget::Sop, 2, vec![0x2304_B12C])
            .to_command()
            .unwrap();
        assert_eq!(hex_of(&c), "0082102CB10423");
    }

    #[test]
    fn the_header_carries_type_and_object_count_and_no_extended_bit() {
        let m = PdMessage::data(SopTarget::SopPrime, 15, vec![1, 2, 3]);
        let h = m.header();
        assert_eq!(h & 0x1F, 15, "message type");
        assert_eq!((h >> 12) & 7, 3, "number of data objects");
        assert_eq!(h & 0x8000, 0, "not extended");
        assert_eq!(PdMessage::control(SopTarget::Sop, 3).header() >> 12 & 7, 0);
    }

    #[test]
    fn hex_input_round_trips_through_the_typed_message() {
        let m = PdMessage::from_hex(SopTarget::Sop, "82102CB10423").unwrap();
        assert_eq!(m.msg_type, 2);
        assert_eq!(m.data_objects, vec![0x2304_B12C]);
        let m = PdMessage::from_hex(SopTarget::SopPrime, "8d00").unwrap();
        assert_eq!(m, PdMessage::control(SopTarget::SopPrime, 13));
    }

    #[test]
    fn malformed_messages_are_refused() {
        assert_eq!(
            PdMessage::data(SopTarget::Sop, 2, vec![]).to_command(),
            Err(MessageError::NoDataObjects)
        );
        assert_eq!(
            PdMessage::data(SopTarget::Sop, 15, vec![0; 8]).to_command(),
            Err(MessageError::TooManyDataObjects { count: 8 })
        );
        assert_eq!(
            PdMessage::control(SopTarget::Sop, 32).to_command(),
            Err(MessageError::BadType { ty: 32 })
        );
        assert!(matches!(
            PdMessage::from_hex(SopTarget::Sop, "8D0"),
            Err(MessageError::BadHex { .. })
        ));
        assert!(matches!(
            PdMessage::from_hex(SopTarget::Sop, "zz00"),
            Err(MessageError::BadHex { .. })
        ));
        // Header says 1 data object, none follows.
        assert!(matches!(
            PdMessage::from_hex(SopTarget::Sop, "8210"),
            Err(MessageError::BadHex { .. })
        ));
    }

    #[test]
    fn sop_names_for_the_cli() {
        assert_eq!(SopTarget::from_name("sop"), Some(SopTarget::Sop));
        assert_eq!(SopTarget::from_name("sop1"), Some(SopTarget::SopPrime));
        assert_eq!(
            SopTarget::from_name("sop2"),
            Some(SopTarget::SopDoublePrime)
        );
        assert_eq!(SopTarget::from_name("sop3"), None);
    }

    // ---- AC-DGSQ6T.2: the Discover Identity preset to the cable ----

    #[test]
    fn the_discover_identity_preset_goes_to_the_cable_as_a_structured_vdm_req() {
        assert_eq!(
            Preset::from_name("discover-identity-cable"),
            Some(Preset::DiscoverIdentityCable)
        );
        assert_eq!(
            Preset::DiscoverIdentityCable.name(),
            "discover-identity-cable"
        );
        let m = Preset::DiscoverIdentityCable.message();
        assert_eq!(m.sop, SopTarget::SopPrime);
        assert_eq!(m.msg_type, 15, "Vendor_Defined");
        assert_eq!(m.data_objects.len(), 1);
        let VdmHeader::Structured(h) = VdmHeader::decode(m.data_objects[0]) else {
            panic!("not structured: {:#X}", m.data_objects[0]);
        };
        assert_eq!(h.svid, 0xFF00);
        assert_eq!(h.command_type, CommandType::Req);
        assert_eq!(h.command, VdmCommand::DiscoverIdentity);
        let c = m.to_command().unwrap();
        let b = bytes(&hex_of(&c));
        assert_eq!(b[0], 1, "SOP'");
        assert_eq!(b.len(), 7);
    }

    #[test]
    fn any_structured_discover_identity_req_to_a_cable_is_recognised() {
        assert!(
            Preset::DiscoverIdentityCable
                .message()
                .is_cable_discover_identity()
        );
        // SOP'', VDM version 2.0 (bits 14..13 = 0b00), another SVID, an extra object.
        let other = PdMessage::data(SopTarget::SopDoublePrime, 15, vec![0xFF01_8001, 0]);
        assert!(other.is_cable_discover_identity());
        // Not on SOP, not an ACK, not another command, not unstructured, not data type 1.
        for m in [
            PdMessage::data(SopTarget::Sop, 15, vec![0xFF00_A801]),
            PdMessage::data(SopTarget::SopPrime, 15, vec![0xFF00_A841]),
            PdMessage::data(SopTarget::SopPrime, 15, vec![0xFF00_A802]),
            PdMessage::data(SopTarget::SopPrime, 15, vec![0xFF00_2801]),
            PdMessage::data(SopTarget::SopPrime, 1, vec![0xFF00_A801]),
            PdMessage::control(SopTarget::SopPrime, 13),
        ] {
            assert!(!m.is_cable_discover_identity(), "{m:?}");
        }
    }

    // ---- AC-DGSQ6T.3: GoodCRC is refused with a reason ----

    #[test]
    fn goodcrc_is_refused_on_every_sop() {
        for sop in [
            SopTarget::Sop,
            SopTarget::SopPrime,
            SopTarget::SopDoublePrime,
        ] {
            let err = PdMessage::control(sop, 1).to_command().unwrap_err();
            assert_eq!(err, MessageError::GoodCrc);
            assert!(err.to_string().contains("GoodCRC"), "{err}");
        }
        // Data type 1 is Source_Capabilities, not GoodCRC.
        assert!(
            PdMessage::data(SopTarget::Sop, 1, vec![0x0001_912C])
                .to_command()
                .is_ok()
        );
    }

    #[test]
    fn refusal_names_goodcrc_as_pd_cmd_and_as_pd_data() {
        let reason = refusal(&Command::PdCmd(1)).expect("pd cmd=1 refused");
        assert!(reason.contains("GoodCRC"), "{reason}");
        // A header-only GoodCRC to SOP' in pd data form.
        let reason = refusal(&Command::parse("pd data=014100")).expect("pd data GoodCRC refused");
        assert!(reason.contains("GoodCRC"), "{reason}");
        assert_eq!(refusal(&Command::PdCmd(13)), None);
        assert_eq!(refusal(&Command::parse("pd data=008D00")), None);
        assert_eq!(refusal(&Command::PdmParm), None);
    }

    #[test]
    fn goodcrc_in_any_raw_spelling_is_refused() {
        for line in [
            "pd cmd=01",
            "pd cmd=+1",
            "pd cmd=0x1",
            "pd cmd=0X01",
            "pd  cmd=1",
            "PD CMD=1",
            "pd cmd = 1",
            "pd data=014100",
            "PD DATA=014100",
            "pd  data=01 41 00",
            // The 5-bit Message Type field of a truncated value is GoodCRC.
            "pd cmd=33",
        ] {
            let command = Command::parse(line);
            let reason = refusal(&command).unwrap_or_else(|| panic!("{line:?} not refused"));
            assert!(reason.contains("GoodCRC"), "{line:?}: {reason}");
        }
    }

    #[test]
    fn a_pd_cmd_or_pd_data_line_that_cannot_be_read_is_refused() {
        for line in [
            "pd cmd=",
            "pd cmd=-1",
            "pd cmd=1a",
            "pd cmd=x",
            "pd cmdx=13",
            "pd data=zz",
            "pd data=014",
            "pd data=+14100",
            "pd data=0141",
            "pd data",
        ] {
            assert!(
                refusal(&Command::parse(line)).is_some(),
                "{line:?} not refused"
            );
        }
    }

    #[test]
    fn other_raw_pd_cmd_and_pd_data_lines_are_not_refused() {
        for line in [
            "pd cmd=013",
            "pd cmd=0xd",
            "pd  cmd=13",
            "pd data=008d00",
            "hello meter",
        ] {
            let command = Command::parse(line);
            assert!(matches!(command, Command::Raw(_)), "{line:?}");
            assert_eq!(refusal(&command), None, "{line:?}");
        }
    }

    // ---- hex parsing: digits only ----

    #[test]
    fn hex_parsing_rejects_anything_but_hex_digits() {
        assert_eq!(parse_hex_bytes("8d00"), Some(vec![0x8D, 0x00]));
        assert_eq!(parse_hex_bytes(""), Some(vec![]));
        assert_eq!(parse_hex_bytes("+d00"), None);
        assert_eq!(parse_hex_bytes("8d0"), None);
        assert_eq!(parse_hex_bytes("8d 0"), None);
        assert!(matches!(
            PdMessage::from_hex(SopTarget::Sop, "+D00"),
            Err(MessageError::BadHex { .. })
        ));
        // A received line with a sign is not a message.
        assert_eq!(received_message("pd data=+18D00"), None);
        assert_eq!(received_message("018D00"), Some((1, 0x008D, vec![])));
    }

    // ---- reserved control types ----

    #[test]
    fn reserved_control_types_are_refused_with_their_own_error() {
        for ty in [0, 0x19, 0x1C, 0x1F] {
            assert_eq!(
                PdMessage::control(SopTarget::Sop, ty).to_command(),
                Err(MessageError::ReservedControlType { ty }),
                "{ty}"
            );
        }
        let err = MessageError::ReservedControlType { ty: 0x19 }.to_string();
        assert!(err.contains("reserved"), "{err}");
    }

    // ---- AC-DGSQ6T.2: the cable's ACK decodes into e-marker details ----

    /// Session 2: the 240 W cable's Discover Identity ACK on SOP'. [device]
    const CABLE_ACK: [u32; 5] = [
        0xFF00_A841,
        0x1860_050D,
        0x0000_404D,
        0x8254_0000,
        0x000A_2640,
    ];

    fn le(objects: &[u32]) -> Vec<u8> {
        objects.iter().flat_map(|o| o.to_le_bytes()).collect()
    }

    #[test]
    fn a_passive_cable_ack_decodes_into_its_emarker_details() {
        let id = cable_identity_from_message(1, 0x518F, &le(&CABLE_ACK)).expect("cable ACK");
        assert_eq!(id.sop, SopTarget::SopPrime);
        assert_eq!(id.vid, 0x050D);
        assert_eq!(id.pid, Some(0x8254));
        assert!(!id.active);
        assert_eq!(id.max_voltage_v, Some(50));
        assert_eq!(id.max_current_ma, Some(5000));
        assert!(id.epr_capable);
        assert_eq!(id.identity.id_header.vid, 0x050D);
    }

    #[test]
    fn non_cable_or_non_ack_messages_are_not_a_cable_identity() {
        // The same ACK on SOP (a port partner) is not a cable.
        assert_eq!(
            cable_identity_from_message(0, 0x518F, &le(&CABLE_ACK)),
            None
        );
        // The REQ to the cable is not an answer.
        assert_eq!(
            cable_identity_from_message(1, 0x108F, &le(&[0xFF00_A801])),
            None
        );
        // A control message.
        assert_eq!(cable_identity_from_message(1, 0x008D, &[]), None);
    }
}
