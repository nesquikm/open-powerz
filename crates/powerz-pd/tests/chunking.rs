//! AC-TGE83B.6: chunked extended messages (such as a chunked
//! EPR_Source_Capabilities) are reassembled across records into one decoded
//! message, and a missing chunk is reported. Chunks carry at most 26 data
//! bytes (USB PD r3.2, MaxExtendedMsgChunkLen).

mod common;

use common::*;
use powerz_pd::{Capture, EprAvs, FixedSource, Issue, Message, SourcePdo};

const EPR_SOURCE_CAPABILITIES: u16 = 0x11;
const VENDOR_DEFINED_EXTENDED: u16 = 0x1E;
const CHUNK: usize = 26;

/// Session-2 EPR caps layout: 4 SPR fixed PDOs, three empty slots, then
/// EPR PDO8 28 V 5 A and PDO9 EPR AVS 15..28 V 140 W.
const EPR_PDOS: [u32; 9] = [
    0x0981_912C,
    0x0002_D12C,
    0x0004_B12C,
    0x0006_41F4,
    0,
    0,
    0,
    0x0008_C1F4,
    0xD230_968C,
];

/// Chunk `n` of an extended message carrying `data`, from the source.
fn chunk(ty: u16, data: &[u8], n: usize) -> (u16, Vec<u8>) {
    let start = n * CHUNK;
    let end = (start + CHUNK).min(data.len());
    extended(
        ty,
        ext_header(true, n as u16, false, data.len() as u16),
        &data[start..end],
        true,
    )
}

/// A chunk request for chunk `n` from the sink.
fn chunk_request(ty: u16, n: u16) -> (u16, Vec<u8>) {
    extended(ty, ext_header(true, n, true, 0), &[], false)
}

fn feed_chunk(cap: &mut Capture, (h, body): (u16, Vec<u8>)) -> powerz_pd::Decoded {
    feed(cap, SOP, h, &body)
}

#[test]
fn chunked_epr_source_capabilities_reassemble() {
    let data = objects(&EPR_PDOS);
    assert_eq!(data.len(), 36);
    let mut cap = Capture::new();

    let d0 = feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 0));
    assert!(
        matches!(
            d0.message,
            Message::Chunk {
                chunk_number: 0,
                ..
            }
        ),
        "chunk 0 alone is a partial message: {:?}",
        d0.message
    );
    assert!(d0.issues.is_empty(), "{:?}", d0.issues);

    // GoodCRC and the sink's chunk request in between do not disturb it.
    feed(&mut cap, SOP, header(false, 0, 0, 0, REV3, 0, 1), &[]);
    let req = feed_chunk(&mut cap, chunk_request(EPR_SOURCE_CAPABILITIES, 1));
    assert!(
        matches!(req.message, Message::ChunkRequest { chunk_number: 1 }),
        "{:?}",
        req.message
    );
    assert!(req.extended_header.is_some_and(|x| x.request_chunk));

    let d1 = feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 1));
    assert!(d1.issues.is_empty(), "{:?}", d1.issues);
    match &d1.message {
        Message::EprSourceCapabilities(pdos) => {
            assert_eq!(pdos.len(), 9);
            assert_eq!(pdos[0], SourcePdo::decode(0x0981_912C));
            assert!(matches!(
                pdos[3],
                SourcePdo::Fixed(FixedSource {
                    voltage_mv: 20_000,
                    max_current_ma: 5_000,
                    ..
                })
            ));
            assert!(matches!(
                pdos[7],
                SourcePdo::Fixed(FixedSource {
                    voltage_mv: 28_000,
                    max_current_ma: 5_000,
                    ..
                })
            ));
            assert!(matches!(
                pdos[8],
                SourcePdo::EprAvs(EprAvs {
                    min_voltage_mv: 15_000,
                    max_voltage_mv: 28_000,
                    pdp_w: 140,
                    ..
                })
            ));
        }
        other => panic!("expected the reassembled EPR_Source_Capabilities, got {other:?}"),
    }
    let s = d1.summary();
    assert!(s.starts_with("EPR_Source_Capabilities"), "{s}");
    assert!(s.contains("28.00 V"), "{s}");
    let fields = d1.fields();
    assert_eq!(at(&fields, &["PDO 8", "Voltage"]).meaning, "28.00 V");
    assert_eq!(at(&fields, &["PDO 9", "PDP"]).raw, 140);
}

fn vde_data() -> Vec<u8> {
    (0..60u8).collect()
}

#[test]
fn three_chunk_message_reassembles_in_order() {
    let data = vde_data();
    let mut cap = Capture::new();
    for n in 0..2 {
        let d = feed_chunk(&mut cap, chunk(VENDOR_DEFINED_EXTENDED, &data, n));
        assert!(
            matches!(d.message, Message::Chunk { .. }),
            "chunk {n}: {:?}",
            d.message
        );
    }
    let d = feed_chunk(&mut cap, chunk(VENDOR_DEFINED_EXTENDED, &data, 2));
    assert!(d.issues.is_empty(), "{:?}", d.issues);
    assert!(
        !matches!(d.message, Message::Chunk { .. }),
        "{:?}",
        d.message
    );
    assert!(
        d.summary().starts_with("Vendor_Defined_Extended"),
        "{}",
        d.summary()
    );
    assert_eq!(d.extended_header.map(|x| x.data_size), Some(60));
}

#[test]
fn missing_middle_chunk_is_reported() {
    let data = vde_data();
    let mut cap = Capture::new();
    feed_chunk(&mut cap, chunk(VENDOR_DEFINED_EXTENDED, &data, 0));
    let d = feed_chunk(&mut cap, chunk(VENDOR_DEFINED_EXTENDED, &data, 2));
    assert!(
        d.issues.contains(&Issue::MissingChunk {
            expected: 1,
            got: 2
        }),
        "{:?}",
        d.issues
    );
}

#[test]
fn out_of_order_first_chunk_is_reported_then_recovers() {
    let data = objects(&EPR_PDOS);
    let mut cap = Capture::new();
    let d = feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 1));
    assert!(
        d.issues.contains(&Issue::MissingChunk {
            expected: 0,
            got: 1
        }),
        "{:?}",
        d.issues
    );
    assert!(!matches!(d.message, Message::EprSourceCapabilities(_)));

    feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 0));
    let d = feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 1));
    assert!(d.issues.is_empty(), "{:?}", d.issues);
    assert!(matches!(&d.message, Message::EprSourceCapabilities(p) if p.len() == 9));
}

#[test]
fn restarted_message_reports_the_lost_tail() {
    let data = objects(&EPR_PDOS);
    let mut cap = Capture::new();
    feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 0));
    let d = feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 0));
    assert!(
        d.issues.contains(&Issue::MissingChunk {
            expected: 1,
            got: 0
        }),
        "{:?}",
        d.issues
    );
    let d = feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 1));
    assert!(d.issues.is_empty(), "{:?}", d.issues);
    assert!(matches!(&d.message, Message::EprSourceCapabilities(p) if p.len() == 9));
}

#[test]
fn hard_reset_drops_a_pending_message() {
    let data = objects(&EPR_PDOS);
    let mut cap = Capture::new();
    feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 0));
    cap.feed(powerz_pd::Input::HardReset);
    let d = feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 1));
    assert!(
        d.issues.contains(&Issue::MissingChunk {
            expected: 0,
            got: 1
        }),
        "{:?}",
        d.issues
    );
}

#[test]
fn an_empty_chunk_does_not_complete_a_message() {
    let data = objects(&EPR_PDOS);
    let mut cap = Capture::new();
    feed_chunk(&mut cap, chunk(EPR_SOURCE_CAPABILITIES, &data, 0));
    // Chunk 1 arrives with no data at all.
    let (h, body) = extended(
        EPR_SOURCE_CAPABILITIES,
        ext_header(true, 1, false, data.len() as u16),
        &[],
        true,
    );
    let d = feed(&mut cap, SOP, h, &body);
    assert!(
        !matches!(d.message, Message::EprSourceCapabilities(_)),
        "{:?}",
        d.message
    );
    assert!(!d.issues.is_empty(), "the empty chunk went unreported");
}
