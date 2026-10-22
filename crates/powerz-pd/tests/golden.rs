//! AC-TGE83B.8 (spec half): golden summaries for an SPR negotiation, EPR
//! mode entry and EPR keep-alive built from the USB PD r3.2 layouts. The
//! session-2 device vectors are covered in
//! `crates/powerz-protocol/tests/pd_decode_golden.rs`.

mod common;

use common::*;
use powerz_pd::{Capture, EcdbType, EprAction, EprMode, ExtendedControl, Message};

#[test]
fn spr_negotiation_summaries() {
    let mut cap = Capture::new();
    let caps = feed(
        &mut cap,
        SOP,
        data_from_source(4, 1),
        &objects(&[0x0981_912C, 0x0002_D12C, 0x0004_B12C, 0x0006_41D6]),
    );
    assert_eq!(
        caps.summary(),
        "Source_Capabilities: Fixed 5.00 V 3.00 A, Fixed 9.00 V 3.00 A, \
         Fixed 15.00 V 3.00 A, Fixed 20.00 V 4.70 A"
    );
    assert_eq!(
        feed(&mut cap, SOP, header(false, 0, 0, 0, REV3, 0, 1), &[]).summary(),
        "GoodCRC"
    );
    let req = feed(
        &mut cap,
        SOP,
        data_from_sink(1, 2),
        &objects(&[0x1344_B12C]),
    );
    assert_eq!(req.summary(), "Request: #1 Fixed 5.00 V 3.00 A");
    assert_eq!(feed(&mut cap, SOP, control(3), &[]).summary(), "Accept");
    assert_eq!(feed(&mut cap, SOP, control(6), &[]).summary(), "PS_RDY");
}

#[test]
fn pps_request_summary() {
    let mut cap = Capture::new();
    feed(
        &mut cap,
        SOP,
        data_from_source(2, 1),
        &objects(&[0x0981_912C, 0xC9A4_213C]),
    );
    let req = feed(
        &mut cap,
        SOP,
        data_from_sink(1, 2),
        &objects(&[0x2003_8428]),
    );
    assert_eq!(req.summary(), "Request: #2 PPS 9.00 V 2.00 A");
}

#[test]
fn epr_mode_entry_and_keep_alive() {
    let mut cap = Capture::new();
    let steps = [
        (
            data_from_sink(1, 10),
            0x0100_0000u32,
            EprAction::Enter,
            "EPR_Mode: Enter",
        ),
        (
            data_from_source(1, 10),
            0x0200_0000,
            EprAction::EnterAcknowledged,
            "EPR_Mode: Enter_Acknowledged",
        ),
        (
            data_from_source(1, 10),
            0x0300_0000,
            EprAction::EnterSucceeded,
            "EPR_Mode: Enter_Succeeded",
        ),
    ];
    for (h, dobj, action, summary) in steps {
        let d = feed(&mut cap, SOP, h, &objects(&[dobj]));
        assert_eq!(d.message, Message::EprMode(EprMode { action, data: 0 }));
        assert_eq!(d.summary(), summary);
    }

    let (h, body) = extended_single(0x10, &[3, 0], false);
    let ka = feed(&mut cap, SOP, h, &body);
    assert_eq!(
        ka.message,
        Message::ExtendedControl(ExtendedControl {
            kind: EcdbType::EprKeepAlive,
            data: 0
        })
    );
    assert_eq!(ka.summary(), "Extended_Control: EPR_KeepAlive");

    let (h, body) = extended_single(0x10, &[4, 0], true);
    let ack = feed(&mut cap, SOP, h, &body);
    assert_eq!(ack.summary(), "Extended_Control: EPR_KeepAlive_Ack");
}
