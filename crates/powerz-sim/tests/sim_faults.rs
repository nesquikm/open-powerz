//! AC-G0W08Z.7: per-device fault injection (reply timeouts, disconnect
//! mid-stream, wrong reply ids) surfaces through the library as the matching
//! typed error or as a recovered stale reply.

mod common;

use common::*;
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::Error;
use powerz_protocol::transport::{Error as TransportError, Transport};

fn faulty(scenario: Scenario, faults: FaultPlan) -> (SimSession, SimClock) {
    let clock = clock();
    let dev = device(Persona::Bulk, scenario, &clock).with_faults(faults);
    (session(dev, &clock), clock)
}

#[test]
fn timeout_on_one_request_is_a_typed_timeout_and_the_next_request_works() {
    let (mut s, _clock) = faulty(Scenario::Load5v1a, FaultPlan::new().timeout_at(2));
    s.connect().unwrap(); // request 1
    assert_eq!(
        s.read_adc().map(|_| ()),
        Err(Error::Transport(TransportError::Timeout)),
        "request 2 times out"
    );
    let adc = s.read_adc().expect("request 3 is answered");
    assert_near("VBUS", adc.vbus_v(), 5.0, 0.25);
}

#[test]
fn timeout_always_fails_connect() {
    let (mut s, _clock) = faulty(Scenario::Idle, FaultPlan::new().timeout_always());
    assert_eq!(s.connect(), Err(Error::Transport(TransportError::Timeout)));
}

#[test]
fn disconnect_mid_stream_is_a_typed_disconnect() {
    let (mut s, clock) = faulty(Scenario::Idle, FaultPlan::new().disconnect_after_polls(3));
    s.connect().unwrap();
    s.start_stream(Rate::Sps50).unwrap();
    for i in 0..3 {
        clock.advance(ms(100));
        s.poll_stream()
            .unwrap_or_else(|e| panic!("poll {i} before the unplug: {e}"));
    }
    clock.advance(ms(100));
    assert_eq!(
        s.poll_stream().map(|_| ()),
        Err(Error::Transport(TransportError::Disconnected))
    );
    assert_eq!(
        s.read_adc().map(|_| ()),
        Err(Error::Transport(TransportError::Disconnected)),
        "the meter stays gone"
    );
}

#[test]
fn wrong_id_reply_is_recovered_as_a_stale_reply() {
    let (mut s, _clock) = faulty(Scenario::Load5v1a, FaultPlan::new().wrong_id_at(2));
    s.connect().unwrap();
    let adc = s.read_adc().expect("session re-reads past the wrong id");
    assert_near("VBUS", adc.vbus_v(), 5.0, 0.25);
}

#[test]
fn wrong_id_reply_on_the_wire_is_followed_by_the_right_one() {
    let clock = clock();
    let dev =
        device(Persona::Bulk, Scenario::Idle, &clock).with_faults(FaultPlan::new().wrong_id_at(2));
    let mut d = raw_connect(dev);
    let wrong = d.transact(&get_data(5, Attribute::ADC), T).unwrap();
    assert_ne!(wrong[1], 5, "request 2 gets a wrong id: {wrong:02x?}");
    let right = d.read(T).unwrap();
    assert_eq!(ty(&right), 0x41);
    assert_eq!(right[1], 5);
    let next = d.transact(&frame::stop_queue(6), T).unwrap();
    assert_eq!(next[1], 6, "only request 2 is affected");
}

#[test]
fn permission_denied_is_a_typed_permission_error() {
    let (mut s, _clock) = faulty(Scenario::Idle, FaultPlan::new().permission_denied());
    match s.connect() {
        Err(Error::Transport(TransportError::Permission(_))) => {}
        other => panic!("expected a permission error, got {other:?}"),
    }
}

#[test]
fn faults_are_per_device() {
    let clock = clock();
    let _broken = device(Persona::Bulk, Scenario::Idle, &clock)
        .with_faults(FaultPlan::new().timeout_always());
    let mut fine = session(device(Persona::Bulk, Scenario::Idle, &clock), &clock);
    fine.connect().expect("a second device has no faults");
}

#[test]
fn fault_plan_parses_the_env_syntax() {
    assert_eq!(
        FaultPlan::parse("timeout@2,disconnect@3,wrong-id@4,permission"),
        Ok(FaultPlan::new()
            .timeout_at(2)
            .disconnect_after_polls(3)
            .wrong_id_at(4)
            .permission_denied())
    );
    assert_eq!(
        FaultPlan::parse("timeout"),
        Ok(FaultPlan::new().timeout_always())
    );
    assert_eq!(FaultPlan::parse(""), Ok(FaultPlan::new()));
    assert!(FaultPlan::parse("bogus").is_err());
    assert!(FaultPlan::parse("timeout@x").is_err());
}
