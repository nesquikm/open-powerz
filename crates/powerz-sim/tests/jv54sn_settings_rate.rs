//! FR JV54SN: the `settings-write-resets-queue-rate` fault. `[inferred]` On
//! the KM003C a settings write (type 0x0A) sends every key, the rate 0x15
//! included, at its read value, and the running queue seems to go back to
//! the meter's own rate afterwards. Under this fault any accepted settings
//! write re-applies the stored rate to the meter's queue, even while a
//! stream started with 0x0E runs and even when the rate key did not
//! change. Without the fault the simulator keeps its old behaviour.

mod common;

use common::*;
use powerz_protocol::decode::settings::WritableField;
use powerz_protocol::decode::{QueueRecord, Rate};

fn deltas(records: &[QueueRecord]) -> Vec<u32> {
    records
        .windows(2)
        .map(|w| w[1].time_ms() - w[0].time_ms())
        .collect()
}

/// A bulk session streaming at 50 SPS from a meter whose own (stored) rate
/// is 10 SPS, under `faults`.
fn streaming_50_on_a_10_sps_meter(faults: FaultPlan) -> (SimSession, SimClock) {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Load5v1a, &clock);
    // The meter's own rate becomes 10 SPS (written while nothing streams,
    // before the faults are in place).
    let mut twin = session(dev.cdc_twin(), &clock);
    let written = twin
        .write_settings(&[(WritableField::SampleRate, 1)])
        .expect("rate write");
    assert_eq!(written.sample_rate.value, Rate::Sps10);
    let mut s = session(dev.with_faults(faults), &clock);
    s.connect().expect("CONNECT");
    s.start_stream(Rate::Sps50).expect("start at 50 SPS");
    s.poll_stream().unwrap();
    clock.advance(ms(500));
    let p = s.poll_stream().unwrap();
    assert!(
        deltas(&p.records).iter().all(|&d| d == 20),
        "streams at 50 SPS before the write: {:?}",
        deltas(&p.records)
    );
    (s, clock)
}

/// A brightness write on the meter's CDC port (the rate key goes out at
/// its read value, 10 SPS), then the deltas of one second of queue records.
fn deltas_after_a_brightness_write(s: &mut SimSession, clock: &SimClock) -> Vec<u32> {
    let mut cdc = session(s.transport().cdc_twin(), clock);
    let after = cdc
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect("brightness write");
    assert_eq!(after.brightness.value, 80);
    assert_eq!(
        after.sample_rate.value,
        Rate::Sps10,
        "stored rate unchanged"
    );
    s.poll_stream().unwrap(); // drain
    clock.advance(ms(1000));
    deltas(&s.poll_stream().unwrap().records)
}

#[test]
fn jv54sn_under_the_fault_a_settings_write_puts_the_running_queue_back_to_the_stored_rate() {
    let (mut s, clock) =
        streaming_50_on_a_10_sps_meter(FaultPlan::new().settings_write_resets_queue_rate());
    let d = deltas_after_a_brightness_write(&mut s, &clock);
    assert!(
        d.len() >= 8 && d.iter().all(|&x| x == 100),
        "records 100 ms apart after the write: {d:?}"
    );
}

#[test]
fn jv54sn_without_the_fault_a_settings_write_leaves_the_running_queue_alone() {
    let (mut s, clock) = streaming_50_on_a_10_sps_meter(FaultPlan::new());
    let d = deltas_after_a_brightness_write(&mut s, &clock);
    assert!(
        d.len() >= 45 && d.iter().all(|&x| x == 20),
        "still 50 SPS: {d:?}"
    );
}

#[test]
fn jv54sn_under_the_fault_a_restart_after_the_write_streams_at_the_started_rate_again() {
    let (mut s, clock) =
        streaming_50_on_a_10_sps_meter(FaultPlan::new().settings_write_resets_queue_rate());
    deltas_after_a_brightness_write(&mut s, &clock);
    s.stop_stream().expect("0x0F");
    s.start_stream(Rate::Sps50).expect("0x0E at 50 SPS");
    s.poll_stream().unwrap();
    clock.advance(ms(1000));
    let d = deltas(&s.poll_stream().unwrap().records);
    assert!(
        d.len() >= 45 && d.iter().all(|&x| x == 20),
        "50 SPS after the restart: {d:?}"
    );
}

#[test]
fn jv54sn_a_write_the_meter_ignores_does_not_touch_the_queue_under_the_fault() {
    let (mut s, clock) = streaming_50_on_a_10_sps_meter(
        FaultPlan::new()
            .settings_write_resets_queue_rate()
            .ignore_settings_write(),
    );
    let mut cdc = session(s.transport().cdc_twin(), &clock);
    assert!(
        cdc.write_settings(&[(WritableField::Brightness, 80)])
            .is_err()
    );
    s.poll_stream().unwrap();
    clock.advance(ms(1000));
    let d = deltas(&s.poll_stream().unwrap().records);
    assert!(d.iter().all(|&x| x == 20), "{d:?}");
}

#[test]
fn jv54sn_the_fault_parses_from_its_cli_name() {
    assert_eq!(
        FaultPlan::parse("settings-write-resets-queue-rate"),
        Ok(FaultPlan::new().settings_write_resets_queue_rate())
    );
    assert_eq!(
        FaultPlan::parse("ignore-settings-write,settings-write-resets-queue-rate"),
        Ok(FaultPlan::new()
            .ignore_settings_write()
            .settings_write_resets_queue_rate())
    );
}
