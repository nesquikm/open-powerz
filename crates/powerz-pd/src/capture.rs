//! The capture context: feed it records in order, get decoded messages.

use crate::blocks::{self, EPR_MODE, EXTENDED_CONTROL};
use crate::extended::{EprMode, ExtendedControl, ExtendedHeader};
use crate::field::Field;
use crate::header::{Header, MessageKind, Sop};
use crate::pdo::{SinkPdo, SourcePdo};
use crate::rdo::Request;
use crate::vdm::Vdm;

/// One captured record, as the decoder sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input<'a> {
    /// A PD message.
    Message {
        /// The meter's SOP code (0 SOP, 1 SOP', 2 SOP'', ...).
        sop: u8,
        /// The 16-bit message header.
        header: u16,
        /// The bytes after the header.
        body: &'a [u8],
    },
    /// Hard Reset signalling.
    HardReset,
    /// Cable Reset signalling.
    CableReset,
    /// A sink attached.
    Attach,
    /// The sink detached.
    Detach,
}

/// A decoded message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// A control message (no body).
    Control,
    /// Source_Capabilities.
    SourceCapabilities(Vec<SourcePdo>),
    /// Sink_Capabilities.
    SinkCapabilities(Vec<SinkPdo>),
    /// Request.
    Request(Request),
    /// EPR_Request.
    EprRequest(Request),
    /// Vendor_Defined.
    Vdm(Vdm),
    /// EPR_Mode.
    EprMode(EprMode),
    /// Extended_Control.
    ExtendedControl(ExtendedControl),
    /// EPR_Source_Capabilities (reassembled when chunked).
    EprSourceCapabilities(Vec<SourcePdo>),
    /// EPR_Sink_Capabilities (reassembled when chunked).
    EprSinkCapabilities(Vec<SinkPdo>),
    /// One chunk of a longer extended message, waiting for the rest.
    Chunk {
        /// Chunk Number.
        chunk_number: u8,
        /// Data Size of the whole message, bytes.
        data_size: u16,
    },
    /// A request for chunk `chunk_number`.
    ChunkRequest {
        /// Chunk Number asked for.
        chunk_number: u8,
    },
    /// A data message without a typed body, its data objects raw.
    Data(Vec<u32>),
    /// An extended message without a typed body, its data block raw.
    Extended(Vec<u8>),
}

/// Something wrong with a message or its context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// A Request with no Source_Capabilities remembered on SOP.
    NoSourceCapabilities,
    /// A Request pointing past the remembered PDOs (or at 0).
    PositionOutOfRange {
        /// Object Position of the RDO.
        position: u8,
        /// Number of PDOs remembered.
        available: usize,
    },
    /// A chunk arrived out of sequence.
    MissingChunk {
        /// Chunk Number that was due.
        expected: u8,
        /// Chunk Number that arrived.
        got: u8,
    },
    /// The body is shorter than the header's Number of Data Objects says;
    /// the incomplete trailing object is not decoded.
    ShortBody {
        /// Bytes the header announces (Number of Data Objects × 4).
        expected: usize,
        /// Bytes that arrived.
        got: usize,
    },
}

/// One decoded PD message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The message header.
    pub header: Header,
    /// The extended header, for extended messages.
    pub extended_header: Option<ExtendedHeader>,
    /// The typed body.
    pub message: Message,
    /// Problems found while decoding.
    pub issues: Vec<Issue>,
}

impl Decoded {
    /// One-line summary for the packet table, starting with the type name.
    pub fn summary(&self) -> String {
        let name = self.header.message_type.name();
        match &self.message {
            Message::Request(r) | Message::EprRequest(r) => format!("{name}: {}", r.summary()),
            Message::Vdm(v) => format!("{name}: {}", v.summary()),
            Message::EprMode(m) => format!("{name}: {}", m.action.name()),
            Message::ExtendedControl(c) => format!("{name}: {}", c.kind.name()),
            Message::SourceCapabilities(pdos) | Message::EprSourceCapabilities(pdos) => {
                let list: Vec<String> = pdos
                    .iter()
                    .filter(|p| **p != SourcePdo::decode(0))
                    .map(SourcePdo::summary)
                    .collect();
                if list.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}: {}", list.join(", "))
                }
            }
            Message::Data(objects) => match objects.first() {
                Some(&dobj) => blocks::data_summary(self.header.message_type.code(), dobj)
                    .map_or_else(|| name.to_string(), |tail| format!("{name}: {tail}")),
                None => name.to_string(),
            },
            Message::Extended(_) => {
                blocks::block_summary(self.header.message_type.code(), &self.block())
                    .map_or_else(|| name.to_string(), |tail| format!("{name}: {tail}"))
            }
            _ => name.to_string(),
        }
    }

    /// The field tree: the header first, then the extended header and body.
    pub fn fields(&self) -> Vec<Field> {
        let mut out = vec![self.header.field()];
        if let Some(ext) = &self.extended_header {
            out.push(ext.field());
        }
        match &self.message {
            Message::SourceCapabilities(pdos) | Message::EprSourceCapabilities(pdos) => {
                out.extend(pdos.iter().enumerate().map(|(i, p)| p.field(i + 1)));
            }
            Message::SinkCapabilities(pdos) | Message::EprSinkCapabilities(pdos) => {
                out.extend(pdos.iter().enumerate().map(|(i, p)| p.field(i + 1)));
            }
            Message::Request(r) => out.push(r.rdo.field()),
            Message::EprRequest(r) => {
                out.push(r.rdo.field());
                out.extend(r.copy_field());
            }
            Message::Vdm(v) => out.extend(v.fields(self.header.sop)),
            Message::Data(objects) => out.extend(
                objects
                    .first()
                    .and_then(|&d| blocks::data_object_field(self.header.message_type.code(), d)),
            ),
            Message::EprMode(m) => out.extend(blocks::data_object_field(
                self.header.message_type.code(),
                m.object(),
            )),
            Message::ExtendedControl(_) | Message::Extended(_) => out.extend(
                blocks::data_block_field(self.header.message_type.code(), &self.block()),
            ),
            _ => {}
        }
        out
    }
}

impl Decoded {
    /// The extended data block: the body after the extended header, cut to
    /// Data Size (dropping the wire padding).
    fn block(&self) -> Vec<u8> {
        match &self.message {
            Message::Extended(data) => {
                let n = self
                    .extended_header
                    .map_or(data.len(), |e| usize::from(e.data_size).min(data.len()));
                data[..n].to_vec()
            }
            Message::ExtendedControl(c) => c.bytes().to_vec(),
            _ => Vec::new(),
        }
    }
}

/// Data message code of Source_Capabilities.
const SOURCE_CAPABILITIES: u8 = 0x01;
/// Data message code of Sink_Capabilities.
const SINK_CAPABILITIES: u8 = 0x04;
/// Data message code of Request.
const REQUEST: u8 = 0x02;
/// Data message code of EPR_Request.
const EPR_REQUEST: u8 = 0x09;
/// Data message code of Vendor_Defined.
const VENDOR_DEFINED: u8 = 0x0F;
/// Control message code of Soft_Reset.
const SOFT_RESET: u8 = 0x0D;
/// Extended message code of EPR_Source_Capabilities.
const EPR_SOURCE_CAPABILITIES: u8 = 0x11;
/// Extended message code of EPR_Sink_Capabilities.
const EPR_SINK_CAPABILITIES: u8 = 0x12;
/// MaxExtendedMsgChunkLen: data bytes in one chunk (USB PD r3.2).
const MAX_CHUNK_LEN: usize = 26;

/// A chunked extended message being reassembled.
#[derive(Debug, Clone)]
struct Pending {
    /// Meter SOP code.
    sop: u8,
    /// Extended message type code.
    code: u8,
    /// Data Size of the whole message, bytes.
    data_size: usize,
    /// Chunk Number due next.
    next: u8,
    /// Data collected so far.
    data: Vec<u8>,
}

/// Decoder state across the records of one capture.
#[derive(Debug, Clone, Default)]
pub struct Capture {
    /// The last Source_Capabilities seen on SOP, until a reset or detach.
    source_caps: Option<Vec<SourcePdo>>,
    /// Chunked extended messages in flight, one per (SOP, type).
    pending: Vec<Pending>,
}

/// The whole little-endian 32-bit objects of `data` (a trailing partial
/// object is dropped).
fn objects(data: &[u8]) -> impl Iterator<Item = u32> + '_ {
    data.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
}

/// Decode a complete extended data block (padding already cut for the
/// typed bodies).
fn decode_extended(code: u8, data: &[u8]) -> Message {
    match code {
        EXTENDED_CONTROL => ExtendedControl::decode(data).map_or_else(
            || Message::Extended(data.to_vec()),
            Message::ExtendedControl,
        ),
        EPR_SOURCE_CAPABILITIES => {
            Message::EprSourceCapabilities(objects(data).map(SourcePdo::decode).collect())
        }
        EPR_SINK_CAPABILITIES => {
            Message::EprSinkCapabilities(objects(data).map(SinkPdo::decode).collect())
        }
        _ => Message::Extended(data.to_vec()),
    }
}

impl Capture {
    /// A fresh capture with nothing remembered.
    pub fn new() -> Capture {
        Capture::default()
    }

    /// Feed the next record. Messages decode to `Some`; events update the
    /// context and decode to `None`.
    pub fn feed(&mut self, input: Input<'_>) -> Option<Decoded> {
        let (sop, header, body) = match input {
            Input::Message { sop, header, body } => (sop, header, body),
            Input::HardReset | Input::Detach => {
                self.source_caps = None;
                self.pending.clear();
                return None;
            }
            Input::CableReset | Input::Attach => return None,
        };
        let header = Header::decode(header, Sop::from_raw(sop));
        let on_sop = header.sop == Sop::Sop;
        let mut issues = Vec::new();
        let extended_header = match (header.extended, body) {
            (true, [lo, hi, ..]) => Some(ExtendedHeader::decode(u16::from_le_bytes([*lo, *hi]))),
            _ => None,
        };
        let message = match header.message_type.kind() {
            MessageKind::Control => {
                if on_sop && header.message_type.code() == SOFT_RESET {
                    self.source_caps = None;
                }
                Message::Control
            }
            MessageKind::Data => {
                let expected = usize::from(header.num_data_objects) * 4;
                if body.len() < expected {
                    issues.push(Issue::ShortBody {
                        expected,
                        got: body.len(),
                    });
                }
                // Words past the announced count are not data objects.
                let objects: Vec<u32> = objects(body)
                    .take(usize::from(header.num_data_objects))
                    .collect();
                match (header.message_type.code(), objects.as_slice()) {
                    (SOURCE_CAPABILITIES, _) => {
                        let pdos: Vec<SourcePdo> =
                            objects.into_iter().map(SourcePdo::decode).collect();
                        if on_sop {
                            self.source_caps = Some(pdos.clone());
                        }
                        Message::SourceCapabilities(pdos)
                    }
                    (REQUEST, [rdo, ..]) => {
                        let (req, found) = Request::against_caps(*rdo, self.source_caps.as_deref());
                        issues.extend(found);
                        Message::Request(req)
                    }
                    (EPR_REQUEST, [rdo, copy, ..]) => {
                        Message::EprRequest(Request::with_copy(*rdo, *copy))
                    }
                    (SINK_CAPABILITIES, _) => Message::SinkCapabilities(
                        objects.into_iter().map(SinkPdo::decode).collect(),
                    ),
                    // Set reserved bits stay visible: such an object keeps the
                    // raw path, whose field tree flags them.
                    (EPR_MODE, [dobj, ..]) if dobj & 0xFFFF == 0 => {
                        Message::EprMode(EprMode::decode(*dobj))
                    }
                    (VENDOR_DEFINED, [_, ..]) => Vdm::decode(&objects, header.sop)
                        .map_or_else(|| Message::Data(objects.clone()), Message::Vdm),
                    _ => Message::Data(objects),
                }
            }
            MessageKind::Extended => {
                let data = body.get(2..).unwrap_or_default();
                let code = header.message_type.code();
                match extended_header {
                    Some(ext) if ext.chunked && ext.request_chunk => Message::ChunkRequest {
                        chunk_number: ext.chunk_number,
                    },
                    Some(ext)
                        if ext.chunked
                            && (ext.chunk_number > 0
                                || usize::from(ext.data_size) > MAX_CHUNK_LEN) =>
                    {
                        match self.reassemble(sop, code, ext, data, &mut issues) {
                            Some(full) => decode_extended(code, &full),
                            None => Message::Chunk {
                                chunk_number: ext.chunk_number,
                                data_size: ext.data_size,
                            },
                        }
                    }
                    Some(ext)
                        if matches!(code, EPR_SOURCE_CAPABILITIES | EPR_SINK_CAPABILITIES) =>
                    {
                        let n = usize::from(ext.data_size).min(data.len());
                        decode_extended(code, &data[..n])
                    }
                    _ => decode_extended(code, data),
                }
            }
        };
        Some(Decoded {
            header,
            extended_header,
            message,
            issues,
        })
    }

    /// Add one chunk to its message in flight; the whole data block once the
    /// last chunk is in. Out-of-sequence chunks are reported and dropped.
    fn reassemble(
        &mut self,
        sop: u8,
        code: u8,
        ext: ExtendedHeader,
        data: &[u8],
        issues: &mut Vec<Issue>,
    ) -> Option<Vec<u8>> {
        let got = ext.chunk_number;
        let found = self
            .pending
            .iter()
            .position(|p| p.sop == sop && p.code == code)
            .map(|i| self.pending.swap_remove(i));
        let mut p = match found {
            Some(p) if got > 0 && p.next == got => p,
            other => {
                if let Some(p) = &other {
                    issues.push(Issue::MissingChunk {
                        expected: p.next,
                        got,
                    });
                } else if got > 0 {
                    issues.push(Issue::MissingChunk { expected: 0, got });
                }
                if got > 0 {
                    return None;
                }
                Pending {
                    sop,
                    code,
                    data_size: usize::from(ext.data_size),
                    next: 0,
                    data: Vec::new(),
                }
            }
        };
        // Every chunk but the last carries MaxExtendedMsgChunkLen bytes; the
        // last carries what remains (both padded to whole data objects). A
        // shorter chunk cannot complete the message: drop it.
        let want = p.data_size.saturating_sub(p.data.len()).min(MAX_CHUNK_LEN);
        if data.len() < want {
            issues.push(Issue::ShortBody {
                expected: want,
                got: data.len(),
            });
            return None;
        }
        p.data.extend_from_slice(&data[..want]);
        p.next = p.next.saturating_add(1);
        if p.data.len() >= p.data_size {
            return Some(p.data);
        }
        self.pending.push(p);
        None
    }
}
