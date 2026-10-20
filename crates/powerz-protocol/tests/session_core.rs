//! Session basics (FR N6EHQC): CONNECT, request/reply matching, typed
//! errors and the plain GET_DATA readers.

mod common;

use common::*;
use powerz_protocol::frame::Attribute;
use powerz_protocol::session::{Error, Session};
use powerz_protocol::transport::mock::{MockTransport, Step};

fn session(script: Vec<Step>) -> Session<MockTransport> {
    Session::open(MockTransport::new(script))
}

// ---- AC-N6EHQC.1: open without CONNECT, connect() needs ACCEPT ----

#[test]
fn session_open_sends_nothing_and_first_request_works_without_connect() {
    let mut s = session(vec![Step::transact(get_data(1, 0x001), vector(ADC_52))]);
    assert_eq!(s.transport().written().len(), 0);
    let adc = s.read_adc().unwrap();
    assert_eq!(adc.vbus_v(), 4196e-6);
    assert_eq!(s.transport().written(), &[get_data(1, 0x001)]);
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_open_writes_no_bytes() {
    let s = session(vec![]);
    assert_eq!(s.transport().written().len(), 0);
}

#[test]
fn session_connect_ok_on_accept() {
    let mut s = session(vec![Step::transact([0x02, 0x01, 0x00, 0x00], accept(1))]);
    s.connect().unwrap();
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_connect_reject_is_rejected_error() {
    let mut s = session(vec![Step::transact([0x02, 0x01, 0x00, 0x00], reject(1))]);
    assert_eq!(s.connect(), Err(Error::Rejected { request_type: 0x02 }));
}

#[test]
fn session_connect_other_reply_is_error() {
    let mut s = session(vec![Step::transact(
        [0x02, 0x01, 0x00, 0x00],
        vec![0x03, 0x01, 0x00, 0x00],
    )]);
    assert!(s.connect().is_err());
}

// ---- AC-N6EHQC.2: ids, matching, REJECT, DISCONNECT ----

#[test]
fn session_each_request_uses_next_counter_id() {
    let mut s = session(vec![
        Step::transact([0x02, 0x01, 0x00, 0x00], accept(1)),
        Step::transact(get_data(2, 0x001), with_id(vector(ADC_52), 2)),
        Step::transact(get_data(3, 0x001), with_id(vector(ADC_52), 3)),
        Step::transact(get_data(4, 0x008), with_id(vector(SETTINGS_188), 4)),
    ]);
    s.connect().unwrap();
    s.read_adc().unwrap();
    s.read_adc().unwrap();
    s.read_settings().unwrap();
    let ids: Vec<u8> = s.transport().written().iter().map(|f| f[1]).collect();
    assert_eq!(ids, vec![1, 2, 3, 4]);
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_reply_with_other_id_is_discarded_and_next_read() {
    let mut s = session(vec![
        Step::transact(get_data(1, 0x001), with_id(vector(ADC_52), 0x77)),
        Step::read(with_id(vector(ADC_52), 0x78)),
        Step::read(with_id(vector(ADC_52), 0x79)),
        Step::read(with_id(vector(ADC_52), 1)),
    ]);
    let adc = s.read_adc().unwrap();
    assert_eq!(adc.vbus_v(), 4196e-6);
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn session_more_than_three_mismatched_replies_is_protocol_error() {
    let mut s = session(vec![
        Step::transact(get_data(1, 0x001), with_id(vector(ADC_52), 0x77)),
        Step::read(with_id(vector(ADC_52), 0x78)),
        Step::read(with_id(vector(ADC_52), 0x79)),
        Step::read(with_id(vector(ADC_52), 0x7A)),
    ]);
    assert!(matches!(s.read_adc(), Err(Error::Protocol { .. })));
    assert_eq!(s.transport().remaining(), 0, "exactly three re-reads");
}

#[test]
fn session_reject_returns_rejected_with_request_type() {
    let mut s = session(vec![Step::transact(get_data(1, 0x001), reject(1))]);
    assert_eq!(
        s.get_data(Attribute::ADC),
        Err(Error::Rejected { request_type: 0x0C })
    );
}

#[test]
fn session_disconnect_reply_to_get_data_is_empty_list() {
    let mut s = session(vec![Step::transact(
        get_data(1, 0x002),
        vec![0x03, 0x01, 0x00, 0x00],
    )]);
    assert_eq!(s.get_data(Attribute::ADC_QUEUE).unwrap(), vec![]);
}

#[test]
fn session_wrong_reply_type_to_get_data_is_protocol_error() {
    let mut s = session(vec![Step::transact(get_data(1, 0x001), accept(1))]);
    assert!(matches!(
        s.get_data(Attribute::ADC),
        Err(Error::Protocol { .. })
    ));
}

#[test]
fn session_transport_error_is_propagated() {
    let mut s = session(vec![Step::transact_err(
        get_data(1, 0x001),
        powerz_protocol::transport::Error::Timeout,
    )]);
    assert_eq!(
        s.read_adc().unwrap_err(),
        Error::Transport(powerz_protocol::transport::Error::Timeout)
    );
}

// ---- AC-N6EHQC.3: decoded readers and auth_level() ----

#[test]
fn session_read_adc_decodes_snapshot() {
    let mut s = session(vec![Step::transact(get_data(1, 0x001), vector(ADC_52))]);
    let adc = s.read_adc().unwrap();
    assert_eq!(adc.vbus_v(), 4196e-6);
    assert_eq!(adc.ibus_a(), -95e-6);
}

#[test]
fn session_read_settings_decodes_block() {
    let mut s = session(vec![Step::transact(
        get_data(1, 0x008),
        vector(SETTINGS_188),
    )]);
    let st = s.read_settings().unwrap();
    assert!(st.block_a_crc_ok());
    assert!(st.block_b_crc_ok());
    assert_eq!(st.raw().len(), 180);
}

#[test]
fn session_get_data_returns_every_section() {
    let mut s = session(vec![Step::transact(
        get_data(1, 0x009),
        with_id(vector(ADC_SETTINGS_236), 1),
    )]);
    let sections = s.get_data(Attribute::ADC | Attribute::SETTINGS).unwrap();
    let shape: Vec<(u16, usize)> = sections
        .iter()
        .map(|x| (x.att.bits(), x.payload.len()))
        .collect();
    assert_eq!(shape, vec![(0x001, 44), (0x008, 180)]);
    let frame = vector(ADC_SETTINGS_236);
    assert_eq!(sections[0].payload, frame[8..52].to_vec());
}

#[test]
fn session_empty_put_data_is_empty_list() {
    let mut s = session(vec![Step::transact(
        get_data(1, 0x080),
        put_header(1, 0).to_vec(),
    )]);
    assert_eq!(s.get_data(Attribute(0x080)).unwrap(), vec![]);
}

#[test]
fn session_auth_level_follows_latest_put_data_header() {
    let adc = vector(ADC_52);
    let mut level1 = with_id(adc.clone(), 2);
    level1[..4].copy_from_slice(&put_header(2, 1));
    let mut level2 = with_id(adc.clone(), 3);
    level2[..4].copy_from_slice(&put_header(3, 2));
    let mut level0 = with_id(adc, 4);
    level0[..4].copy_from_slice(&put_header(4, 0));
    let mut s = session(vec![
        Step::transact(get_data(1, 0x001), vector(ADC_52)),
        Step::transact(get_data(2, 0x001), level1),
        Step::transact(get_data(3, 0x001), level2),
        Step::transact(get_data(4, 0x001), level0),
    ]);
    assert_eq!(s.auth_level(), 0);
    s.read_adc().unwrap();
    assert_eq!(s.auth_level(), 0);
    s.read_adc().unwrap();
    assert_eq!(s.auth_level(), 1);
    s.read_adc().unwrap();
    assert_eq!(s.auth_level(), 2);
    s.read_adc().unwrap();
    assert_eq!(s.auth_level(), 0);
}
