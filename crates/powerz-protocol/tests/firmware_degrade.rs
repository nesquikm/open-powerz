//! The shared firmware-status degrade helper (FR V9SK52, AC .2 / .3):
//! `Session::read_firmware_status_or_unknown`, `FirmwareStatus::unknown`
//! and `session::Error::is_link_fatal`, over a scripted mock transport.
//! Every byte vector is synthetic.

mod common;

use common::*;
use powerz_protocol::decode::DecodeError;
use powerz_protocol::decode::head;
use powerz_protocol::frame;
use powerz_protocol::session::{self, FirmwareStatus, Session};
use powerz_protocol::transport;
use powerz_protocol::transport::mock::{MockTransport, Step};

const BOOT_ADDR: u32 = 0x0000_0420;

fn session(script: Vec<Step>) -> Session<MockTransport, FakeClock> {
    Session::with_clock(
        MockTransport::new(script),
        FakeClock::new(1_790_000_000_000),
    )
}

fn connect_step(id: u8) -> Step {
    Step::transact(frame::connect(id).to_vec(), accept(id))
}

/// A session whose CONNECT (id 1) was accepted, with `rest` scripted after.
fn connected(rest: Vec<Step>) -> Session<MockTransport, FakeClock> {
    let mut s = session([vec![connect_step(1)], rest].concat());
    s.connect().expect("scripted CONNECT");
    s
}

fn types(s: &Session<MockTransport, FakeClock>) -> Vec<u8> {
    s.transport()
        .written()
        .iter()
        .map(|f| f[0] & 0x7F)
        .collect()
}

// ---------------------------------------------------------------- FirmwareStatus::unknown

#[test]
fn unknown_firmware_status_has_no_blocks_no_update_flag_and_no_warning() {
    let st = FirmwareStatus::unknown();
    assert_eq!(st.bootloader, None);
    assert_eq!(st.application, None);
    assert!(!st.update_required);
    assert_eq!(st.warnings(), Vec::new());
    assert_eq!(
        st,
        FirmwareStatus {
            bootloader: None,
            application: None,
            update_required: false,
        }
    );
}

// ---------------------------------------------------------------- Error::is_link_fatal

#[test]
fn unplug_and_permission_errors_are_link_fatal() {
    assert!(session::Error::Transport(transport::Error::Disconnected).is_link_fatal());
    assert!(
        session::Error::Transport(transport::Error::Permission("denied".into())).is_link_fatal()
    );
}

#[test]
fn timeouts_and_bad_replies_are_not_link_fatal() {
    for e in [
        session::Error::Transport(transport::Error::Timeout),
        session::Error::Protocol("late reply".into()),
        session::Error::Rejected { request_type: 0x40 },
        session::Error::Decode(DecodeError::Length {
            what: "HEAD reply",
            expected: 8,
            got: 4,
        }),
    ] {
        assert!(!e.is_link_fatal(), "{e:?}");
    }
}

// ---------------------------------------------------------------- degrade + reconnect

#[test]
fn an_undecodable_head_degrades_to_unknown_and_reconnects_once() {
    // HEAD (id 2) answered with a bare header: too short to decode.
    let mut s = connected(vec![
        Step::transact(head::request(2).to_vec(), vec![0x40, 2, 0x00, 0x01]),
        connect_step(3),
        Step::transact(get_data(4, 0x001), with_id(vector(ADC_52), 4)),
    ]);
    let st = s
        .read_firmware_status_or_unknown()
        .expect("an undecodable HEAD only costs the firmware status");
    assert_eq!(st, FirmwareStatus::unknown());
    assert_eq!(
        types(&s),
        vec![0x02, 0x40, 0x02],
        "one recovery CONNECT right after the failed HEAD"
    );
    // The pipe is usable again.
    s.read_adc().expect("a request after the recovery works");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn a_head_timeout_degrades_to_unknown_and_reconnects_once() {
    let mut s = connected(vec![
        Step::transact_err(head::request(2).to_vec(), transport::Error::Timeout),
        connect_step(3),
        Step::transact(get_data(4, 0x001), with_id(vector(ADC_52), 4)),
    ]);
    let st = s
        .read_firmware_status_or_unknown()
        .expect("a HEAD timeout only costs the firmware status");
    assert_eq!(st, FirmwareStatus::unknown());
    assert_eq!(types(&s), vec![0x02, 0x40, 0x02]);
    s.read_adc().expect("a request after the recovery works");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn link_fatal_errors_on_head_are_returned_and_send_no_connect() {
    for err in [
        transport::Error::Disconnected,
        transport::Error::Permission("denied".into()),
    ] {
        let mut s = connected(vec![Step::transact_err(
            head::request(2).to_vec(),
            err.clone(),
        )]);
        let r = s.read_firmware_status_or_unknown();
        assert_eq!(r, Err(session::Error::Transport(err.clone())), "{err:?}");
        assert_eq!(
            types(&s),
            vec![0x02, 0x40],
            "{err:?}: no CONNECT after a link-fatal error"
        );
    }
}

#[test]
fn link_fatal_errors_on_an_info_block_are_returned_and_send_no_connect() {
    for err in [
        transport::Error::Disconnected,
        transport::Error::Permission("denied".into()),
    ] {
        let mut s = connected(vec![
            Step::transact(
                head::request(2).to_vec(),
                vec![0x40, 2, 0x00, 0x01, 0, 0, 0, 0],
            ),
            Step::transact_err(memread_request(3, BOOT_ADDR, 64), err.clone()),
        ]);
        let r = s.read_firmware_status_or_unknown();
        assert_eq!(r, Err(session::Error::Transport(err.clone())), "{err:?}");
        assert_eq!(types(&s), vec![0x02, 0x40, 0x44], "{err:?}: no CONNECT");
    }
}

#[test]
fn an_info_block_timeout_degrades_to_unknown_and_reconnects_once() {
    let mut s = connected(vec![
        Step::transact(
            head::request(2).to_vec(),
            vec![0x40, 2, 0x00, 0x01, 0, 0, 0, 0],
        ),
        Step::transact_err(memread_request(3, BOOT_ADDR, 64), transport::Error::Timeout),
        connect_step(4),
        Step::transact(get_data(5, 0x001), with_id(vector(ADC_52), 5)),
    ]);
    let st = s
        .read_firmware_status_or_unknown()
        .expect("an info-block timeout only costs the firmware status");
    assert_eq!(st, FirmwareStatus::unknown());
    assert_eq!(types(&s), vec![0x02, 0x40, 0x44, 0x02]);
    s.read_adc().expect("a request after the recovery works");
    assert_eq!(s.transport().remaining(), 0);
}
