//! AC-G0W08Z.1: `SimDevice` implements `Transport`, echoes request ids,
//! answers 0x4C with id 0, carries the auth level in reply header bits
//! 17..18 and queues one stale DISCONNECT for the first read after open.

mod common;

use common::*;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::transport::Transport;

fn assert_is_transport<T: Transport>(t: T) -> T {
    t
}

#[test]
fn first_read_after_open_is_one_stale_disconnect() {
    let clock = clock();
    let mut d = assert_is_transport(device(Persona::Bulk, Scenario::Idle, &clock));
    let stale = d.transact(&frame::connect(1), T).unwrap();
    assert_eq!(stale.len(), 4, "{stale:02x?}");
    assert_eq!(stale[0], 0x03, "stale DISCONNECT type: {stale:02x?}");
    assert_ne!(stale[1], 0x01, "stale reply must not carry the request id");
    assert_eq!(&stale[2..], &[0x00, 0x00]);
    let accept = d.read(T).unwrap();
    assert_eq!(accept, vec![0x05, 0x01, 0x00, 0x00]);
    // Only one stale reply: the next request is answered directly.
    let reply = d.transact(&get_data(2, Attribute::ADC), T).unwrap();
    assert_eq!(ty(&reply), 0x41, "{reply:02x?}");
    assert_eq!(reply[1], 2);
}

#[test]
fn session_connect_recovers_from_the_stale_disconnect() {
    let clock = clock();
    let mut s = session(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    s.connect().expect("CONNECT after the stale DISCONNECT");
    let adc = s.read_adc().expect("ADC snapshot");
    assert!(adc.vbus_v().abs() < 0.05, "idle VBUS {}", adc.vbus_v());
}

#[test]
fn reply_ids_echo_the_request_id_including_zero_and_repeats() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    for id in [7u8, 0, 200, 200, 255] {
        let reply = d.transact(&get_data(id, Attribute::ADC), T).unwrap();
        assert_eq!(ty(&reply), 0x41, "id {id}: {reply:02x?}");
        assert_eq!(reply[1], id, "id {id} not echoed: {reply:02x?}");
    }
    let accept_or_reject = d.transact(&frame::stop_queue(0x33), T).unwrap();
    assert_eq!(accept_or_reject[1], 0x33);
}

#[test]
fn auth_reply_has_id_zero() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let uid = credential(&mut d, 1);
    let reply = raw_auth(&mut d, 0x5A, &uid);
    assert_eq!(ty(&reply), 0x4C, "{reply:02x?}");
    assert_eq!(reply[1], 0x00, "0x4C reply id must be 0: {reply:02x?}");
    assert_eq!(reply.len(), 36, "header + 32 encrypted bytes");
}

#[test]
fn reply_headers_carry_the_auth_level_in_bits_17_18() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let before = d.transact(&get_data(3, Attribute::ADC), T).unwrap();
    assert_eq!(permi(&before), 0, "level 0 before auth: {before:02x?}");

    let uid = credential(&mut d, 1);
    let auth = raw_auth(&mut d, 4, &uid);
    assert_eq!(permi(&auth), 1, "auth reply level: {auth:02x?}");
    let adc = d.transact(&get_data(5, Attribute::ADC), T).unwrap();
    assert_eq!(permi(&adc), 1, "ADC header after level-1 auth: {adc:02x?}");
    assert_eq!(adc[2] & 0x06, 0x02, "byte 2 reads 0x82-style at level 1");
    let accept = d.transact(&frame::stop_queue(6), T).unwrap();
    assert_eq!(ty(&accept), 0x05);
    assert_eq!(permi(&accept), 1, "control replies carry the level too");

    let otp = credential(&mut d, 2);
    let auth2 = raw_auth(&mut d, 7, &otp);
    assert_eq!(permi(&auth2), 2, "auth reply level 2: {auth2:02x?}");
    let adc2 = d.transact(&get_data(8, Attribute::ADC), T).unwrap();
    assert_eq!(permi(&adc2), 2, "ADC header at level 2: {adc2:02x?}");
}
