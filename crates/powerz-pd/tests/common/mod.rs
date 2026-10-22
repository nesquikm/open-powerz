//! Shared helpers for the `powerz-pd` integration tests (FR TGE83B).
//!
//! Messages are built here from the field layouts of USB PD r3.2, so every
//! test vector is written in our own words from the public spec.

#![allow(dead_code)]

use powerz_pd::{Capture, Decoded, Field, Input};

/// SOP codes as the meter reports them in a record's SOP byte.
pub const SOP: u8 = 0;
pub const SOP_PRIME: u8 = 1;
pub const SOP_DPRIME: u8 = 2;

/// Spec revision 3.x in the header's bits 7..6.
pub const REV3: u16 = 0b10;

/// Build a 16-bit PD message header.
///
/// `role` is bit 8 (Port Power Role on SOP, Cable Plug on SOP'/SOP''),
/// `data_role` is bit 5 (Port Data Role on SOP, reserved on cable SOP).
pub fn header(
    extended: bool,
    ndo: u16,
    id: u16,
    role: u16,
    rev: u16,
    data_role: u16,
    ty: u16,
) -> u16 {
    (u16::from(extended) << 15)
        | ((ndo & 7) << 12)
        | ((id & 7) << 9)
        | ((role & 1) << 8)
        | ((rev & 3) << 6)
        | ((data_role & 1) << 5)
        | (ty & 0x1F)
}

/// Header of a control message from the source (DFP) on SOP.
pub fn control(ty: u16) -> u16 {
    header(false, 0, 0, 1, REV3, 1, ty)
}

/// Header of a data message with `ndo` objects from the source (DFP) on SOP.
pub fn data_from_source(ndo: u16, ty: u16) -> u16 {
    header(false, ndo, 0, 1, REV3, 1, ty)
}

/// Header of a data message with `ndo` objects from the sink (UFP) on SOP.
pub fn data_from_sink(ndo: u16, ty: u16) -> u16 {
    header(false, ndo, 0, 0, REV3, 0, ty)
}

/// Little-endian bytes of a list of 32-bit data objects.
pub fn objects(dos: &[u32]) -> Vec<u8> {
    dos.iter().flat_map(|d| d.to_le_bytes()).collect()
}

/// Build a 16-bit extended message header.
pub fn ext_header(chunked: bool, chunk: u16, request_chunk: bool, size: u16) -> u16 {
    (u16::from(chunked) << 15)
        | ((chunk & 0xF) << 11)
        | (u16::from(request_chunk) << 10)
        | (size & 0x1FF)
}

/// One extended message: PD header + extended header + `data`, padded to a
/// whole number of 4-byte data objects (as on the wire). Returns
/// `(pd_header, body)`.
pub fn extended(ty: u16, ext: u16, data: &[u8], from_source: bool) -> (u16, Vec<u8>) {
    let mut body = ext.to_le_bytes().to_vec();
    body.extend_from_slice(data);
    while !body.len().is_multiple_of(4) {
        body.push(0);
    }
    let ndo = (body.len() / 4) as u16;
    let role = u16::from(from_source);
    (header(true, ndo, 0, role, REV3, role, ty), body)
}

/// An unchunked-in-one-chunk extended message as most chargers send it
/// (Chunked = 1, chunk 0, data size = `data.len()`).
pub fn extended_single(ty: u16, data: &[u8], from_source: bool) -> (u16, Vec<u8>) {
    extended(
        ty,
        ext_header(true, 0, false, data.len() as u16),
        data,
        from_source,
    )
}

/// Feed one PD message to `cap`; panics if the decoder returns nothing.
#[track_caller]
pub fn feed(cap: &mut Capture, sop: u8, header: u16, body: &[u8]) -> Decoded {
    cap.feed(Input::Message { sop, header, body })
        .expect("a PD message always decodes to something")
}

/// Decode one message in a fresh capture.
#[track_caller]
pub fn decode_one(sop: u8, header: u16, body: &[u8]) -> Decoded {
    feed(&mut Capture::new(), sop, header, body)
}

/// Depth-first search for the first field named `name` in `fields`.
pub fn find<'a>(fields: &'a [Field], name: &str) -> Option<&'a Field> {
    for f in fields {
        if f.name == name {
            return Some(f);
        }
        if let Some(hit) = find(&f.children, name) {
            return Some(hit);
        }
    }
    None
}

/// Walk `path`: each step is searched depth-first inside the previous hit.
#[track_caller]
pub fn at<'a>(fields: &'a [Field], path: &[&str]) -> &'a Field {
    let mut scope = fields;
    let mut hit: Option<&Field> = None;
    for step in path {
        let f = find(scope, step)
            .unwrap_or_else(|| panic!("no field {step:?} on path {path:?} in {fields:#?}"));
        scope = &f.children;
        hit = Some(f);
    }
    hit.expect("non-empty path")
}

/// Every field in the tree, depth-first.
pub fn all_fields(fields: &[Field]) -> Vec<&Field> {
    let mut out = Vec::new();
    for f in fields {
        out.push(f);
        out.extend(all_fields(&f.children));
    }
    out
}
