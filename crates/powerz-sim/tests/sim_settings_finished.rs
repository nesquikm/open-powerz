//! AC-4PJRVM.8: on its CDC persona the demo meter answers a settings write
//! with FINISHED carrying the request id, as the KM003C does, and still
//! applies the write. ACCEPT is not what the meter sends.

mod common;

use common::*;
use powerz_protocol::decode::settings::{WritableField, settings_write_request};
use powerz_protocol::transport::Transport;

/// Session on the CDC persona, where the settings write is taken.
fn cdc() -> SimSession {
    let clock = clock();
    session(device(Persona::Cdc, Scenario::Idle, &clock), &clock)
}

#[test]
fn a_cdc_settings_write_is_answered_with_finished_and_the_request_id() {
    let mut s = cdc();
    let before = s.read_meter_settings().expect("read");
    let request = settings_write_request(9, &before, &[(WritableField::Brightness, 30)])
        .expect("a well-formed write");
    let reply = s
        .transport_mut()
        .transact(&request, T)
        .expect("a reply to the settings write");
    assert_eq!(
        (ty(&reply), reply[1]),
        (0x07, 9),
        "FINISHED with the request id, got {reply:02x?}"
    );
    let after = s.read_meter_settings().expect("read back");
    assert_eq!(after.brightness.value, 30, "the write was applied");
}

#[test]
fn the_session_takes_the_finished_and_reads_the_new_value_back() {
    let mut s = cdc();
    let after = s
        .write_settings(&[(WritableField::Brightness, 30)])
        .expect("the write is taken");
    assert_eq!(after.brightness.value, 30);
}
