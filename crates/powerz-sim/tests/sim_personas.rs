//! AC-G0W08Z.8: the CDC persona answers 0x01/0x02/0x03 with no reply and
//! otherwise behaves like bulk; the HID persona answers only single-report
//! GET_DATA and "reboots" (`Error::Disconnected`) on any request whose reply
//! would exceed 64 bytes, so the library HID guard is exercised end to end.

mod common;

use common::*;
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::{Error, Session, memory_read_request};
use powerz_protocol::transport::hid::HidTransport;
use powerz_protocol::transport::{Error as TransportError, Transport};

// ---- CDC ----

#[test]
fn cdc_serves_adc_settings_info_and_the_queue_like_bulk() {
    let clock = clock();
    let mut s = session(device(Persona::Cdc, Scenario::Pd9v2a, &clock), &clock);
    let adc = s.read_adc().unwrap();
    assert_near("VBUS", adc.vbus_v(), 9.0, 0.45);
    let settings = s.read_settings().unwrap();
    assert!(settings.block_a_crc_ok() && settings.block_b_crc_ok());
    // No CONNECT on CDC (0x02 starts ADCNEW there); memory read still works.
    assert_eq!(s.device_info().unwrap().serial, "123456");
    s.start_stream(Rate::Sps50).unwrap();
    s.poll_stream().unwrap();
    clock.advance(ms(1000));
    let n = s.poll_stream().unwrap().records.len();
    assert!((49..=51).contains(&n), "{n} records");
}

#[test]
fn cdc_adcnew_types_get_no_reply() {
    let clock = clock();
    let mut s = session(device(Persona::Cdc, Scenario::Idle, &clock), &clock);
    s.read_adc().unwrap(); // past any stale reply
    let d = s.transport_mut();
    for (ty, id) in [(0x01u8, 0x21u8), (0x02, 0x22), (0x03, 0x23)] {
        assert_eq!(
            d.transact(&[ty, id, 0x00, 0x00], T),
            Err(TransportError::Timeout),
            "type {ty:#04x} on CDC is ADCNEW: no reply"
        );
    }
    s.read_adc()
        .expect("CDC keeps answering the binary protocol");
}

// ---- HID ----

#[test]
fn hid_serves_the_adc_snapshot_through_the_library_hid_transport() {
    let clock = clock();
    let hid = HidTransport::with_device(device(Persona::Hid, Scenario::Load5v1a, &clock));
    let mut s = Session::with_clock(hid, clock.clone());
    let adc = s.read_adc().unwrap();
    assert_near("VBUS", adc.vbus_v(), 5.0, 0.25);
    assert_near("IBUS", adc.ibus_a(), 1.0, 0.05);
}

#[test]
fn hid_guard_refuses_long_replies_before_the_sim_can_reboot() {
    let clock = clock();
    let hid = HidTransport::with_device(device(Persona::Hid, Scenario::Idle, &clock));
    let mut s = Session::with_clock(hid, clock.clone());
    assert_eq!(
        s.read_settings().map(|_| ()),
        Err(Error::Transport(TransportError::UnsupportedOnHid))
    );
    assert_eq!(
        s.poll_stream().map(|_| ()),
        Err(Error::Transport(TransportError::UnsupportedOnHid))
    );
    // Nothing reached the simulator, so it did not reboot.
    s.read_adc().expect("ADC still works over HID");
}

#[test]
fn hid_adc_reply_fits_one_report() {
    let clock = clock();
    let mut d = device(Persona::Hid, Scenario::Idle, &clock);
    let reply = d.transact(&get_data(1, Attribute::ADC), T).unwrap();
    assert!(reply.len() <= 64, "{} bytes", reply.len());
    assert_eq!(ty(&reply), 0x41);
    assert_eq!(reply[1], 1);
    assert_eq!(sections(&reply[..52])[0].1.len(), 44);
}

#[test]
fn hid_reply_over_64_bytes_reboots_the_sim() {
    for mask in [
        Attribute::SETTINGS,
        Attribute::ADC_QUEUE,
        Attribute::ADC | Attribute::SETTINGS,
    ] {
        let clock = clock();
        let mut d = device(Persona::Hid, Scenario::Idle, &clock);
        clock.advance(ms(500)); // a queue poll would hold ~25 records
        assert_eq!(
            d.transact(&get_data(1, mask), T),
            Err(TransportError::Disconnected),
            "mask {:#05x}",
            mask.bits()
        );
    }
}

#[test]
fn hid_control_commands_get_no_reply() {
    let clock = clock();
    let mut d = device(Persona::Hid, Scenario::Idle, &clock);
    for req in [
        frame::connect(1).to_vec(),
        frame::start_queue(2, 2).to_vec(),
        frame::stop_queue(3).to_vec(),
        memory_read_request(4, 0x4001_0450, 12),
    ] {
        assert_eq!(
            d.transact(&req, T),
            Err(TransportError::Timeout),
            "{:02x?}",
            &req[..4]
        );
    }
    // The meter was not upset by the ignored commands.
    let reply = d.transact(&get_data(5, Attribute::ADC), T).unwrap();
    assert_eq!(reply[1], 5);
}

#[test]
fn cdc_starts_with_no_stale_disconnect() {
    // The stale DISCONNECT waits on the bulk IN endpoint only; on CDC the
    // first ADCNEW-type request still gets no reply at all.
    let mut dev = SimDevice::new(Persona::Cdc, Scenario::Idle);
    assert_eq!(
        dev.transact(
            &[0x01, 0x01, 0x00, 0x00],
            std::time::Duration::from_millis(20)
        ),
        Err(TransportError::Timeout)
    );
}
