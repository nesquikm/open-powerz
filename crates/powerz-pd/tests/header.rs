//! AC-TGE83B.1: the message header decodes all fields, with the Cable Plug
//! and reserved variants for SOP'/SOP'', and names every control, data and
//! extended message type of USB PD r3.2, EPR messages included.

mod common;

use common::*;
use powerz_pd::{CablePlug, DataRole, Flag, Header, MessageKind, PowerRole, Sop, SpecRevision};

#[test]
fn sop_codes_map_to_sop_kinds() {
    assert_eq!(Sop::from_raw(0), Sop::Sop);
    assert_eq!(Sop::from_raw(1), Sop::SopPrime);
    assert_eq!(Sop::from_raw(2), Sop::SopDoublePrime);
    assert_eq!(Sop::from_raw(3), Sop::SopPrimeDebug);
    assert_eq!(Sop::from_raw(4), Sop::SopDoublePrimeDebug);
    assert_eq!(Sop::from_raw(9), Sop::Other(9));
}

#[test]
fn sop_header_decodes_every_field() {
    // Source_Capabilities from the session-2 charger: 0x41A1.
    let h = Header::decode(0x41A1, Sop::Sop);
    assert_eq!(h.raw, 0x41A1);
    assert!(!h.extended);
    assert_eq!(h.num_data_objects, 4);
    assert_eq!(h.message_id, 0);
    assert_eq!(h.port_power_role, Some(PowerRole::Source));
    assert_eq!(h.cable_plug, None);
    assert_eq!(h.spec_revision, SpecRevision::Rev3);
    assert_eq!(h.port_data_role, Some(DataRole::Dfp));
    assert_eq!(h.message_type.kind(), MessageKind::Data);
    assert_eq!(h.message_type.code(), 1);
    assert_eq!(h.message_type.name(), "Source_Capabilities");

    // EPR_Mode from the sink: 0x188A (1 object, MessageID 4, Sink, UFP).
    let h = Header::decode(0x188A, Sop::Sop);
    assert_eq!(h.num_data_objects, 1);
    assert_eq!(h.message_id, 4);
    assert_eq!(h.port_power_role, Some(PowerRole::Sink));
    assert_eq!(h.port_data_role, Some(DataRole::Ufp));
    assert_eq!(h.message_type.name(), "EPR_Mode");

    // Extended_Control from the source: 0x95B0.
    let h = Header::decode(0x95B0, Sop::Sop);
    assert!(h.extended);
    assert_eq!(h.num_data_objects, 1);
    assert_eq!(h.message_id, 2);
    assert_eq!(h.message_type.kind(), MessageKind::Extended);
    assert_eq!(h.message_type.name(), "Extended_Control");
}

#[test]
fn spec_revision_values() {
    let rev = |r: u16| Header::decode(header(false, 0, 0, 0, r, 0, 1), Sop::Sop).spec_revision;
    assert_eq!(rev(0b00), SpecRevision::Rev1);
    assert_eq!(rev(0b01), SpecRevision::Rev2);
    assert_eq!(rev(0b10), SpecRevision::Rev3);
    assert_eq!(rev(0b11), SpecRevision::Reserved);
}

#[test]
fn cable_sop_headers_use_cable_plug_and_reserved_bit_5() {
    // Discover Identity ACK from the cable plug on SOP': 0x518F.
    for sop in [
        Sop::SopPrime,
        Sop::SopDoublePrime,
        Sop::SopPrimeDebug,
        Sop::SopDoublePrimeDebug,
    ] {
        let h = Header::decode(0x518F, sop);
        assert_eq!(h.port_power_role, None, "{sop:?}: bit 8 is Cable Plug");
        assert_eq!(h.cable_plug, Some(CablePlug::CablePlug), "{sop:?}");
        assert_eq!(h.port_data_role, None, "{sop:?}: bit 5 is reserved");
        assert_eq!(h.num_data_objects, 5);
        assert_eq!(h.message_type.name(), "Vendor_Defined");
    }
    // The same request from the source port: Cable Plug = 0.
    let h = Header::decode(0x108F, Sop::SopPrime);
    assert_eq!(h.cable_plug, Some(CablePlug::Port));
}

#[test]
fn header_field_tree_names_bits_and_flags() {
    let d = decode_one(
        SOP,
        0x41A1,
        &objects(&[0x0981_912C, 0x0002_D12C, 0x0004_B12C, 0x0006_41D6]),
    );
    let fields = d.fields();
    let hdr = at(&fields, &["Header"]);
    let expect = [
        ("Extended", 15, 15, 0),
        ("Number of Data Objects", 14, 12, 4),
        ("MessageID", 11, 9, 0),
        ("Port Power Role", 8, 8, 1),
        ("Specification Revision", 7, 6, 2),
        ("Port Data Role", 5, 5, 1),
        ("Message Type", 4, 0, 1),
    ];
    for (name, hi, lo, raw) in expect {
        let f = at(&hdr.children, &[name]);
        assert_eq!((f.bits.hi, f.bits.lo), (hi, lo), "{name} bits");
        assert_eq!(f.raw, raw, "{name} raw");
        assert!(!f.meaning.is_empty(), "{name} meaning");
        assert_eq!(f.flag, None, "{name} is a valid value");
    }
    assert!(
        at(&hdr.children, &["Port Power Role"])
            .meaning
            .contains("Source")
    );
    assert!(
        at(&hdr.children, &["Message Type"])
            .meaning
            .contains("Source_Capabilities")
    );

    // Cable SOP: Cable Plug at bit 8, bit 5 is Reserved and flagged if set.
    let h = header(false, 0, 0, 1, REV3, 1, 1); // GoodCRC with bit 5 set
    let d = decode_one(SOP_PRIME, h, &[]);
    let fields = d.fields();
    let hdr = at(&fields, &["Header"]);
    assert!(find(&hdr.children, "Port Power Role").is_none());
    let plug = at(&hdr.children, &["Cable Plug"]);
    assert_eq!((plug.bits.hi, plug.bits.lo, plug.raw), (8, 8, 1));
    let res = at(&hdr.children, &["Reserved"]);
    assert_eq!((res.bits.hi, res.bits.lo, res.raw), (5, 5, 1));
    assert_eq!(res.flag, Some(Flag::Reserved));
}

#[test]
fn reserved_message_types_and_revision_are_flagged() {
    let d = decode_one(SOP, header(false, 0, 0, 1, REV3, 1, 0x1A), &[]);
    assert!(d.header.message_type.is_reserved());
    let fields = d.fields();
    assert_eq!(
        at(&fields, &["Header", "Message Type"]).flag,
        Some(Flag::Reserved)
    );

    let d = decode_one(SOP, header(false, 0, 0, 1, 0b11, 1, 1), &[]);
    let fields = d.fields();
    assert_eq!(
        at(&fields, &["Header", "Specification Revision"]).flag,
        Some(Flag::Reserved)
    );
}

/// Name of the message type `code` of `kind`, through a header.
fn name(kind: MessageKind, code: u16) -> (String, bool) {
    let h = match kind {
        MessageKind::Control => header(false, 0, 0, 1, REV3, 1, code),
        MessageKind::Data => header(false, 1, 0, 1, REV3, 1, code),
        MessageKind::Extended => header(true, 1, 0, 1, REV3, 1, code),
        other => panic!("unexpected message kind {other:?}"),
    };
    let t = Header::decode(h, Sop::Sop).message_type;
    assert_eq!(t.kind(), kind);
    assert_eq!(u16::from(t.code()), code);
    (t.name().to_string(), t.is_reserved())
}

fn check_table(kind: MessageKind, table: &[(u16, &str)]) {
    for code in 0..32u16 {
        let (got, reserved) = name(kind, code);
        match table.iter().find(|(c, _)| *c == code) {
            Some((_, want)) => {
                assert_eq!(got, *want, "{kind:?} {code:#04x}");
                assert!(!reserved, "{kind:?} {code:#04x} is defined");
            }
            None => assert!(reserved, "{kind:?} {code:#04x} is reserved, got {got}"),
        }
    }
}

#[test]
fn every_control_message_type_is_named() {
    check_table(
        MessageKind::Control,
        &[
            (0x01, "GoodCRC"),
            (0x02, "GotoMin"),
            (0x03, "Accept"),
            (0x04, "Reject"),
            (0x05, "Ping"),
            (0x06, "PS_RDY"),
            (0x07, "Get_Source_Cap"),
            (0x08, "Get_Sink_Cap"),
            (0x09, "DR_Swap"),
            (0x0A, "PR_Swap"),
            (0x0B, "VCONN_Swap"),
            (0x0C, "Wait"),
            (0x0D, "Soft_Reset"),
            (0x0E, "Data_Reset"),
            (0x0F, "Data_Reset_Complete"),
            (0x10, "Not_Supported"),
            (0x11, "Get_Source_Cap_Extended"),
            (0x12, "Get_Status"),
            (0x13, "FR_Swap"),
            (0x14, "Get_PPS_Status"),
            (0x15, "Get_Country_Codes"),
            (0x16, "Get_Sink_Cap_Extended"),
            (0x17, "Get_Source_Info"),
            (0x18, "Get_Revision"),
        ],
    );
}

#[test]
fn every_data_message_type_is_named() {
    check_table(
        MessageKind::Data,
        &[
            (0x01, "Source_Capabilities"),
            (0x02, "Request"),
            (0x03, "BIST"),
            (0x04, "Sink_Capabilities"),
            (0x05, "Battery_Status"),
            (0x06, "Alert"),
            (0x07, "Get_Country_Info"),
            (0x08, "Enter_USB"),
            (0x09, "EPR_Request"),
            (0x0A, "EPR_Mode"),
            (0x0B, "Source_Info"),
            (0x0C, "Revision"),
            (0x0F, "Vendor_Defined"),
        ],
    );
}

#[test]
fn every_extended_message_type_is_named() {
    check_table(
        MessageKind::Extended,
        &[
            (0x01, "Source_Capabilities_Extended"),
            (0x02, "Status"),
            (0x03, "Get_Battery_Cap"),
            (0x04, "Get_Battery_Status"),
            (0x05, "Battery_Capabilities"),
            (0x06, "Get_Manufacturer_Info"),
            (0x07, "Manufacturer_Info"),
            (0x08, "Security_Request"),
            (0x09, "Security_Response"),
            (0x0A, "Firmware_Update_Request"),
            (0x0B, "Firmware_Update_Response"),
            (0x0C, "PPS_Status"),
            (0x0D, "Country_Info"),
            (0x0E, "Country_Codes"),
            (0x0F, "Sink_Capabilities_Extended"),
            (0x10, "Extended_Control"),
            (0x11, "EPR_Source_Capabilities"),
            (0x12, "EPR_Sink_Capabilities"),
            (0x1E, "Vendor_Defined_Extended"),
        ],
    );
}
