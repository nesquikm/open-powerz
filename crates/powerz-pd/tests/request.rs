//! AC-TGE83B.3: a Request decodes against the last Source_Capabilities seen
//! on SOP in the same capture. Soft_Reset on SOP, a hard reset and a detach
//! clear that memory. EPR_Request decodes its RDO and its copy of the PDO.

mod common;

use common::*;
use powerz_pd::{
    AvsRdo, BatteryRdo, Capture, FixedRdo, Flag, Input, Issue, Message, PpsRdo, Rdo, Request,
    SourcePdo,
};

/// 5 V 3 A, 9 V 3 A, PPS 3.3..21 V 3 A, Battery 5..21 V 100 W, Variable 5..21 V 3 A.
const CAPS: [u32; 5] = [
    0x0981_912C,
    0x0002_D12C,
    0xC9A4_213C,
    0x5A41_9190,
    0x9A41_912C,
];

fn caps_capture() -> Capture {
    let mut cap = Capture::new();
    let d = feed(&mut cap, SOP, data_from_source(5, 1), &objects(&CAPS));
    assert!(matches!(d.message, Message::SourceCapabilities(_)));
    cap
}

fn request(cap: &mut Capture, rdo: u32) -> (Request, Vec<Issue>) {
    let d = feed(cap, SOP, data_from_sink(1, 2), &objects(&[rdo]));
    match d.message {
        Message::Request(r) => (r, d.issues),
        other => panic!("expected Request, got {other:?}"),
    }
}

#[test]
fn fixed_request_decodes_against_remembered_caps() {
    let mut cap = caps_capture();
    // Session-2 RDO 0x1344B12C: position 1, 3 A operating / 3 A max,
    // USB Communications Capable, No USB Suspend, EPR Capable.
    let (req, issues) = request(&mut cap, 0x1344_B12C);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(req.pdo, Some(SourcePdo::decode(CAPS[0])));
    match req.rdo {
        Rdo::Fixed(FixedRdo {
            position,
            give_back,
            capability_mismatch,
            usb_communications_capable,
            no_usb_suspend,
            unchunked_extended_messages,
            epr_capable,
            operating_current_ma,
            max_current_ma,
            ..
        }) => {
            assert_eq!(position, 1);
            assert!(!give_back);
            assert!(!capability_mismatch);
            assert!(usb_communications_capable);
            assert!(no_usb_suspend);
            assert!(!unchunked_extended_messages);
            assert!(epr_capable);
            assert_eq!(operating_current_ma, 3_000);
            assert_eq!(max_current_ma, 3_000);
        }
        other => panic!("expected a fixed RDO, got {other:?}"),
    }
}

#[test]
fn pps_battery_and_variable_requests() {
    let mut cap = caps_capture();
    // PPS at position 3: 9.00 V (450 x 20 mV), 2 A (40 x 50 mA).
    let (req, _) = request(&mut cap, 0x3003_8428);
    assert!(matches!(
        req.rdo,
        Rdo::Pps(PpsRdo {
            position: 3,
            voltage_mv: 9_000,
            current_ma: 2_000,
            ..
        })
    ));
    // Battery at position 4: 60 W operating (240 x 250 mW), 100 W max.
    let (req, _) = request(&mut cap, (4 << 28) | (240 << 10) | 400);
    assert!(matches!(
        req.rdo,
        Rdo::Battery(BatteryRdo {
            position: 4,
            operating_power_mw: 60_000,
            max_power_mw: 100_000,
            ..
        })
    ));
    // Variable at position 5: same layout as fixed.
    let (req, _) = request(&mut cap, (5 << 28) | (200 << 10) | 300);
    assert!(matches!(
        req.rdo,
        Rdo::Variable(FixedRdo {
            position: 5,
            operating_current_ma: 2_000,
            max_current_ma: 3_000,
            ..
        })
    ));
}

#[test]
fn request_summary_names_the_offer() {
    let mut cap = caps_capture();
    let d = feed(
        &mut cap,
        SOP,
        data_from_sink(1, 2),
        &objects(&[0x1344_B12C]),
    );
    let s = d.summary();
    assert!(s.starts_with("Request"), "{s}");
    assert!(s.contains("5.00 V"), "{s}");
    assert!(s.contains("3.00 A"), "{s}");
    let fields = d.fields();
    let pos = at(&fields, &["RDO", "Object Position"]);
    assert_eq!((pos.bits.hi, pos.bits.lo, pos.raw), (31, 28, 1));
    let op = at(&fields, &["RDO", "Operating Current"]);
    assert_eq!((op.bits.hi, op.bits.lo, op.raw), (19, 10, 300));
    assert_eq!(op.meaning, "3.00 A");
}

#[test]
fn request_without_caps_is_unresolved() {
    let mut cap = Capture::new();
    let (req, issues) = request(&mut cap, 0x1344_B12C);
    assert_eq!(req.pdo, None);
    assert!(matches!(
        req.rdo,
        Rdo::Unresolved {
            position: 1,
            raw: 0x1344_B12C
        }
    ));
    assert!(issues.contains(&Issue::NoSourceCapabilities), "{issues:?}");
}

#[test]
fn request_position_out_of_range_or_zero() {
    let mut cap = caps_capture();
    let (req, issues) = request(&mut cap, 0x7000_0000 | 0x0004_B12C);
    assert_eq!(req.pdo, None);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, Issue::PositionOutOfRange { position: 7, .. })),
        "{issues:?}"
    );

    let d = feed(
        &mut cap,
        SOP,
        data_from_sink(1, 2),
        &objects(&[0x0004_B12C]),
    );
    let fields = d.fields();
    assert_eq!(
        at(&fields, &["RDO", "Object Position"]).flag,
        Some(Flag::Invalid)
    );
}

#[test]
fn later_caps_replace_earlier_ones() {
    let mut cap = caps_capture();
    feed(
        &mut cap,
        SOP,
        data_from_source(1, 1),
        &objects(&[0x0002_D12C]),
    );
    let (req, _) = request(&mut cap, 0x1344_B12C);
    assert_eq!(req.pdo, Some(SourcePdo::decode(0x0002_D12C)));
}

#[test]
fn caps_on_cable_sop_are_not_remembered() {
    let mut cap = Capture::new();
    feed(&mut cap, SOP_PRIME, data_from_source(5, 1), &objects(&CAPS));
    let (_, issues) = request(&mut cap, 0x1344_B12C);
    assert!(issues.contains(&Issue::NoSourceCapabilities));
}

#[test]
fn soft_reset_on_sop_clears_caps_but_not_on_cable_sop() {
    let mut cap = caps_capture();
    feed(&mut cap, SOP_PRIME, control(0x0D), &[]);
    let (req, _) = request(&mut cap, 0x1344_B12C);
    assert!(req.pdo.is_some(), "Soft_Reset on SOP' keeps the caps");

    feed(&mut cap, SOP, control(0x0D), &[]);
    let (req, issues) = request(&mut cap, 0x1344_B12C);
    assert_eq!(req.pdo, None, "Soft_Reset on SOP clears the caps");
    assert!(issues.contains(&Issue::NoSourceCapabilities));
}

#[test]
fn hard_reset_and_detach_clear_caps() {
    let mut cap = caps_capture();
    assert!(
        cap.feed(Input::HardReset).is_none(),
        "events decode to nothing"
    );
    let (req, _) = request(&mut cap, 0x1344_B12C);
    assert_eq!(req.pdo, None, "hard reset clears the caps");

    let mut cap = caps_capture();
    assert!(cap.feed(Input::Detach).is_none());
    let (req, _) = request(&mut cap, 0x1344_B12C);
    assert_eq!(req.pdo, None, "detach clears the caps");

    // Attach and cable reset leave them alone.
    let mut cap = caps_capture();
    assert!(cap.feed(Input::Attach).is_none());
    assert!(cap.feed(Input::CableReset).is_none());
    let (req, _) = request(&mut cap, 0x1344_B12C);
    assert!(req.pdo.is_some());
}

#[test]
fn epr_request_decodes_rdo_and_pdo_copy() {
    // Session 2: EPR_Request at position 1 with the copy of PDO1 (5 V 3 A).
    let mut cap = Capture::new();
    let d = feed(
        &mut cap,
        SOP,
        data_from_sink(2, 9),
        &objects(&[0x1344_B12C, 0x0981_912C]),
    );
    assert!(
        d.issues.is_empty(),
        "the copy of the PDO needs no caps: {:?}",
        d.issues
    );
    match d.message {
        Message::EprRequest(r) => {
            assert_eq!(r.pdo, Some(SourcePdo::decode(0x0981_912C)));
            assert!(matches!(
                r.rdo,
                Rdo::Fixed(FixedRdo {
                    position: 1,
                    operating_current_ma: 3_000,
                    ..
                })
            ));
        }
        other => panic!("expected EPR_Request, got {other:?}"),
    }

    // Session 2: AVS RDO 0x9344B014 = 15000 mV 1000 mA against EPR AVS PDO9.
    let d = feed(
        &mut cap,
        SOP,
        data_from_sink(2, 9),
        &objects(&[0x9344_B014, 0xD230_968C]),
    );
    match d.message {
        Message::EprRequest(r) => {
            assert!(matches!(r.pdo, Some(SourcePdo::EprAvs(_))));
            assert!(matches!(
                r.rdo,
                Rdo::Avs(AvsRdo {
                    position: 9,
                    voltage_mv: 15_000,
                    current_ma: 1_000,
                    ..
                })
            ));
        }
        other => panic!("expected EPR_Request, got {other:?}"),
    }
    let fields = d.fields();
    assert!(find(&fields, "RDO").is_some());
    let copy = at(&fields, &["Copy of PDO", "Maximum Voltage"]);
    assert_eq!(copy.meaning, "28.00 V");
    let s = d.summary();
    assert!(s.starts_with("EPR_Request"), "{s}");
    assert!(s.contains("15.00 V") && s.contains("1.00 A"), "{s}");
}
