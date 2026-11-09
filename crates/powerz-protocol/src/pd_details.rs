//! The details of one captured PD record (FR TGE83C, FR QFTCX0): its raw
//! bytes and its decoded field tree mapped onto those bytes. Pure; shared by
//! the desktop backend and the web edition.

use crate::decode::pd_packet::{PdRecord, PdRecordKind};
use crate::pd::{Decoded, Field};
use serde::Serialize;

/// One node of a record's field tree, with the bytes of the record's raw
/// bytes it covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldNode {
    /// Field name.
    pub name: String,
    /// Human-readable value.
    pub meaning: String,
    /// The field's bits, shifted down to bit 0.
    pub raw: u64,
    /// `[start, end)` byte range in the record's raw bytes.
    pub bytes: [usize; 2],
    /// Sub-fields.
    pub children: Vec<FieldNode>,
}

/// The bytes a record's details show: the PD message bytes (header
/// little-endian, then the body) of a message, the record bytes otherwise.
pub fn detail_bytes(record: &PdRecord) -> Vec<u8> {
    match &record.kind {
        PdRecordKind::Message(m) | PdRecordKind::BigMessage(m) => {
            let mut raw = m.header.to_le_bytes().to_vec();
            raw.extend_from_slice(&m.body);
            raw
        }
        PdRecordKind::Event(_) => record.raw.clone(),
    }
}

/// Byte range of bits `hi..=lo` of the word at byte `word`.
fn bit_bytes(word: usize, hi: u16, lo: u16) -> [usize; 2] {
    [word + usize::from(lo) / 8, word + usize::from(hi) / 8 + 1]
}

/// `field` with its byte range: the node covers `span`, its children the
/// bits of the word at byte `word`; every range is clamped to `len`.
fn node(field: &Field, span: [usize; 2], word: usize, len: usize) -> FieldNode {
    let clamp = |[s, e]: [usize; 2]| [s.min(len), e.min(len)];
    let span = clamp(span);
    FieldNode {
        name: field.name.clone(),
        meaning: field.meaning.clone(),
        raw: field.raw,
        bytes: span,
        children: field
            .children
            .iter()
            .map(|c| {
                let r = clamp(bit_bytes(word, c.bits.hi, c.bits.lo));
                // A child stays inside its parent.
                let r = [r[0].clamp(span[0], span[1]), r[1].clamp(span[0], span[1])];
                node(c, r, word, len)
            })
            .collect(),
    }
}

/// The field tree of `decoded` mapped onto its `len` raw bytes (see
/// [`detail_bytes`]): the header and the extended header take 2 bytes each,
/// every data object 4.
pub fn field_nodes(decoded: &Decoded, len: usize) -> Vec<FieldNode> {
    let mut at = 0;
    decoded
        .fields()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let size = if i == 0 || (i == 1 && decoded.extended_header.is_some()) {
                2
            } else {
                4
            };
            let n = node(f, [at, at + size], at, len);
            at += size;
            n
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pd::{Capture, Input};

    #[test]
    fn field_nodes_cover_the_header_then_each_object() {
        // Source_Capabilities, one fixed 5 V / 3 A PDO.
        let header: u16 = 0x1161;
        let body = 0x0001_912Cu32.to_le_bytes();
        let decoded = Capture::new()
            .feed(Input::Message {
                sop: 0,
                header,
                body: &body,
            })
            .expect("decoded");
        let nodes = field_nodes(&decoded, 6);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].bytes, [0, 2]);
        assert_eq!(nodes[1].bytes, [2, 6]);
        for c in &nodes[0].children {
            assert!(c.bytes[1] <= 2, "{c:?}");
        }
    }

    #[test]
    fn ranges_clamp_to_short_bytes() {
        let header: u16 = 0x1161;
        let body = 0x0001_912Cu32.to_le_bytes();
        let decoded = Capture::new()
            .feed(Input::Message {
                sop: 0,
                header,
                body: &body,
            })
            .expect("decoded");
        let nodes = field_nodes(&decoded, 3);
        assert_eq!(nodes[1].bytes, [2, 3]);
    }
}
