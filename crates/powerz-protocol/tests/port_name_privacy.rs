//! AC-AXHK2Q.8, the library side: on macOS a serial port's name holds the
//! meter's serial, so no error carries it, in its text or its debug form.
//! Every error the `serialport` crate can hand back goes through the real
//! open mappings of both paths, the PDM one and the transport one, with the
//! port name in its description as well. The serial and the name are made up.

use powerz_protocol::pdm;
use powerz_protocol::transport::{self, cdc};
use serialport::ErrorKind;
use std::io::ErrorKind as Io;

const SERIAL: &str = "FAKE00123456";
const PORT: &str = "/dev/cu.usbmodemFAKE001234561";
const HELD_TEXT: &str = "the meter's serial port is in use by another program";

fn meter_port() -> cdc::PortCandidate {
    cdc::PortCandidate {
        name: PORT.into(),
        vid: powerz_protocol::VID,
        pid: 0x0001,
        serial: Some(SERIAL.into()),
    }
}

/// Each kind `serialport` reports, the I/O kinds an open or a listing can
/// carry included.
fn kinds() -> Vec<ErrorKind> {
    let mut kinds = vec![
        ErrorKind::NoDevice,
        ErrorKind::InvalidInput,
        ErrorKind::Unknown,
    ];
    kinds.extend(
        [
            Io::NotFound,
            Io::PermissionDenied,
            Io::TimedOut,
            Io::WouldBlock,
            Io::BrokenPipe,
            Io::AddrInUse,
            Io::InvalidData,
            Io::Other,
        ]
        .map(ErrorKind::Io),
    );
    kinds
}

/// What the serial library may say: a bare OS text, or one naming the port.
fn descriptions() -> Vec<String> {
    vec![
        "Resource busy".to_owned(),
        format!("{PORT}: Resource busy"),
        format!("Could not open {PORT}"),
        format!("Unable to acquire exclusive lock on serial port {PORT}"),
    ]
}

fn assert_clean(error: &impl std::fmt::Display, debug: &impl std::fmt::Debug, case: &str) {
    for text in [error.to_string(), format!("{debug:?}")] {
        assert!(!text.contains(PORT), "{case}: {text}");
        assert!(!text.contains(SERIAL), "{case}: {text}");
        assert!(!text.contains("usbmodem"), "{case}: {text}");
    }
}

fn pdm_open(kind: ErrorKind, description: &str) -> pdm::Error {
    pdm::open_found(&[meter_port()], Some(SERIAL), |_| {
        Err::<(), _>(serialport::Error::new(kind, description))
    })
    .expect_err("the open fails")
}

fn transport_open(kind: ErrorKind, description: &str) -> transport::Error {
    cdc::open_listed(&[meter_port()], SERIAL, |_| {
        Err::<(), _>(serialport::Error::new(kind, description))
    })
    .expect_err("the open fails")
}

#[test]
fn a_held_port_is_called_the_meters_serial_port_on_the_pdm_path() {
    let error = pdm_open(ErrorKind::NoDevice, "Resource busy");
    assert!(matches!(error, pdm::Error::PortHeld), "{error:?}");
    assert_eq!(error.to_string(), HELD_TEXT);
}

#[test]
fn a_held_port_is_called_the_meters_serial_port_on_the_transport_path() {
    let error = transport_open(ErrorKind::NoDevice, "Resource busy");
    assert!(matches!(error, transport::Error::PortHeld), "{error:?}");
    assert_eq!(error.to_string(), HELD_TEXT);
}

#[test]
fn no_pdm_error_mapped_from_the_serial_library_names_the_port() {
    for kind in kinds() {
        for description in descriptions() {
            let error = pdm_open(kind, &description);
            assert_clean(&error, &error, &format!("{kind:?} / {description}"));
        }
    }
}

#[test]
fn no_transport_error_mapped_from_the_serial_library_names_the_port() {
    for kind in kinds() {
        for description in descriptions() {
            let error = transport_open(kind, &description);
            assert_clean(&error, &error, &format!("{kind:?} / {description}"));
        }
    }
}

#[test]
fn an_io_error_quoting_the_port_is_clean_after_the_shared_mapping() {
    for os in ["macos", "linux", "windows"] {
        for kind in [Io::PermissionDenied, Io::Other, Io::TimedOut] {
            let quoted = std::io::Error::new(kind, format!("{PORT}: failed"));
            let error = transport::map_io_error(&quoted, os);
            assert_clean(&error, &error, &format!("{os} {kind:?}"));
        }
    }
}
