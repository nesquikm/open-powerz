//! AC-TGE83B.2: source and sink power data objects decode for fixed,
//! battery, variable, PPS, SPR AVS and EPR AVS, with the field widths and
//! units of USB PD r3.2 (50 mV / 10 mA / 250 mW for fixed, battery and
//! variable; 100 mV / 50 mA for PPS; 10 mA for SPR AVS; 100 mV / 1 W for
//! EPR AVS).

mod common;

use common::*;
use powerz_pd::{
    Battery, EprAvs, FixedSink, FixedSource, Flag, Message, Pps, SinkPdo, SourcePdo, SprAvs,
    Variable,
};

#[test]
fn fixed_source_pdo() {
    // 5 V 3 A, Unconstrained Power, Unchunked Extended Messages, EPR Capable
    // (session-2 charger PDO1).
    assert_eq!(
        SourcePdo::decode(0x0981_912C),
        SourcePdo::Fixed(FixedSource {
            raw: 0x0981_912C,
            dual_role_power: false,
            usb_suspend_supported: false,
            unconstrained_power: true,
            usb_communications_capable: false,
            dual_role_data: false,
            unchunked_extended_messages: true,
            epr_capable: true,
            peak_current: 0,
            voltage_mv: 5_000,
            max_current_ma: 3_000,
        })
    );
    // 20 V 4.7 A.
    match SourcePdo::decode(0x0006_41D6) {
        SourcePdo::Fixed(f) => {
            assert_eq!(f.voltage_mv, 20_000);
            assert_eq!(f.max_current_ma, 4_700);
            assert!(!f.epr_capable);
        }
        other => panic!("expected a fixed PDO, got {other:?}"),
    }
    // Every flag bit 29..23 and peak current 21..20.
    match SourcePdo::decode(0x3FB1_912C) {
        SourcePdo::Fixed(f) => {
            assert!(f.dual_role_power);
            assert!(f.usb_suspend_supported);
            assert!(f.unconstrained_power);
            assert!(f.usb_communications_capable);
            assert!(f.dual_role_data);
            assert!(f.unchunked_extended_messages);
            assert!(f.epr_capable);
            assert_eq!(f.peak_current, 0b11);
        }
        other => panic!("expected a fixed PDO, got {other:?}"),
    }
}

#[test]
fn battery_and_variable_source_pdos() {
    // Battery: 5..21 V, 100 W (400 x 250 mW).
    assert_eq!(
        SourcePdo::decode(0x5A41_9190),
        SourcePdo::Battery(Battery {
            raw: 0x5A41_9190,
            max_voltage_mv: 21_000,
            min_voltage_mv: 5_000,
            power_mw: 100_000,
        })
    );
    // Variable: 5..21 V, 3 A.
    assert_eq!(
        SourcePdo::decode(0x9A41_912C),
        SourcePdo::Variable(Variable {
            raw: 0x9A41_912C,
            max_voltage_mv: 21_000,
            min_voltage_mv: 5_000,
            current_ma: 3_000,
        })
    );
}

#[test]
fn spr_pps_apdo() {
    // 3.3..21 V, 3 A (60 x 50 mA), PPS Power Limited.
    assert_eq!(
        SourcePdo::decode(0xC9A4_213C),
        SourcePdo::Pps(Pps {
            raw: 0xC9A4_213C,
            power_limited: true,
            max_voltage_mv: 21_000,
            min_voltage_mv: 3_300,
            max_current_ma: 3_000,
        })
    );
    // Bit 25 is reserved in a PPS APDO: max voltage is bits 24..17 only.
    match SourcePdo::decode(0xC3A4_213C) {
        SourcePdo::Pps(p) => assert_eq!(p.max_voltage_mv, 21_000),
        other => panic!("expected PPS, got {other:?}"),
    }
}

#[test]
fn spr_avs_apdo() {
    // 9..15 V at 3 A, 15..20 V at 2.25 A.
    assert_eq!(
        SourcePdo::decode(0xE004_B0E1),
        SourcePdo::SprAvs(SprAvs {
            raw: 0xE004_B0E1,
            peak_current: 0,
            max_current_15v_ma: 3_000,
            max_current_20v_ma: 2_250,
        })
    );
}

#[test]
fn epr_avs_apdo() {
    // Session-2 EPR PDO9: 15..28 V, 140 W.
    assert_eq!(
        SourcePdo::decode(0xD230_968C),
        SourcePdo::EprAvs(EprAvs {
            raw: 0xD230_968C,
            peak_current: 0,
            max_voltage_mv: 28_000,
            min_voltage_mv: 15_000,
            pdp_w: 140,
        })
    );
}

#[test]
fn reserved_apdo_subtype() {
    assert_eq!(
        SourcePdo::decode(0xF000_0000),
        SourcePdo::Reserved(0xF000_0000)
    );
    assert_eq!(SinkPdo::decode(0xF000_0000), SinkPdo::Reserved(0xF000_0000));
}

#[test]
fn sink_pdos() {
    // Fixed sink: 5 V 3 A operational, Dual-Role Power, Higher Capability,
    // Fast Role Swap required current 0b10 (1.5 A @ 5 V).
    assert_eq!(
        SinkPdo::decode(0x3101_912C),
        SinkPdo::Fixed(FixedSink {
            raw: 0x3101_912C,
            dual_role_power: true,
            higher_capability: true,
            unconstrained_power: false,
            usb_communications_capable: false,
            dual_role_data: false,
            fast_role_swap: 0b10,
            voltage_mv: 5_000,
            operational_current_ma: 3_000,
        })
    );
    assert!(matches!(
        SinkPdo::decode(0x5A41_9190),
        SinkPdo::Battery(Battery {
            power_mw: 100_000,
            ..
        })
    ));
    assert!(matches!(
        SinkPdo::decode(0x9A41_912C),
        SinkPdo::Variable(Variable {
            current_ma: 3_000,
            ..
        })
    ));
    match SinkPdo::decode(0xC1A4_213C) {
        SinkPdo::Pps(p) => {
            assert_eq!(
                (p.min_voltage_mv, p.max_voltage_mv, p.max_current_ma),
                (3_300, 21_000, 3_000)
            );
        }
        other => panic!("expected sink PPS, got {other:?}"),
    }
    assert!(matches!(
        SinkPdo::decode(0xE004_B0E1),
        SinkPdo::SprAvs(SprAvs {
            max_current_15v_ma: 3_000,
            max_current_20v_ma: 2_250,
            ..
        })
    ));
    assert!(matches!(
        SinkPdo::decode(0xD230_968C),
        SinkPdo::EprAvs(EprAvs {
            max_voltage_mv: 28_000,
            min_voltage_mv: 15_000,
            pdp_w: 140,
            ..
        })
    ));
}

#[test]
fn source_capabilities_message_lists_pdos() {
    let pdos = [
        0x0981_912C,
        0x0002_D12C,
        0x0004_B12C,
        0x0006_41D6,
        0xC9A4_213C,
        0xE004_B0E1,
    ];
    let d = decode_one(SOP, data_from_source(6, 1), &objects(&pdos));
    match &d.message {
        Message::SourceCapabilities(list) => {
            assert_eq!(list.len(), 6);
            assert_eq!(list[1], SourcePdo::decode(0x0002_D12C));
            assert!(matches!(list[4], SourcePdo::Pps(_)));
            assert!(matches!(list[5], SourcePdo::SprAvs(_)));
        }
        other => panic!("expected Source_Capabilities, got {other:?}"),
    }
}

#[test]
fn sink_capabilities_message_lists_pdos() {
    let d = decode_one(
        SOP,
        data_from_sink(2, 4),
        &objects(&[0x3101_912C, 0xC1A4_213C]),
    );
    match &d.message {
        Message::SinkCapabilities(list) => {
            assert_eq!(list.len(), 2);
            assert!(matches!(
                list[0],
                SinkPdo::Fixed(FixedSink {
                    operational_current_ma: 3_000,
                    ..
                })
            ));
            assert!(matches!(list[1], SinkPdo::Pps(_)));
        }
        other => panic!("expected Sink_Capabilities, got {other:?}"),
    }
}

#[test]
fn pdo_field_tree_has_widths_and_units() {
    let pdos = [0x0981_912C, 0xC9A4_213C, 0xD230_968C, 0xF000_0000];
    let d = decode_one(SOP, data_from_source(4, 1), &objects(&pdos));
    let fields = d.fields();

    let v = at(&fields, &["PDO 1", "Voltage"]);
    assert_eq!((v.bits.hi, v.bits.lo, v.raw), (19, 10, 100));
    assert_eq!(v.meaning, "5.00 V");
    let i = at(&fields, &["PDO 1", "Maximum Current"]);
    assert_eq!((i.bits.hi, i.bits.lo, i.raw), (9, 0, 300));
    assert_eq!(i.meaning, "3.00 A");

    let pmax = at(&fields, &["PDO 2", "Maximum Voltage"]);
    assert_eq!((pmax.bits.hi, pmax.bits.lo, pmax.raw), (24, 17, 210));
    assert_eq!(pmax.meaning, "21.00 V");
    let pmin = at(&fields, &["PDO 2", "Minimum Voltage"]);
    assert_eq!((pmin.bits.hi, pmin.bits.lo, pmin.raw), (15, 8, 33));
    assert_eq!(pmin.meaning, "3.30 V");
    let pi = at(&fields, &["PDO 2", "Maximum Current"]);
    assert_eq!((pi.bits.hi, pi.bits.lo, pi.raw), (6, 0, 60));
    assert_eq!(pi.meaning, "3.00 A");

    let emax = at(&fields, &["PDO 3", "Maximum Voltage"]);
    assert_eq!((emax.bits.hi, emax.bits.lo, emax.raw), (25, 17, 280));
    assert_eq!(emax.meaning, "28.00 V");
    let pdp = at(&fields, &["PDO 3", "PDP"]);
    assert_eq!((pdp.bits.hi, pdp.bits.lo, pdp.raw), (7, 0, 140));
    assert_eq!(pdp.meaning, "140 W");

    // The reserved APDO subtype is flagged.
    let pdo4 = at(&fields, &["PDO 4"]);
    assert!(
        all_fields(std::slice::from_ref(pdo4))
            .iter()
            .any(|f| f.flag == Some(Flag::Reserved)),
        "reserved APDO flagged: {pdo4:#?}"
    );
}

#[test]
fn objects_beyond_the_header_count_are_not_decoded() {
    // The header announces one PDO; a second word after it is not a PDO.
    let d = decode_one(
        SOP,
        data_from_source(1, 0x01),
        &objects(&[0x0001_912C, 0x0002_D12C]),
    );
    match &d.message {
        Message::SourceCapabilities(pdos) => assert_eq!(pdos.len(), 1, "{pdos:?}"),
        other => panic!("expected Source_Capabilities, got {other:?}"),
    }
}
