//! AC-TGE83B.5: extended messages decode their extended header and data
//! blocks (SCEDB, SDB, BCDB, GBCDB, GBSDB, MIDB, PPSSDB, SKEDB, ECDB). The
//! data messages Battery_Status, Sink_Capabilities, Alert, BIST,
//! Get_Country_Info, Enter_USB, EPR_Mode, Source_Info and Revision decode
//! as well. Data-block bit ranges count from bit 0 of the block's first byte.

mod common;

use common::*;
use powerz_pd::{EcdbType, EprAction, EprMode, ExtendedControl, ExtendedHeader, Flag, Message};

fn ext(ty: u16, data: &[u8]) -> powerz_pd::Decoded {
    let (h, body) = extended_single(ty, data, true);
    decode_one(SOP, h, &body)
}

fn data_msg(ty: u16, dobj: u32) -> powerz_pd::Decoded {
    decode_one(SOP, data_from_source(1, ty), &objects(&[dobj]))
}

#[test]
fn extended_header_fields() {
    let d = ext(0x02, &[45, 0x02, 0, 0x02, 0, 0, 0]);
    assert_eq!(
        d.extended_header,
        Some(ExtendedHeader {
            raw: 0x8007,
            chunked: true,
            chunk_number: 0,
            request_chunk: false,
            data_size: 7,
        })
    );
    let fields = d.fields();
    let eh = at(&fields, &["Extended Header"]);
    for (name, hi, lo, raw) in [
        ("Chunked", 15, 15, 1),
        ("Chunk Number", 14, 11, 0),
        ("Request Chunk", 10, 10, 0),
        ("Data Size", 8, 0, 7),
    ] {
        let f = at(&eh.children, &[name]);
        assert_eq!((f.bits.hi, f.bits.lo, f.raw), (hi, lo, raw), "{name}");
    }
    // A non-extended message has no extended header.
    assert_eq!(decode_one(SOP, control(1), &[]).extended_header, None);
}

#[test]
fn source_capabilities_extended_scedb() {
    let mut b = vec![
        0x34, 0x12, 0x78, 0x56, 0xCD, 0xAB, 0, 0, 1, 2, 0, 3, 0x07, 0,
    ];
    b.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // Peak Current 1..3
    b.extend_from_slice(&[0, 0x01, 0, 65, 140]);
    assert_eq!(b.len(), 25);
    let d = ext(0x01, &b);
    let fields = d.fields();
    let vid = at(&fields, &["SCEDB", "VID"]);
    assert_eq!((vid.bits.hi, vid.bits.lo, vid.raw), (15, 0, 0x1234));
    let pid = at(&fields, &["SCEDB", "PID"]);
    assert_eq!((pid.bits.hi, pid.bits.lo, pid.raw), (31, 16, 0x5678));
    assert_eq!(at(&fields, &["SCEDB", "XID"]).raw, 0xABCD);
    assert_eq!(at(&fields, &["SCEDB", "Holdup Time"]).raw, 3);
    let spr = at(&fields, &["SCEDB", "SPR Source PDP Rating"]);
    assert_eq!((spr.bits.hi, spr.bits.lo, spr.raw), (191, 184, 65));
    assert_eq!(spr.meaning, "65 W");
    assert_eq!(at(&fields, &["SCEDB", "EPR Source PDP Rating"]).raw, 140);
    assert!(d.summary().starts_with("Source_Capabilities_Extended"));
}

#[test]
fn status_sdb() {
    let d = ext(0x02, &[45, 0x02, 0, 0x02, 0, 0, 0]);
    let fields = d.fields();
    let t = at(&fields, &["SDB", "Internal Temp"]);
    assert_eq!((t.bits.hi, t.bits.lo, t.raw), (7, 0, 45));
    assert!(t.meaning.contains("45"), "{}", t.meaning);
    assert_eq!(at(&fields, &["SDB", "Present Input"]).raw, 0x02);
    let ev = at(&fields, &["SDB", "Event Flags"]);
    assert_eq!((ev.bits.hi, ev.bits.lo, ev.raw), (31, 24, 0x02));
    assert!(ev.meaning.contains("OCP"), "{}", ev.meaning);
    assert!(find(&fields, "Power State Change").is_some());
}

#[test]
fn battery_cap_and_status_requests() {
    let d = ext(0x03, &[4]);
    assert_eq!(at(&d.fields(), &["GBCDB", "Battery Cap Ref"]).raw, 4);
    let d = ext(0x04, &[1]);
    assert_eq!(at(&d.fields(), &["GBSDB", "Battery Status Ref"]).raw, 1);
    // References above 7 are invalid.
    let d = ext(0x03, &[9]);
    assert_eq!(
        at(&d.fields(), &["GBCDB", "Battery Cap Ref"]).flag,
        Some(Flag::Invalid)
    );
}

#[test]
fn battery_capabilities_bcdb() {
    let d = ext(
        0x05,
        &[0x34, 0x12, 0x78, 0x56, 0xF4, 0x01, 0xE0, 0x01, 0x00],
    );
    let fields = d.fields();
    let cap = at(&fields, &["BCDB", "Battery Design Capacity"]);
    assert_eq!((cap.bits.hi, cap.bits.lo, cap.raw), (47, 32, 500));
    assert_eq!(cap.meaning, "50.0 Wh");
    assert_eq!(
        at(&fields, &["BCDB", "Battery Last Full Charge Capacity"]).raw,
        480
    );
    assert_eq!(
        at(&fields, &["BCDB", "Invalid Battery Reference"]).flag,
        None
    );

    let d = ext(0x05, &[0x34, 0x12, 0x78, 0x56, 0, 0, 0, 0, 0x01]);
    let fields = d.fields();
    let inv = at(&fields, &["BCDB", "Invalid Battery Reference"]);
    assert_eq!(inv.raw, 1);
    assert_eq!(inv.flag, Some(Flag::Invalid));
}

#[test]
fn manufacturer_info_midb() {
    let mut b = vec![0x34, 0x12, 0x78, 0x56];
    b.extend_from_slice(b"ACME PD");
    let d = ext(0x07, &b);
    let fields = d.fields();
    assert_eq!(at(&fields, &["MIDB", "Manufacturer VID"]).raw, 0x1234);
    assert_eq!(at(&fields, &["MIDB", "PID"]).raw, 0x5678);
    assert_eq!(
        at(&fields, &["MIDB", "Manufacturer String"]).meaning,
        "ACME PD"
    );
    assert!(d.summary().contains("ACME PD"), "{}", d.summary());
}

#[test]
fn pps_status_ppssdb() {
    let d = ext(0x0C, &[0xC2, 0x01, 0xFF, 0x00]);
    let fields = d.fields();
    let v = at(&fields, &["PPSSDB", "Output Voltage"]);
    assert_eq!((v.bits.hi, v.bits.lo, v.raw), (15, 0, 450));
    assert_eq!(v.meaning, "9.00 V");
    let i = at(&fields, &["PPSSDB", "Output Current"]);
    assert_eq!((i.bits.hi, i.bits.lo, i.raw), (23, 16, 0xFF));
    assert!(
        i.meaning.to_lowercase().contains("not supported"),
        "{}",
        i.meaning
    );
    assert!(find(&fields, "Real Time Flags").is_some());
}

#[test]
fn sink_capabilities_extended_skedb() {
    let mut b = vec![
        0x34, 0x12, 0x78, 0x56, 0, 0, 0, 0, 1, 2, 1, 0, 0, 0, 0, 0, 0, 0,
    ];
    b.extend_from_slice(&[15, 60, 100, 0, 0, 0]);
    assert_eq!(b.len(), 24);
    let d = ext(0x0F, &b);
    let fields = d.fields();
    assert_eq!(at(&fields, &["SKEDB", "SKEDB Version"]).raw, 1);
    assert_eq!(at(&fields, &["SKEDB", "SPR Sink Minimum PDP"]).raw, 15);
    assert_eq!(at(&fields, &["SKEDB", "SPR Sink Operational PDP"]).raw, 60);
    let max = at(&fields, &["SKEDB", "SPR Sink Maximum PDP"]);
    assert_eq!((max.bits.hi, max.bits.lo, max.raw), (167, 160, 100));
    assert_eq!(max.meaning, "100 W");
    assert!(find(&fields, "EPR Sink Maximum PDP").is_some());
}

#[test]
fn extended_control_ecdb() {
    for (ty, kind, name) in [
        (1u8, EcdbType::EprGetSourceCap, "EPR_Get_Source_Cap"),
        (2, EcdbType::EprGetSinkCap, "EPR_Get_Sink_Cap"),
        (3, EcdbType::EprKeepAlive, "EPR_KeepAlive"),
        (4, EcdbType::EprKeepAliveAck, "EPR_KeepAlive_Ack"),
    ] {
        let d = ext(0x10, &[ty, 0]);
        assert_eq!(
            d.message,
            Message::ExtendedControl(ExtendedControl { kind, data: 0 })
        );
        assert!(d.summary().contains(name), "{}", d.summary());
        assert_eq!(at(&d.fields(), &["ECDB", "Type"]).raw, u64::from(ty));
    }
    let d = ext(0x10, &[9, 0]);
    assert_eq!(
        d.message,
        Message::ExtendedControl(ExtendedControl {
            kind: EcdbType::Reserved(9),
            data: 0
        })
    );
    assert_eq!(
        at(&d.fields(), &["ECDB", "Type"]).flag,
        Some(Flag::Reserved)
    );
}

#[test]
fn battery_status_bsdo() {
    let d = data_msg(0x05, 0x01F4_0600);
    let fields = d.fields();
    let cap = at(&fields, &["BSDO", "Battery Present Capacity"]);
    assert_eq!((cap.bits.hi, cap.bits.lo, cap.raw), (31, 16, 500));
    assert_eq!(cap.meaning, "50.0 Wh");
    let info = at(&fields, &["BSDO", "Battery Info"]);
    assert_eq!((info.bits.hi, info.bits.lo, info.raw), (15, 8, 0x06));
    let d = data_msg(0x05, 0xFFFF_0200);
    let cap = at(&d.fields(), &["BSDO", "Battery Present Capacity"])
        .meaning
        .to_lowercase();
    assert!(cap.contains("unknown"), "{cap}");
}

#[test]
fn alert_ado() {
    // Type of Alert, USB PD r3.2: bit 1 Battery Status Change, bit 2 OCP.
    let d = data_msg(0x06, 0x0400_0000);
    let fields = d.fields();
    let t = at(&fields, &["ADO", "Type of Alert"]);
    assert_eq!((t.bits.hi, t.bits.lo, t.raw), (31, 24, 0x04));
    assert!(t.meaning.contains("OCP"), "{}", t.meaning);
    let d = data_msg(0x06, 0x0200_0000);
    let t = at(&d.fields(), &["ADO", "Type of Alert"]).clone();
    assert!(t.meaning.contains("Battery Status Change"), "{}", t.meaning);
}

#[test]
fn bist_bdo() {
    let d = data_msg(0x03, 0x5000_0000);
    let m = at(&d.fields(), &["BDO", "BIST Mode"]).clone();
    assert_eq!((m.bits.hi, m.bits.lo, m.raw), (31, 28, 5));
    assert!(m.meaning.contains("Carrier"), "{}", m.meaning);
    let d = data_msg(0x03, 0x1000_0000);
    assert_eq!(
        at(&d.fields(), &["BDO", "BIST Mode"]).flag,
        Some(Flag::Reserved)
    );
}

#[test]
fn get_country_info_ccdo() {
    let d = data_msg(0x07, 0x5553_0000);
    let c = at(&d.fields(), &["CCDO", "Country Code"]).clone();
    assert_eq!((c.bits.hi, c.bits.lo, c.raw), (31, 16, 0x5553));
    assert_eq!(c.meaning, "US");
}

#[test]
fn enter_usb_eudo() {
    let d = data_msg(0x08, 0x2000_0000);
    let m = at(&d.fields(), &["EUDO", "USB Mode"]).clone();
    assert_eq!((m.bits.hi, m.bits.lo, m.raw), (30, 28, 2));
    assert!(m.meaning.contains("USB4"), "{}", m.meaning);
}

#[test]
fn epr_mode_eprmdo() {
    for (dobj, action) in [
        (0x0100_0000, EprAction::Enter),
        (0x0200_0000, EprAction::EnterAcknowledged),
        (0x0300_0000, EprAction::EnterSucceeded),
        (0x0400_0000, EprAction::EnterFailed),
        (0x0500_0000, EprAction::Exit),
    ] {
        let d = data_msg(0x0A, dobj);
        assert_eq!(d.message, Message::EprMode(EprMode { action, data: 0 }));
    }
    let d = data_msg(0x0A, 0x0401_0000);
    let data = at(&d.fields(), &["EPRMDO", "Data"]).clone();
    assert_eq!((data.bits.hi, data.bits.lo, data.raw), (23, 16, 1));
    assert!(data.meaning.contains("Cable"), "{}", data.meaning);
    let d = data_msg(0x0A, 0x0000_0000);
    assert_eq!(
        d.message,
        Message::EprMode(EprMode {
            action: EprAction::Reserved(0),
            data: 0
        })
    );
    assert_eq!(
        at(&d.fields(), &["EPRMDO", "Action"]).flag,
        Some(Flag::Reserved)
    );
}

#[test]
fn source_info_sido() {
    let d = data_msg(0x0B, 0x8064_6464);
    let fields = d.fields();
    let t = at(&fields, &["SIDO", "Port Type"]);
    assert_eq!((t.bits.hi, t.bits.lo, t.raw), (31, 31, 1));
    assert!(t.meaning.contains("Guaranteed"), "{}", t.meaning);
    let p = at(&fields, &["SIDO", "Port Maximum PDP"]);
    assert_eq!((p.bits.hi, p.bits.lo, p.raw), (23, 16, 100));
    assert_eq!(p.meaning, "100 W");
    assert!(find(&fields, "Port Present PDP").is_some());
    assert!(find(&fields, "Port Reported PDP").is_some());
}

#[test]
fn revision_rmdo() {
    let d = data_msg(0x0C, 0x3211_0000);
    let fields = d.fields();
    for (name, hi, lo, raw) in [
        ("Revision Major", 31, 28, 3),
        ("Revision Minor", 27, 24, 2),
        ("Version Major", 23, 20, 1),
        ("Version Minor", 19, 16, 1),
    ] {
        let f = at(&fields, &["RMDO", name]);
        assert_eq!((f.bits.hi, f.bits.lo, f.raw), (hi, lo, raw), "{name}");
    }
    assert!(d.summary().contains("3.2"), "{}", d.summary());
}

#[test]
fn epr_mode_reserved_bits_are_flagged() {
    let d = data_msg(0x0A, 0x0100_0001);
    let eprmdo = at(&d.fields(), &["EPRMDO"]).clone();
    assert!(
        eprmdo
            .children
            .iter()
            .any(|f| f.bits.lo == 0 && f.flag == Some(Flag::Reserved)),
        "{eprmdo:?}"
    );
}

#[test]
fn extended_header_reserved_bit_9_is_shown_and_flagged() {
    let data = [45, 0x02, 0, 0x02, 0, 0, 0];
    let (h, body) = extended(0x02, ext_header(true, 0, false, 7) | 0x0200, &data, true);
    let d = decode_one(SOP, h, &body);
    let eh = at(&d.fields(), &["Extended Header"]).clone();
    let bit9 = eh
        .children
        .iter()
        .find(|f| (f.bits.hi, f.bits.lo) == (9, 9))
        .unwrap_or_else(|| panic!("bit 9 not shown: {eh:?}"));
    assert_eq!(bit9.flag, Some(Flag::Reserved));
}

#[test]
fn country_code_bytes_that_are_not_letters_stay_printable() {
    let d = data_msg(0x07, 0x0A1B_0000);
    let c = at(&d.fields(), &["CCDO", "Country Code"]).clone();
    assert!(!c.meaning.chars().any(char::is_control), "{:?}", c.meaning);
}
