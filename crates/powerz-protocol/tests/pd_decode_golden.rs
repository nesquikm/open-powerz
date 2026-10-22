//! Golden tests (FR TGE83B, AC .8 and .3): the session-2 PD_PACKET replies
//! (`tests/vectors/pd_*.hex`, FR TGE83A) fed record by record through the
//! PD decoder (`powerz_protocol::pd`, the `powerz-pd` crate) via
//! `PdRecord::pd_input`. Values match the annotations in the session-2
//! notes: Source_Capabilities 5 V 3 A .. 20 V 4.7 A, RDO 0x1344B12C =
//! position 1 3 A, EPR_Mode entry, chunked EPR_Source_Capabilities with
//! PDO8 28 V 5 A and PDO9 EPR AVS 15..28 V 140 W, EPR_KeepAlive / Ack and
//! the cable's Discover Identity ACK.

mod common;

use common::vector;
use powerz_protocol::decode::pd_packet::{PdChunk, PdRecord};
use powerz_protocol::frame;
use powerz_protocol::pd::{
    Capture, CommandType, Decoded, EcdbType, EprAction, EprAvs, EprMode, ExtendedControl, FixedRdo,
    FixedSource, Input, Issue, Message, ProductTypeVdo, Rdo, SourcePdo, VdmBody, VdmCommand,
    VdmHeader,
};

const SRCCAP_144: &str = include_str!("vectors/pd_srccap_144.hex");
const PSRDY_36: &str = include_str!("vectors/pd_psrdy_36.hex");
const EPRMODE_60: &str = include_str!("vectors/pd_eprmode_60.hex");
const EPRCAPS_260: &str = include_str!("vectors/pd_eprcaps_260.hex");
const DETACH_334: &str = include_str!("vectors/pd_detach_334.hex");
const HARDRESET_26: &str = include_str!("vectors/pd_hardreset_26.hex");

fn records(fixture: &str) -> Vec<PdRecord> {
    let reply = vector(fixture);
    let sections = frame::parse_put_data(&reply).expect("PUT_DATA frame");
    PdChunk::parse(sections[0].payload)
        .expect("PD chunk")
        .records
}

/// Feed every record of `fixture`; one entry per record (None for events).
fn decode_all(cap: &mut Capture, fixture: &str) -> Vec<Option<Decoded>> {
    records(fixture)
        .iter()
        .map(|r| r.pd_input().and_then(|i| cap.feed(i)))
        .collect()
}

#[track_caller]
fn msg(d: &[Option<Decoded>], i: usize) -> &Decoded {
    d[i].as_ref()
        .unwrap_or_else(|| panic!("record {i} decodes to a message"))
}

#[test]
fn record_bridge_maps_messages_and_events() {
    let recs = records(SRCCAP_144);
    assert_eq!(
        recs[4].pd_input(),
        Some(Input::Message {
            sop: 0,
            header: 0x41A1,
            body: &[
                0x2C, 0x91, 0x81, 0x09, 0x2C, 0xD1, 0x02, 0x00, 0x2C, 0xB1, 0x04, 0x00, 0xD6, 0x41,
                0x06, 0x00
            ],
        })
    );
    let detach = records(DETACH_334);
    assert_eq!(
        detach.last().and_then(PdRecord::pd_input),
        Some(Input::Detach)
    );
    assert_eq!(records(HARDRESET_26)[0].pd_input(), Some(Input::HardReset));
}

#[test]
fn session2_spr_negotiation() {
    let mut cap = Capture::new();
    let d = decode_all(&mut cap, SRCCAP_144);
    assert_eq!(d.len(), 10);

    // SOP' Discover Identity REQ and the cable's ACK.
    let req = msg(&d, 0);
    let Message::Vdm(v) = &req.message else {
        panic!("{:?}", req.message)
    };
    let VdmHeader::Structured(h) = &v.header else {
        panic!("{:?}", v.header)
    };
    assert_eq!(
        (h.command, h.command_type),
        (VdmCommand::DiscoverIdentity, CommandType::Req)
    );
    let ack = msg(&d, 2);
    let Message::Vdm(v) = &ack.message else {
        panic!("{:?}", ack.message)
    };
    let VdmBody::DiscoverIdentity(id) = &v.body else {
        panic!("{:?}", v.body)
    };
    assert_eq!(id.id_header.vid, 0x050D);
    assert!(matches!(
        id.product_type_vdos.first(),
        Some(ProductTypeVdo::PassiveCable(c)) if c.epr_capable && c.max_vbus_voltage == 3 && c.vbus_current == 2
    ));

    let caps = msg(&d, 4);
    assert_eq!(
        caps.summary(),
        "Source_Capabilities: Fixed 5.00 V 3.00 A, Fixed 9.00 V 3.00 A, \
         Fixed 15.00 V 3.00 A, Fixed 20.00 V 4.70 A"
    );

    let request = msg(&d, 6);
    assert_eq!(request.summary(), "Request: #1 Fixed 5.00 V 3.00 A");
    let Message::Request(r) = &request.message else {
        panic!("{:?}", request.message)
    };
    assert_eq!(r.pdo, Some(SourcePdo::decode(0x0981_912C)));
    assert!(matches!(
        r.rdo,
        Rdo::Fixed(FixedRdo {
            position: 1,
            operating_current_ma: 3_000,
            max_current_ma: 3_000,
            ..
        })
    ));
    assert_eq!(msg(&d, 8).summary(), "Accept");

    let d = decode_all(&mut cap, PSRDY_36);
    assert_eq!(msg(&d, 0).summary(), "PS_RDY");
    assert_eq!(msg(&d, 1).summary(), "GoodCRC");
}

#[test]
fn session2_epr_entry_chunked_caps_and_epr_request() {
    let mut cap = Capture::new();
    decode_all(&mut cap, SRCCAP_144);
    let d = decode_all(&mut cap, EPRMODE_60);
    assert_eq!(
        msg(&d, 0).message,
        Message::EprMode(EprMode {
            action: EprAction::Enter,
            data: 0
        })
    );
    assert_eq!(
        msg(&d, 2).message,
        Message::EprMode(EprMode {
            action: EprAction::EnterAcknowledged,
            data: 0
        })
    );

    let d = decode_all(&mut cap, EPRCAPS_260);
    assert_eq!(d.len(), 20);
    assert_eq!(
        msg(&d, 0).header.message_type.name(),
        "Soft_Reset",
        "SOP' Soft_Reset"
    );
    assert_eq!(
        msg(&d, 8).message,
        Message::EprMode(EprMode {
            action: EprAction::EnterSucceeded,
            data: 0
        })
    );
    assert!(matches!(
        msg(&d, 10).message,
        Message::Chunk {
            chunk_number: 0,
            ..
        }
    ));
    assert!(matches!(
        msg(&d, 12).message,
        Message::ChunkRequest { chunk_number: 1 }
    ));

    let caps = msg(&d, 14);
    assert!(caps.issues.is_empty(), "{:?}", caps.issues);
    let Message::EprSourceCapabilities(pdos) = &caps.message else {
        panic!("{:?}", caps.message)
    };
    assert_eq!(pdos.len(), 9);
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
    assert_eq!(
        pdos[8],
        SourcePdo::EprAvs(EprAvs {
            raw: 0xD230_968C,
            peak_current: 0,
            max_voltage_mv: 28_000,
            min_voltage_mv: 15_000,
            pdp_w: 140,
        })
    );

    let epr_req = msg(&d, 16);
    let Message::EprRequest(r) = &epr_req.message else {
        panic!("{:?}", epr_req.message)
    };
    assert_eq!(r.pdo, Some(SourcePdo::decode(0x0981_912C)));
    assert!(matches!(
        r.rdo,
        Rdo::Fixed(FixedRdo {
            position: 1,
            operating_current_ma: 3_000,
            ..
        })
    ));
    assert_eq!(msg(&d, 18).summary(), "Accept");
}

#[test]
fn session2_keep_alive_then_detach_clears_caps() {
    let mut cap = Capture::new();
    decode_all(&mut cap, SRCCAP_144);
    let d = decode_all(&mut cap, DETACH_334);
    assert_eq!(d.len(), 32);
    assert_eq!(
        msg(&d, 1).message,
        Message::ExtendedControl(ExtendedControl {
            kind: EcdbType::EprKeepAliveAck,
            data: 0
        })
    );
    assert_eq!(msg(&d, 1).summary(), "Extended_Control: EPR_KeepAlive_Ack");
    assert_eq!(
        msg(&d, 3).message,
        Message::ExtendedControl(ExtendedControl {
            kind: EcdbType::EprKeepAlive,
            data: 0
        })
    );
    assert!(d[31].is_none(), "the detach event decodes to nothing");

    // After the detach the remembered caps are gone.
    let request = records(SRCCAP_144).remove(6);
    let after = cap
        .feed(request.pd_input().expect("message"))
        .expect("decoded");
    let Message::Request(r) = &after.message else {
        panic!("{:?}", after.message)
    };
    assert_eq!(r.pdo, None);
    assert!(after.issues.contains(&Issue::NoSourceCapabilities));
}

#[test]
fn session2_hard_reset_clears_caps() {
    let mut cap = Capture::new();
    decode_all(&mut cap, SRCCAP_144);
    decode_all(&mut cap, HARDRESET_26);
    let request = records(SRCCAP_144).remove(6);
    let after = cap
        .feed(request.pd_input().expect("message"))
        .expect("decoded");
    assert!(
        after.issues.contains(&Issue::NoSourceCapabilities),
        "{:?}",
        after.issues
    );
}
