//! Golden tests (FR TGE83A, AC .7): PD_PACKET replies recorded on a real
//! KM003C in session 2 (`tests/vectors/pd_*.hex`, rewritten from the
//! research notes) decode to the values annotated there. PD poll replies
//! only: no serial number or UID is in these frames.

mod common;

use common::vector;
use powerz_protocol::decode::pd_packet::{
    CcLine, PdChunk, PdEvent, PdMessage, PdRecord, PdRecordKind, PdStatus,
};
use powerz_protocol::frame::{self, Attribute};

const ATTACH_26: &str = include_str!("vectors/pd_attach_26.hex");
const DETACH_334: &str = include_str!("vectors/pd_detach_334.hex");
const SRCCAP_144: &str = include_str!("vectors/pd_srccap_144.hex");
const PSRDY_36: &str = include_str!("vectors/pd_psrdy_36.hex");
const EPRMODE_60: &str = include_str!("vectors/pd_eprmode_60.hex");
const EPRCAPS_260: &str = include_str!("vectors/pd_eprcaps_260.hex");
const HARDRESET_26: &str = include_str!("vectors/pd_hardreset_26.hex");

/// Parse the single PD_PACKET section of a captured reply.
fn chunk_of(fixture: &str) -> PdChunk {
    let reply = vector(fixture);
    let sections = frame::parse_put_data(&reply).expect("PUT_DATA frame");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].att, Attribute::PD_PACKET);
    let c = PdChunk::parse(sections[0].payload).expect("PD chunk");
    assert_eq!(c.skipped, 0, "every recorded record fits its chunk");
    c
}

fn message(r: &PdRecord) -> &PdMessage {
    match &r.kind {
        PdRecordKind::Message(m) => m,
        other => panic!("expected a PD message, got {other:?}"),
    }
}

fn event(r: &PdRecord) -> &PdEvent {
    match &r.kind {
        PdRecordKind::Event(e) => e,
        other => panic!("expected an event, got {other:?}"),
    }
}

fn u32s(body: &[u8]) -> Vec<u32> {
    body.chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect()
}

#[test]
fn attach_vector() {
    let c = chunk_of(ATTACH_26);
    assert_eq!(
        c.status,
        PdStatus {
            time_ms: 0x02F7_0A63,
            vbus_mv: 516,
            ibus_ma: 0,
            cc1_mv: 1_525,
            cc2_mv: 0,
        }
    );
    assert_eq!(c.records.len(), 1);
    let r = &c.records[0];
    assert_eq!(r.time_ms, 0x02F7_0A28);
    assert!(r.from_meter, "45 ... 11: Attach CC1 caused by the meter");
    assert_eq!(event(r), &PdEvent::Attach(CcLine::Cc1));
}

#[test]
fn detach_vector_with_keep_alive_backlog() {
    let c = chunk_of(DETACH_334);
    assert_eq!(
        c.status,
        PdStatus {
            time_ms: 0x02F7_0A25,
            vbus_mv: 598,
            ibus_ma: 0,
            cc1_mv: 1_526,
            cc2_mv: 0,
        }
    );
    assert_eq!(c.records.len(), 32);

    // GoodCRC from the charger (b0 0x87), then EPR_KeepAlive_Ack (0x8B),
    // then the meter's GoodCRC (0xC7).
    let first = &c.records[0];
    assert_eq!(first.time_ms, 0x02F6_F856);
    assert!(!first.from_meter);
    assert_eq!(message(first).sop, 0);
    assert_eq!(message(first).header, 0x0761);
    assert!(message(first).body.is_empty());

    let ack = message(&c.records[1]);
    assert_eq!(ack.header, 0x95B0);
    assert_eq!(ack.body, [0x02, 0x80, 0x04, 0x00]);
    assert!(c.records[2].from_meter);
    assert_eq!(message(&c.records[2]).header, 0x0481);

    let last = c.records.last().unwrap();
    assert_eq!(last.time_ms, 0x02F7_020E);
    assert!(last.from_meter);
    assert_eq!(event(last), &PdEvent::Detach(CcLine::Cc1));
    assert_eq!(last.raw, [0x45, 0x0E, 0x02, 0xF7, 0x02, 0x12]);

    // Everything before the detach is a PD message.
    assert!(
        c.records[..31]
            .iter()
            .all(|r| matches!(r.kind, PdRecordKind::Message(_)))
    );
}

#[test]
fn source_capabilities_request_accept_vector() {
    let c = chunk_of(SRCCAP_144);
    assert_eq!(
        c.status,
        PdStatus {
            time_ms: 0x02F7_0B5C,
            vbus_mv: 5_182,
            ibus_ma: 0,
            cc1_mv: 1_528,
            cc2_mv: 3,
        }
    );
    assert_eq!(c.records.len(), 10);

    // SOP' Discover Identity REQ from the charger.
    let r = &c.records[0];
    assert_eq!(r.time_ms, 0x02F7_0B39);
    assert!(!r.from_meter);
    assert_eq!(message(r).sop, 1);
    assert_eq!(message(r).header, 0x108F);
    assert_eq!(u32s(&message(r).body), [0xFF00_A001]);

    // SOP' Discover Identity ACK from the cable: ID header 0x1860050D,
    // cable VDO 0x000A2640.
    let ack = message(&c.records[2]);
    assert_eq!(ack.sop, 1);
    assert_eq!(ack.header, 0x518F);
    let vdos = u32s(&ack.body);
    assert_eq!(vdos.len(), 5);
    assert_eq!(vdos[1], 0x1860_050D);
    assert_eq!(vdos[4], 0x000A_2640);
    assert!(!c.records[2].from_meter);

    // Source_Capabilities: 5 V 3 A, 9 V 3 A, 15 V 3 A, 20 V 4.7 A.
    let caps = &c.records[4];
    assert_eq!(caps.time_ms, 0x02F7_0B3E);
    assert!(!caps.from_meter);
    assert_eq!(message(caps).sop, 0);
    assert_eq!(message(caps).header, 0x41A1);
    assert_eq!(
        u32s(&message(caps).body),
        [0x0981_912C, 0x0002_D12C, 0x0004_B12C, 0x0006_41D6]
    );

    // Request from the meter: RDO 0x1344B12C (position 1, 3 A).
    let req = &c.records[6];
    assert_eq!(req.time_ms, 0x02F7_0B40);
    assert!(req.from_meter);
    assert_eq!(message(req).header, 0x1082);
    assert_eq!(u32s(&message(req).body), [0x1344_B12C]);

    // Accept from the charger.
    let accept = &c.records[8];
    assert_eq!(accept.time_ms, 0x02F7_0B42);
    assert!(!accept.from_meter);
    assert_eq!(message(accept).header, 0x03A3);
    assert!(message(accept).body.is_empty());
}

#[test]
fn ps_rdy_vector() {
    let c = chunk_of(PSRDY_36);
    assert_eq!(c.status.time_ms, 0x02F7_0B9A);
    assert_eq!(c.status.vbus_mv, 5_191);
    assert_eq!(c.status.cc1_mv, 1_529);
    assert_eq!(c.status.cc2_mv, 3);
    assert_eq!(c.records.len(), 2);
    let ps_rdy = &c.records[0];
    assert_eq!(ps_rdy.time_ms, 0x02F7_0B60, "30 ms after Accept");
    assert!(!ps_rdy.from_meter);
    assert_eq!(message(ps_rdy).header, 0x05A6);
    assert!(c.records[1].from_meter, "GoodCRC from the meter");
    assert_eq!(message(&c.records[1]).header, 0x0481);
}

#[test]
fn epr_mode_enter_vector() {
    let c = chunk_of(EPRMODE_60);
    assert_eq!(c.status.time_ms, 0x02F7_15C1);
    assert_eq!(c.status.vbus_mv, 5_190);
    assert_eq!(c.records.len(), 4);

    let enter = &c.records[0];
    assert!(enter.from_meter);
    assert_eq!(enter.time_ms, 0x02F7_1589);
    assert_eq!(message(enter).header, 0x188A);
    assert_eq!(u32s(&message(enter).body), [0x0100_0000]);

    let acked = &c.records[2];
    assert!(!acked.from_meter);
    assert_eq!(message(acked).header, 0x1DAA);
    assert_eq!(u32s(&message(acked).body), [0x0200_0000]);
}

#[test]
fn chunked_epr_source_capabilities_vector() {
    let c = chunk_of(EPRCAPS_260);
    assert_eq!(c.status.time_ms, 0x02F7_15FF);
    assert_eq!(c.status.vbus_mv, 5_160);
    assert_eq!(c.status.cc1_mv, 836);
    assert_eq!(c.status.cc2_mv, 3);
    assert_eq!(c.records.len(), 20);

    // SOP' Soft_Reset during cable re-discovery.
    assert_eq!(message(&c.records[0]).sop, 1);
    assert_eq!(message(&c.records[0]).header, 0x008D);

    // EPR_Mode Enter_Succeeded.
    assert_eq!(message(&c.records[8]).header, 0x1FAA);
    assert_eq!(u32s(&message(&c.records[8]).body), [0x0300_0000]);

    // Chunk 0 of EPR_Source_Capabilities: b0 0xA3 is a 36-byte message,
    // not a big record; extended header 0x8024 (chunked, 36 bytes).
    let chunk0 = &c.records[10];
    assert_eq!(chunk0.raw[0], 0xA3);
    assert_eq!(chunk0.raw.len(), 36);
    assert_eq!(chunk0.time_ms, 0x02F7_15E2);
    assert_eq!(message(chunk0).header, 0xF1B1);
    assert_eq!(&message(chunk0).body[..2], &[0x24, 0x80]);
    assert_eq!(
        u32s(&message(chunk0).body[2..18]),
        [0x0981_912C, 0x0002_D12C, 0x0004_B12C, 0x0006_41F4]
    );

    // Chunk request from the meter.
    assert!(c.records[12].from_meter);
    assert_eq!(message(&c.records[12]).header, 0x9A91);
    assert_eq!(message(&c.records[12]).body, [0x00, 0x8C, 0x00, 0x00]);

    // Chunk 1: PDO8 28 V 5 A, PDO9 EPR AVS 15-28 V 140 W.
    let chunk1 = message(&c.records[14]);
    assert_eq!(chunk1.header, 0xB3B1);
    assert_eq!(&chunk1.body[..4], &[0x24, 0x88, 0x00, 0x00]);
    assert_eq!(u32s(&chunk1.body[4..]), [0x0008_C1F4, 0xD230_968C]);

    // EPR_Request from the meter: RDO + copy of PDO1.
    let epr_req = &c.records[16];
    assert!(epr_req.from_meter);
    assert_eq!(epr_req.raw[0], 0xCF);
    assert_eq!(message(epr_req).header, 0x2C89);
    assert_eq!(u32s(&message(epr_req).body), [0x1344_B12C, 0x0981_912C]);

    // Accept.
    assert_eq!(message(&c.records[18]).header, 0x05A3);
    assert_eq!(c.records[18].time_ms, 0x02F7_15EA);
}

#[test]
fn hard_reset_vector() {
    let c = chunk_of(HARDRESET_26);
    assert_eq!(
        c.status,
        PdStatus {
            time_ms: 0x02F8_5BF3,
            vbus_mv: 666,
            ibus_ma: 0,
            cc1_mv: 1_527,
            cc2_mv: 0,
        }
    );
    assert_eq!(c.records.len(), 1);
    let r = &c.records[0];
    assert_eq!(r.time_ms, 0x02F8_5BB4);
    assert!(r.from_meter, "45 ... 04: Hard Reset sent by the meter");
    assert_eq!(event(r), &PdEvent::HardReset);
}
