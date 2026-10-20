//! Frame codec: main header (control and data views), extended section
//! header, request builders, reply classification and the PUT_DATA section
//! walker. Pure byte handling, no I/O. See `docs/protocol.md`.
//!
//! Every header on the wire is one little-endian 32-bit word split into
//! bit fields. The [`MainHeader`] starts each frame; PUT_DATA payloads are a
//! chain of [`SectionHeader`]-prefixed sections walked by [`parse_put_data`].

use std::ops::BitOr;

/// Message type codes (bits 0..6 of the main header).
pub(crate) mod msg {
    pub const CONNECT: u8 = 0x02;
    pub const DISCONNECT: u8 = 0x03;
    pub const ACCEPT: u8 = 0x05;
    pub const REJECT: u8 = 0x06;
    pub const FINISHED: u8 = 0x07;
    pub const GET_DATA: u8 = 0x0C;
    pub const START_QUEUE: u8 = 0x0E;
    pub const STOP_QUEUE: u8 = 0x0F;
    pub const PUT_DATA: u8 = 0x41;
    pub const MEM_READ_CONFIRM: u8 = 0x44;
    pub const AUTH_REPLY: u8 = 0x4C;
}

/// Size in bytes of every main and section header.
pub(crate) const HEADER_LEN: usize = 4;

/// Mask for the message type in byte 0 of a frame (bits 0..6; bit 7 is the
/// extend flag).
const TYPE_MASK: u8 = 0x7F;

/// Message type of `frame` from its first byte, extend bit masked off;
/// `None` for an empty frame. Needs only one byte, unlike [`MainHeader`].
pub(crate) fn frame_type(frame: &[u8]) -> Option<u8> {
    frame.first().map(|b| b & TYPE_MASK)
}

/// Whether `request` is a GET_DATA request (message type 0x0C, extend bit
/// ignored).
pub fn is_get_data(request: &[u8]) -> bool {
    frame_type(request) == Some(msg::GET_DATA)
}

/// Attribute mask of a GET_DATA request (`mask << 1` in bytes 2..3, LE);
/// `None` when `request` is not a GET_DATA. Missing mask bytes read as 0.
pub fn get_data_mask(request: &[u8]) -> Option<Attribute> {
    if !is_get_data(request) {
        return None;
    }
    let byte = |i: usize| request.get(i).copied().unwrap_or(0);
    Some(Attribute(u16::from_le_bytes([byte(2), byte(3)]) >> 1))
}

/// Read `width` bits of `word` starting at bit `shift`.
const fn get_bits(word: u32, shift: u32, width: u32) -> u32 {
    (word >> shift) & ((1 << width) - 1)
}

/// Read the single bit `bit` of `word`.
const fn get_flag(word: u32, bit: u32) -> bool {
    word & (1 << bit) != 0
}

/// Place the low `width` bits of `value` at bit `shift` (excess bits dropped).
const fn put_bits(value: u32, shift: u32, width: u32) -> u32 {
    (value & ((1 << width) - 1)) << shift
}

/// Read the little-endian header word at `offset`.
fn read_word(bytes: &[u8], offset: usize) -> Result<u32, FrameError> {
    bytes
        .get(offset..offset + HEADER_LEN)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(FrameError::TruncatedHeader { offset })
}

/// Errors from decoding frames.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// Fewer than 4 bytes were left where a main or section header must start.
    #[error("truncated header at offset {offset}")]
    TruncatedHeader {
        /// Byte offset where the header should have started.
        offset: usize,
    },
    /// A section declares more payload bytes than the buffer holds.
    #[error("section att {att:#x} needs {needed} bytes at offset {offset}, {available} available")]
    SectionOverrun {
        /// Raw attribute code of the offending section.
        att: u16,
        /// Byte offset of the section header.
        offset: usize,
        /// Payload bytes the section header declares.
        needed: usize,
        /// Payload bytes actually left after the section header.
        available: usize,
    },
    /// A section has `next = 1` but the frame ends right after it.
    #[error("section chain promises another section at offset {offset}, frame ends")]
    MissingNextSection {
        /// Byte offset where the promised section should start (frame end).
        offset: usize,
    },
}

/// GET_DATA attribute bit mask (bitflags-style newtype over u16).
///
/// Also used as the attribute code of a PUT_DATA [`Section`]. Combine masks
/// with `|`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Attribute(pub u16);

impl Attribute {
    /// Single ADC sample (`ATT_ADC`).
    pub const ADC: Attribute = Attribute(0x001);
    /// Buffered ADC sample queue (`ATT_ADC_QUEUE`).
    pub const ADC_QUEUE: Attribute = Attribute(0x002);
    /// Device settings block.
    pub const SETTINGS: Attribute = Attribute(0x008);
    /// PD status / PD packet stream.
    pub const PD_STATUS: Attribute = Attribute(0x020);

    /// The raw mask bits.
    pub fn bits(self) -> u16 {
        self.0
    }
}

impl BitOr for Attribute {
    type Output = Attribute;
    fn bitor(self, rhs: Attribute) -> Attribute {
        Attribute(self.0 | rhs.0)
    }
}

/// The 4-byte little-endian main header, kept as its raw 32 bits.
///
/// Read it through the [`control`](MainHeader::control) or
/// [`data`](MainHeader::data) view depending on the message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MainHeader(pub u32);

/// Control view of a main header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlHeader {
    /// Message type, bits 0..6.
    pub ty: u8,
    /// Extended-header flag, bit 7.
    pub extend: bool,
    /// Transaction id, bits 8..15.
    pub id: u8,
    /// Encrypted flag, bit 16.
    pub enc: bool,
    /// Attribute, bits 17..31 (15 bits).
    pub att: u16,
}

/// Data view of a main header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataHeader {
    /// Message type, bits 0..6.
    pub ty: u8,
    /// Extended-header flag, bit 7.
    pub extend: bool,
    /// Transaction id, bits 8..15.
    pub id: u8,
    /// Encoded flag, bit 16.
    pub encode: bool,
    /// Permission, bits 17..18.
    pub permi: u8,
    /// Object count / size in 4-byte words, bits 22..31 (10 bits).
    /// Bits 19..21 are reserved.
    pub obj: u16,
}

impl MainHeader {
    /// Decode the first 4 bytes of `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<MainHeader, FrameError> {
        read_word(bytes, 0).map(MainHeader)
    }

    /// Encode as 4 little-endian bytes.
    pub fn encode(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    /// Message type (bits 0..6), shared by both views.
    fn ty(self) -> u8 {
        get_bits(self.0, 0, 7) as u8
    }

    /// Control view.
    pub fn control(self) -> ControlHeader {
        let v = self.0;
        ControlHeader {
            ty: self.ty(),
            extend: get_flag(v, 7),
            id: get_bits(v, 8, 8) as u8,
            enc: get_flag(v, 16),
            att: get_bits(v, 17, 15) as u16,
        }
    }

    /// Data view.
    pub fn data(self) -> DataHeader {
        let v = self.0;
        DataHeader {
            ty: self.ty(),
            extend: get_flag(v, 7),
            id: get_bits(v, 8, 8) as u8,
            encode: get_flag(v, 16),
            permi: get_bits(v, 17, 2) as u8,
            obj: get_bits(v, 22, 10) as u16,
        }
    }
}

/// Pack the low 17 bits shared by both main-header views.
fn pack_common(ty: u8, extend: bool, id: u8, flag16: bool) -> u32 {
    put_bits(ty.into(), 0, 7)
        | put_bits(extend.into(), 7, 1)
        | put_bits(id.into(), 8, 8)
        | put_bits(flag16.into(), 16, 1)
}

impl ControlHeader {
    /// Pack this view into a main header.
    pub fn to_header(self) -> MainHeader {
        MainHeader(
            pack_common(self.ty, self.extend, self.id, self.enc)
                | put_bits(self.att.into(), 17, 15),
        )
    }
}

impl DataHeader {
    /// Pack this view into a main header (reserved bits 19..21 are zero).
    pub fn to_header(self) -> MainHeader {
        MainHeader(
            pack_common(self.ty, self.extend, self.id, self.encode)
                | put_bits(self.permi.into(), 17, 2)
                | put_bits(self.obj.into(), 22, 10),
        )
    }
}

/// Extended section header inside a data payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionHeader {
    /// Attribute code, bits 0..14.
    pub att: u16,
    /// Another section follows this one, bit 15.
    pub next: bool,
    /// Chunk count (ADC queue sample count), bits 16..21.
    pub chunk: u8,
    /// Payload size in bytes (per chunk for the ADC queue), bits 22..31.
    pub size: u16,
}

impl SectionHeader {
    /// Decode the first 4 bytes of `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<SectionHeader, FrameError> {
        read_word(bytes, 0).map(SectionHeader::from_word)
    }

    fn from_word(v: u32) -> SectionHeader {
        SectionHeader {
            att: get_bits(v, 0, 15) as u16,
            next: get_flag(v, 15),
            chunk: get_bits(v, 16, 6) as u8,
            size: get_bits(v, 22, 10) as u16,
        }
    }

    /// Encode as 4 little-endian bytes.
    pub fn encode(self) -> [u8; 4] {
        (put_bits(self.att.into(), 0, 15)
            | put_bits(self.next.into(), 15, 1)
            | put_bits(self.chunk.into(), 16, 6)
            | put_bits(self.size.into(), 22, 10))
        .to_le_bytes()
    }

    /// Payload bytes this section declares: `chunk * size` for the ADC queue,
    /// `size` for every other attribute.
    fn payload_len(self) -> usize {
        if Attribute(self.att) == Attribute::ADC_QUEUE {
            usize::from(self.chunk) * usize::from(self.size)
        } else {
            usize::from(self.size)
        }
    }
}

/// One section of a PUT_DATA reply, borrowed from the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section<'a> {
    /// Attribute code from the section header.
    pub att: Attribute,
    /// Section payload, header excluded.
    pub payload: &'a [u8],
}

/// A reply classified by its main-header type.
///
/// Known variants carry the decoded header for further inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// ACCEPT (`0x05`): the request was accepted.
    Accept(MainHeader),
    /// REJECT (`0x06`): the request was refused.
    Reject(MainHeader),
    /// FINISHED (`0x07`): a multi-part operation completed.
    Finished(MainHeader),
    /// DISCONNECT (`0x03`): the device ends the session.
    Disconnect(MainHeader),
    /// PUT_DATA (`0x41`): data sections follow the header.
    PutData(MainHeader),
    /// Memory-read confirmation (`0x44` with the extend bit set).
    MemReadConfirm(MainHeader),
    /// Authentication reply (`0x4C`).
    AuthReply(MainHeader),
    /// Any other message type, by its type code.
    Unknown(u8),
}

/// Classify a reply frame by its main header.
pub fn classify_reply(frame: &[u8]) -> Result<Reply, FrameError> {
    let h = MainHeader::decode(frame)?;
    let c = h.control();
    Ok(match (c.ty, c.extend) {
        (msg::ACCEPT, _) => Reply::Accept(h),
        (msg::REJECT, _) => Reply::Reject(h),
        (msg::FINISHED, _) => Reply::Finished(h),
        (msg::DISCONNECT, _) => Reply::Disconnect(h),
        (msg::PUT_DATA, _) => Reply::PutData(h),
        (msg::MEM_READ_CONFIRM, true) => Reply::MemReadConfirm(h),
        (msg::AUTH_REPLY, _) => Reply::AuthReply(h),
        (ty, _) => Reply::Unknown(ty),
    })
}

/// Walk the section chain of a whole PUT_DATA frame (main header included).
///
/// A `PD_STATUS` section has no usable size and takes the rest of the frame.
/// ADC queue sections hold `chunk * size` bytes; every other section `size`.
pub fn parse_put_data(frame: &[u8]) -> Result<Vec<Section<'_>>, FrameError> {
    MainHeader::decode(frame)?;
    let mut sections = Vec::new();
    let mut offset = HEADER_LEN;
    loop {
        let header = SectionHeader::from_word(read_word(frame, offset)?);
        let start = offset + HEADER_LEN;
        let att = Attribute(header.att);
        if att == Attribute::PD_STATUS {
            sections.push(Section {
                att,
                payload: &frame[start..],
            });
            return Ok(sections);
        }
        let needed = header.payload_len();
        let available = frame.len() - start;
        if needed > available {
            return Err(FrameError::SectionOverrun {
                att: header.att,
                offset,
                needed,
                available,
            });
        }
        let end = start + needed;
        sections.push(Section {
            att,
            payload: &frame[start..end],
        });
        if !header.next {
            return Ok(sections);
        }
        if end == frame.len() {
            return Err(FrameError::MissingNextSection { offset: end });
        }
        offset = end;
    }
}

/// Where the section chain of a PUT_DATA frame ends according to its
/// headers alone, without checking that the payloads have arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclaredEnd {
    /// The last section (`next = 0`) ends at this offset.
    Complete(usize),
    /// A section header should start at this offset, but the buffer ends
    /// before its 4 bytes.
    MissingHeader(usize),
    /// A `PD_STATUS` section, whose size field is unusable, ends the chain;
    /// `size_end` is where its size field would put the end.
    PdStatus {
        /// End offset implied by the PD_STATUS section's size field.
        size_end: usize,
    },
}

/// Follow the section headers of the PUT_DATA frame in `buf` (main header
/// included) by their declared payload lengths. Payload bytes are never
/// read, so `buf` may hold only part of the frame.
pub(crate) fn declared_end(buf: &[u8]) -> DeclaredEnd {
    let mut offset = HEADER_LEN;
    loop {
        let Ok(word) = read_word(buf, offset) else {
            return DeclaredEnd::MissingHeader(offset);
        };
        let header = SectionHeader::from_word(word);
        let end = offset + HEADER_LEN + header.payload_len();
        if Attribute(header.att) == Attribute::PD_STATUS {
            return DeclaredEnd::PdStatus { size_end: end };
        }
        if !header.next {
            return DeclaredEnd::Complete(end);
        }
        offset = end;
    }
}

/// A 4-byte control request: `ty id lo hi`.
fn request(ty: u8, id: u8, lo: u8, hi: u8) -> [u8; 4] {
    [ty, id, lo, hi]
}

/// GET_DATA request: `0C id` + `mask << 1` as u16 LE.
pub fn get_data(id: u8, mask: Attribute) -> [u8; 4] {
    let [lo, hi] = (mask.bits() << 1).to_le_bytes();
    request(msg::GET_DATA, id, lo, hi)
}

/// CONNECT request: `02 id 00 00`.
pub fn connect(id: u8) -> [u8; 4] {
    request(msg::CONNECT, id, 0, 0)
}

/// Start ADC queue: `0E id (code << 1) 00`.
pub fn start_queue(id: u8, code: u8) -> [u8; 4] {
    request(msg::START_QUEUE, id, code << 1, 0)
}

/// Stop ADC queue: `0F id 00 00`.
pub fn stop_queue(id: u8) -> [u8; 4] {
    request(msg::STOP_QUEUE, id, 0, 0)
}

/// Transaction id source: pre-incremented u8 that wraps at 256.
#[derive(Debug, Clone, Default)]
pub struct SequenceCounter {
    current: u8,
}

impl SequenceCounter {
    /// A counter whose first `next()` is 1.
    pub fn new() -> SequenceCounter {
        SequenceCounter::default()
    }

    /// Pre-increment and return the new id.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u8 {
        self.current = self.current.wrapping_add(1);
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ---- AC-BXYTJX.1: MainHeader control and data views ----

    #[test]
    fn main_header_decodes_get_data_adc_control_view() {
        let h = MainHeader::decode(&[0x0C, 0x00, 0x02, 0x00]).unwrap();
        assert_eq!(h, MainHeader(0x0002_000C));
        assert_eq!(
            h.control(),
            ControlHeader {
                ty: 0x0C,
                extend: false,
                id: 0,
                enc: false,
                att: 1
            }
        );
    }

    #[test]
    fn main_header_decodes_put_data_data_view() {
        let h = MainHeader::decode(&[0x41, 0x01, 0x80, 0x02]).unwrap();
        let d = h.data();
        assert_eq!(d.ty, 0x41);
        assert!(!d.extend);
        assert_eq!(d.id, 1);
        assert!(!d.encode);
        assert_eq!(d.permi, 0);
        assert_eq!(d.obj, 10);
    }

    #[test]
    fn main_header_data_view_reads_permi_one() {
        let d = MainHeader::decode(&[0x41, 0x61, 0x82, 0x02])
            .unwrap()
            .data();
        assert_eq!(d.ty, 0x41);
        assert_eq!(d.id, 0x61);
        assert_eq!(d.permi, 1);
        assert_eq!(d.obj, 10);
    }

    #[test]
    fn main_header_control_view_field_extremes() {
        let c = MainHeader(0xFFFF_FFFF).control();
        assert_eq!(
            c,
            ControlHeader {
                ty: 0x7F,
                extend: true,
                id: 0xFF,
                enc: true,
                att: 0x7FFF
            }
        );
        let d = MainHeader(0xFFFF_FFFF).data();
        assert_eq!(
            d,
            DataHeader {
                ty: 0x7F,
                extend: true,
                id: 0xFF,
                encode: true,
                permi: 3,
                obj: 0x3FF
            }
        );
        // Single bits land in the right fields.
        assert!(MainHeader(1 << 7).control().extend);
        assert!(MainHeader(1 << 16).control().enc);
        assert!(MainHeader(1 << 16).data().encode);
        assert_eq!(MainHeader(1 << 17).control().att, 1);
        assert_eq!(MainHeader(1 << 18).data().permi, 2);
        assert_eq!(MainHeader(1 << 22).data().obj, 1);
        // Reserved bits 19..21 do not leak into permi or obj.
        let r = MainHeader(0b111 << 19).data();
        assert_eq!((r.permi, r.obj), (0, 0));
    }

    #[test]
    fn main_header_encodes_views_to_exact_bytes() {
        let c = ControlHeader {
            ty: 0x0C,
            extend: false,
            id: 0,
            enc: false,
            att: 1,
        };
        assert_eq!(c.to_header().encode(), [0x0C, 0x00, 0x02, 0x00]);
        let d = DataHeader {
            ty: 0x41,
            extend: false,
            id: 0x61,
            encode: false,
            permi: 1,
            obj: 10,
        };
        assert_eq!(d.to_header().encode(), [0x41, 0x61, 0x82, 0x02]);
        let mem = DataHeader {
            ty: 0x44,
            extend: true,
            id: 7,
            encode: true,
            permi: 0,
            obj: 4,
        };
        assert_eq!(mem.to_header().encode(), [0xC4, 0x07, 0x01, 0x01]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20_000))]

        #[test]
        fn main_header_round_trips_every_u32(raw in any::<u32>()) {
            let bytes = raw.to_le_bytes();
            let h = MainHeader::decode(&bytes).unwrap();
            prop_assert_eq!(h, MainHeader(raw));
            prop_assert_eq!(h.encode(), bytes);
            // The control view covers all 32 bits.
            prop_assert_eq!(h.control().to_header(), h);
            // The data view covers everything except reserved bits 19..21.
            prop_assert_eq!(h.data().to_header(), MainHeader(raw & !(0b111 << 19)));
        }
    }

    // ---- AC-BXYTJX.2: SectionHeader ----

    #[test]
    fn section_header_decodes_adc_section() {
        let s = SectionHeader::decode(&[0x01, 0x00, 0x00, 0x0b]).unwrap();
        assert_eq!(
            s,
            SectionHeader {
                att: 1,
                next: false,
                chunk: 0,
                size: 44
            }
        );
    }

    #[test]
    fn section_header_decodes_next_bit() {
        let s = SectionHeader::decode(&[0x01, 0x80, 0x00, 0x0b]).unwrap();
        assert_eq!(
            s,
            SectionHeader {
                att: 1,
                next: true,
                chunk: 0,
                size: 44
            }
        );
    }

    #[test]
    fn section_header_decodes_queue_chunk_and_extremes() {
        // Captured queue section header: att 2, chunk 63, size 20.
        let q = SectionHeader::decode(&[0x02, 0x00, 0x3f, 0x05]).unwrap();
        assert_eq!(
            q,
            SectionHeader {
                att: 2,
                next: false,
                chunk: 63,
                size: 20
            }
        );
        let all = SectionHeader::decode(&[0xFF; 4]).unwrap();
        assert_eq!(
            all,
            SectionHeader {
                att: 0x7FFF,
                next: true,
                chunk: 0x3F,
                size: 0x3FF
            }
        );
    }

    #[test]
    fn section_header_encodes_exact_bytes() {
        let s = SectionHeader {
            att: 1,
            next: true,
            chunk: 0,
            size: 44,
        };
        assert_eq!(s.encode(), [0x01, 0x80, 0x00, 0x0b]);
    }

    proptest! {
        #[test]
        fn section_header_round_trips_every_u32(raw in any::<u32>()) {
            let bytes = raw.to_le_bytes();
            prop_assert_eq!(SectionHeader::decode(&bytes).unwrap().encode(), bytes);
        }
    }

    // ---- AC-BXYTJX.3: request builders ----

    #[test]
    fn get_data_builds_mask_shifted_left_one() {
        assert_eq!(get_data(0x00, Attribute(0x001)), [0x0C, 0x00, 0x02, 0x00]);
        assert_eq!(get_data(0x05, Attribute(0x001)), [0x0C, 0x05, 0x02, 0x00]);
        assert_eq!(get_data(0x02, Attribute(0x009)), [0x0C, 0x02, 0x12, 0x00]);
        assert_eq!(
            get_data(0x02, Attribute::ADC | Attribute::SETTINGS),
            [0x0C, 0x02, 0x12, 0x00]
        );
        // Captured requests: ADC_QUEUE and PD_STATUS, mask 0x029.
        assert_eq!(
            get_data(0x0b, Attribute::ADC_QUEUE),
            [0x0C, 0x0b, 0x04, 0x00]
        );
        assert_eq!(get_data(0x10, Attribute(0x029)), [0x0C, 0x10, 0x52, 0x00]);
        // A high mask bit carries into byte 3.
        assert_eq!(get_data(0x04, Attribute(0x080)), [0x0C, 0x04, 0x00, 0x01]);
    }

    #[test]
    fn attribute_bitor_combines_masks() {
        assert_eq!((Attribute::ADC | Attribute::PD_STATUS).bits(), 0x021);
    }

    #[test]
    fn connect_builds_exact_bytes() {
        assert_eq!(connect(0x11), [0x02, 0x11, 0x00, 0x00]);
    }

    #[test]
    fn start_queue_builds_code_shifted_left_one() {
        assert_eq!(start_queue(0x01, 1), [0x0E, 0x01, 0x02, 0x00]);
        assert_eq!(start_queue(0x2a, 3), [0x0E, 0x2a, 0x06, 0x00]);
        assert_eq!(start_queue(0x00, 0), [0x0E, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn stop_queue_builds_exact_bytes() {
        assert_eq!(stop_queue(0x07), [0x0F, 0x07, 0x00, 0x00]);
    }

    // ---- AC-BXYTJX.4: classify_reply ----

    fn hdr(bytes: [u8; 4]) -> MainHeader {
        MainHeader(u32::from_le_bytes(bytes))
    }

    #[test]
    fn classify_reply_maps_control_types() {
        let b = [0x05, 0x11, 0x00, 0x00];
        assert_eq!(classify_reply(&b).unwrap(), Reply::Accept(hdr(b)));
        let b = [0x06, 0x01, 0x00, 0x00];
        assert_eq!(classify_reply(&b).unwrap(), Reply::Reject(hdr(b)));
        let b = [0x07, 0x09, 0x00, 0x00];
        assert_eq!(classify_reply(&b).unwrap(), Reply::Finished(hdr(b)));
        let b = [0x03, 0x01, 0x00, 0x00];
        assert_eq!(classify_reply(&b).unwrap(), Reply::Disconnect(hdr(b)));
    }

    #[test]
    fn classify_reply_maps_data_types() {
        // PUT_DATA with payload after the header: classified by header alone.
        let frame = [0x41, 0x01, 0x80, 0x02, 0x01, 0x00, 0x00, 0x0b];
        assert_eq!(
            classify_reply(&frame).unwrap(),
            Reply::PutData(hdr([0x41, 0x01, 0x80, 0x02]))
        );
        let b = [0xC4, 0x07, 0x01, 0x01];
        assert_eq!(classify_reply(&b).unwrap(), Reply::MemReadConfirm(hdr(b)));
        let b = [0x4C, 0x00, 0x03, 0x02];
        assert_eq!(classify_reply(&b).unwrap(), Reply::AuthReply(hdr(b)));
    }

    #[test]
    fn classify_reply_unknown_type() {
        assert_eq!(
            classify_reply(&[0x0C, 0x00, 0x02, 0x00]).unwrap(),
            Reply::Unknown(0x0C)
        );
        assert_eq!(
            classify_reply(&[0x48, 0x00, 0x00, 0x00]).unwrap(),
            Reply::Unknown(0x48)
        );
        assert_eq!(
            classify_reply(&[0x7F, 0x00, 0x00, 0x00]).unwrap(),
            Reply::Unknown(0x7F)
        );
    }

    #[test]
    fn classify_reply_truncated_header_is_error() {
        assert!(matches!(
            classify_reply(&[0x05, 0x01]),
            Err(FrameError::TruncatedHeader { .. })
        ));
    }

    // ---- AC-BXYTJX.8: SequenceCounter ----

    #[test]
    fn sequence_counter_pre_increments_and_wraps() {
        let mut c = SequenceCounter::new();
        assert_eq!(c.next(), 1);
        assert_eq!(c.next(), 2);
        for expected in 3..=255u8 {
            assert_eq!(c.next(), expected);
        }
        assert_eq!(c.next(), 0);
        assert_eq!(c.next(), 1);
        assert_eq!(c.next(), 2);
    }

    #[test]
    fn sequence_counter_default_matches_new() {
        let mut c = SequenceCounter::default();
        assert_eq!(c.next(), 1);
    }
}
