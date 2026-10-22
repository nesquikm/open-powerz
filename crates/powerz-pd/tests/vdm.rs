//! AC-TGE83B.4: structured VDMs decode the VDM header, Discover Identity
//! (ID header, cert stat, product, passive and active cable, UFP and DFP
//! VDOs), Discover SVIDs, Discover Modes, Enter Mode, Exit Mode and
//! Attention. Unstructured VDMs show their header and raw data.

mod common;

use common::*;
use powerz_pd::{CommandType, Message, ProductTypeVdo, Vdm, VdmBody, VdmCommand, VdmHeader};

const VENDOR_DEFINED: u16 = 0x0F;

fn vdm(sop: u8, hdr: u16, dos: &[u32]) -> (Vdm, powerz_pd::Decoded) {
    let d = decode_one(sop, hdr, &objects(dos));
    match &d.message {
        Message::Vdm(v) => (v.clone(), d),
        other => panic!("expected a VDM, got {other:?}"),
    }
}

/// Session 2: the cable's Discover Identity ACK on SOP'.
const CABLE_ACK: [u32; 5] = [
    0xFF00_A841,
    0x1860_050D,
    0x0000_404D,
    0x8254_0000,
    0x000A_2640,
];

#[test]
fn discover_identity_request_header() {
    let (v, d) = vdm(SOP_PRIME, 0x108F, &[0xFF00_A001]);
    match v.header {
        VdmHeader::Structured(h) => {
            assert_eq!(h.raw, 0xFF00_A001);
            assert_eq!(h.svid, 0xFF00);
            assert_eq!((h.major, h.minor), (1, 0), "structured VDM version 2.0");
            assert_eq!(h.object_position, 0);
            assert_eq!(h.command_type, CommandType::Req);
            assert_eq!(h.command, VdmCommand::DiscoverIdentity);
        }
        other => panic!("expected a structured header, got {other:?}"),
    }
    let fields = d.fields();
    let svid = at(&fields, &["VDM Header", "SVID"]);
    assert_eq!((svid.bits.hi, svid.bits.lo, svid.raw), (31, 16, 0xFF00));
    let ty = at(&fields, &["VDM Header", "VDM Type"]);
    assert_eq!((ty.bits.hi, ty.bits.lo, ty.raw), (15, 15, 1));
    let cmd = at(&fields, &["VDM Header", "Command"]);
    assert_eq!((cmd.bits.hi, cmd.bits.lo, cmd.raw), (4, 0, 1));
    assert!(cmd.meaning.contains("Discover Identity"), "{}", cmd.meaning);
    let s = d.summary();
    assert!(s.contains("Discover Identity") && s.contains("REQ"), "{s}");
}

#[test]
fn passive_cable_discover_identity_ack() {
    let (v, d) = vdm(SOP_PRIME, 0x518F, &CABLE_ACK);
    match &v.header {
        VdmHeader::Structured(h) => {
            assert_eq!(h.command_type, CommandType::Ack);
            assert_eq!(h.command, VdmCommand::DiscoverIdentity);
            assert_eq!((h.major, h.minor), (1, 1), "structured VDM version 2.1");
        }
        other => panic!("expected a structured header, got {other:?}"),
    }
    let id = match &v.body {
        VdmBody::DiscoverIdentity(id) => id,
        other => panic!("expected Discover Identity, got {other:?}"),
    };
    assert_eq!(id.id_header.raw, 0x1860_050D);
    assert!(!id.id_header.usb_host);
    assert!(!id.id_header.usb_device);
    assert_eq!(id.id_header.product_type_ufp, 0b011, "passive cable");
    assert!(!id.id_header.modal);
    assert_eq!(id.id_header.product_type_dfp, 0);
    assert_eq!(id.id_header.connector_type, 0b11, "USB Type-C plug");
    assert_eq!(id.id_header.vid, 0x050D);
    assert_eq!(id.cert_stat, Some(0x0000_404D));
    let product = id.product.expect("product VDO");
    assert_eq!((product.pid, product.bcd_device), (0x8254, 0x0000));
    assert_eq!(id.product_type_vdos.len(), 1);
    match &id.product_type_vdos[0] {
        ProductTypeVdo::PassiveCable(c) => {
            assert_eq!(c.raw, 0x000A_2640);
            assert_eq!(c.plug_type, 0b10, "USB Type-C");
            assert!(c.epr_capable);
            assert_eq!(c.latency, 0b0001);
            assert_eq!(c.termination, 0b00);
            assert_eq!(c.max_vbus_voltage, 0b11, "50 V");
            assert_eq!(c.vbus_current, 0b10, "5 A");
            assert_eq!(c.usb_speed, 0b000, "USB 2.0 only");
        }
        other => panic!("expected a passive cable VDO, got {other:?}"),
    }

    let fields = d.fields();
    let vid = at(&fields, &["ID Header", "USB Vendor ID"]);
    assert_eq!((vid.bits.hi, vid.bits.lo, vid.raw), (15, 0, 0x050D));
    assert!(
        at(&fields, &["ID Header", "Connector Type"])
            .meaning
            .contains("Plug")
    );
    assert_eq!(at(&fields, &["Cert Stat VDO", "XID"]).raw, 0x404D);
    assert_eq!(at(&fields, &["Product VDO", "USB Product ID"]).raw, 0x8254);
    let volts = at(&fields, &["Passive Cable VDO", "Maximum VBUS Voltage"]);
    assert_eq!((volts.bits.hi, volts.bits.lo, volts.raw), (10, 9, 3));
    assert!(volts.meaning.contains("50 V"), "{}", volts.meaning);
    let amps = at(&fields, &["Passive Cable VDO", "VBUS Current Handling"]);
    assert_eq!((amps.bits.hi, amps.bits.lo, amps.raw), (6, 5, 2));
    assert!(amps.meaning.contains("5 A"), "{}", amps.meaning);
    assert_eq!(at(&fields, &["Passive Cable VDO", "EPR Capable"]).raw, 1);
}

#[test]
fn active_cable_discover_identity_ack() {
    // ID header: active cable (0b100), Type-C plug, VID 0x1234.
    // VDO1: VDO 1.3, Type-C, latency 1, both ends active, 3 A, VBUS through,
    // SOP'' controller present, USB4 Gen3. VDO2: 70 C max op, 85 C shutdown.
    let dos = [
        0xFF00_A841,
        0x2060_1234,
        0,
        0x5678_0001,
        0x0068_383B,
        0x4655_0000,
    ];
    let (v, d) = vdm(
        SOP_PRIME,
        header(false, 6, 0, 1, REV3, 0, VENDOR_DEFINED),
        &dos,
    );
    let VdmBody::DiscoverIdentity(id) = &v.body else {
        panic!("expected Discover Identity, got {:?}", v.body);
    };
    assert_eq!(id.id_header.product_type_ufp, 0b100);
    assert_eq!(id.product_type_vdos.len(), 2);
    match &id.product_type_vdos[0] {
        ProductTypeVdo::ActiveCable(c) => {
            assert_eq!(c.vdo_version, 0b011);
            assert_eq!(c.termination, 0b11);
            assert_eq!(c.vbus_current, 0b01);
            assert!(c.vbus_through_cable);
            assert!(c.sop_double_prime_present);
            assert_eq!(c.usb_speed, 0b011);
        }
        other => panic!("expected active cable VDO1, got {other:?}"),
    }
    assert!(
        matches!(&id.product_type_vdos[1], ProductTypeVdo::ActiveCable2(c) if c.raw == 0x4655_0000)
    );
    let fields = d.fields();
    assert_eq!(
        at(
            &fields,
            &["Active Cable VDO 2", "Maximum Operating Temperature"]
        )
        .raw,
        70
    );
    assert_eq!(
        at(&fields, &["Active Cable VDO 2", "Shutdown Temperature"]).raw,
        85
    );
}

#[test]
fn ufp_and_dfp_discover_identity_ack() {
    // Dual-role port: USB host + device, UFP PDUSB Peripheral, modal, DFP
    // PDUSB Host, Type-C receptacle, VID 0x18D1. UFP VDO, pad, DFP VDO.
    let dos = [
        0xFF00_A041,
        0xD540_18D1,
        0x0000_0123,
        0x4EE1_0100,
        0x6100_000B,
        0,
        0x4400_0001,
    ];
    let (v, d) = vdm(SOP, data_from_sink(7, VENDOR_DEFINED), &dos);
    let VdmBody::DiscoverIdentity(id) = &v.body else {
        panic!("expected Discover Identity, got {:?}", v.body);
    };
    assert!(id.id_header.usb_host && id.id_header.usb_device && id.id_header.modal);
    assert_eq!(id.id_header.product_type_ufp, 0b010);
    assert_eq!(id.id_header.product_type_dfp, 0b010);
    assert_eq!(id.id_header.connector_type, 0b10);
    assert_eq!(id.cert_stat, Some(0x123));
    assert_eq!(
        id.product.map(|p| (p.pid, p.bcd_device)),
        Some((0x4EE1, 0x0100))
    );
    let ufp = id
        .product_type_vdos
        .iter()
        .find_map(|p| match p {
            ProductTypeVdo::Ufp(u) => Some(u),
            _ => None,
        })
        .expect("UFP VDO");
    assert_eq!(ufp.vdo_version, 0b011);
    assert_eq!(ufp.device_capability, 0b0001);
    assert_eq!(ufp.alternate_modes, 0b001);
    assert_eq!(ufp.usb_speed, 0b011);
    let dfp = id
        .product_type_vdos
        .iter()
        .find_map(|p| match p {
            ProductTypeVdo::Dfp(d) => Some(d),
            _ => None,
        })
        .expect("DFP VDO");
    assert_eq!(dfp.vdo_version, 0b010);
    assert_eq!(dfp.host_capability, 0b100);
    assert_eq!(dfp.port_number, 1);
    let fields = d.fields();
    assert_eq!(at(&fields, &["DFP VDO", "Port Number"]).raw, 1);
    assert_eq!(at(&fields, &["UFP VDO", "USB Highest Speed"]).raw, 3);
}

#[test]
fn discover_svids_ack() {
    let (v, _) = vdm(
        SOP,
        data_from_sink(3, VENDOR_DEFINED),
        &[0xFF00_A042, 0xFF01_8087, 0],
    );
    let VdmHeader::Structured(h) = v.header else {
        panic!("structured")
    };
    assert_eq!(h.command, VdmCommand::DiscoverSvids);
    assert_eq!(v.body, VdmBody::DiscoverSvids(vec![0xFF01, 0x8087]));
}

#[test]
fn discover_modes_ack() {
    let (v, _) = vdm(
        SOP,
        data_from_sink(2, VENDOR_DEFINED),
        &[0xFF01_A043, 0x000C_0045],
    );
    let VdmHeader::Structured(h) = v.header else {
        panic!("structured")
    };
    assert_eq!(h.svid, 0xFF01);
    assert_eq!(h.command, VdmCommand::DiscoverModes);
    assert_eq!(v.body, VdmBody::DiscoverModes(vec![0x000C_0045]));
}

#[test]
fn enter_exit_mode_and_attention() {
    let (v, d) = vdm(SOP, data_from_source(1, VENDOR_DEFINED), &[0xFF01_A104]);
    let VdmHeader::Structured(h) = v.header else {
        panic!("structured")
    };
    assert_eq!(h.command, VdmCommand::EnterMode);
    assert_eq!(h.object_position, 1);
    assert_eq!(h.command_type, CommandType::Req);
    assert!(d.summary().contains("Enter Mode"));

    let (v, _) = vdm(SOP, data_from_source(1, VENDOR_DEFINED), &[0xFF01_A105]);
    let VdmHeader::Structured(h) = v.header else {
        panic!("structured")
    };
    assert_eq!(h.command, VdmCommand::ExitMode);

    let (v, d) = vdm(
        SOP,
        data_from_sink(2, VENDOR_DEFINED),
        &[0xFF01_A106, 0x0000_009A],
    );
    let VdmHeader::Structured(h) = v.header else {
        panic!("structured")
    };
    assert_eq!(h.command, VdmCommand::Attention);
    assert_eq!(v.body, VdmBody::Objects(vec![0x0000_009A]));
    assert!(d.summary().contains("Attention"));

    // NAK / BUSY and SVID-specific commands.
    let (v, _) = vdm(SOP, data_from_sink(1, VENDOR_DEFINED), &[0xFF01_A090]);
    let VdmHeader::Structured(h) = v.header else {
        panic!("structured")
    };
    assert_eq!(h.command_type, CommandType::Nak);
    assert_eq!(h.command, VdmCommand::SvidSpecific(16));
    let (v, _) = vdm(SOP, data_from_sink(1, VENDOR_DEFINED), &[0xFF01_A0C4]);
    let VdmHeader::Structured(h) = v.header else {
        panic!("structured")
    };
    assert_eq!(h.command_type, CommandType::Busy);
}

#[test]
fn unstructured_vdm_shows_header_and_raw_data() {
    let (v, d) = vdm(
        SOP,
        data_from_source(3, VENDOR_DEFINED),
        &[0x05AC_1234, 0xDEAD_BEEF, 0x0102_0304],
    );
    match v.header {
        VdmHeader::Unstructured(u) => {
            assert_eq!(u.raw, 0x05AC_1234);
            assert_eq!(u.svid, 0x05AC);
            assert_eq!(u.vendor_use, 0x1234);
        }
        other => panic!("expected an unstructured header, got {other:?}"),
    }
    assert_eq!(v.body, VdmBody::Objects(vec![0xDEAD_BEEF, 0x0102_0304]));
    let fields = d.fields();
    assert_eq!(at(&fields, &["VDM Header", "VDM Type"]).raw, 0);
    assert_eq!(at(&fields, &["VDO 1"]).raw, 0xDEAD_BEEF);
    assert_eq!(at(&fields, &["VDO 2"]).raw, 0x0102_0304);
    assert!(
        d.summary().to_uppercase().contains("05AC"),
        "{}",
        d.summary()
    );
}

#[test]
fn an_unknown_sop_code_is_not_read_as_a_cable() {
    // Same passive-cable ACK, but on SOP code 5, which names no SOP*: the ID
    // header is read the way the message header reads that code (not a cable).
    let (v, _) = vdm(5, 0x518F, &CABLE_ACK);
    let id = match &v.body {
        VdmBody::DiscoverIdentity(id) => id,
        other => panic!("expected Discover Identity, got {other:?}"),
    };
    assert!(
        !id.product_type_vdos
            .iter()
            .any(|p| matches!(p, ProductTypeVdo::PassiveCable(_))),
        "{:?}",
        id.product_type_vdos
    );
}
