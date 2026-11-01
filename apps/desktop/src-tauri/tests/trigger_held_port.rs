//! AC-AXHK2J.7 on the desktop side: opening the trigger panel on a port held
//! by another program says so in plain words, as a settings write does, and
//! a vanished port is not reported as held.

use open_powerz_desktop_lib::trigger::pdm_error;
use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};
use powerz_protocol::pdm;
use powerz_protocol::transport::cdc::PortCandidate;

fn meter_port() -> PortCandidate {
    PortCandidate {
        name: "/dev/cu.usbmodem1101".into(),
        vid: powerz_protocol::VID,
        pid: 0x0001,
        serial: Some("KM1".into()),
    }
}

fn open_failing(kind: serialport::ErrorKind, text: &str) -> UiError {
    let err = pdm::open_found(&[meter_port()], Some("KM1"), |_| {
        Err::<(), _>(serialport::Error::new(kind, text))
    })
    .expect_err("the open fails");
    pdm_error(&err)
}

#[test]
fn a_held_port_says_so_in_plain_words() {
    let e = open_failing(serialport::ErrorKind::NoDevice, "Resource busy");
    assert_eq!(e.kind, ErrorKind::DeviceBusy);
    assert_eq!(
        e.message,
        "The meter's serial port is in use by another program."
    );
    assert_eq!(
        e.hint.as_deref(),
        Some("Close that program, then try again.")
    );
}

#[test]
fn a_vanished_port_is_not_reported_as_held() {
    for (kind, text) in [
        (
            serialport::ErrorKind::NoDevice,
            "The system cannot find the file specified.",
        ),
        (serialport::ErrorKind::Unknown, "No such device"),
    ] {
        let e = open_failing(kind, text);
        assert_eq!(e.kind, ErrorKind::Disconnected, "{text}");
        assert!(!e.message.contains("another program"), "{text}: {e:?}");
    }
}
