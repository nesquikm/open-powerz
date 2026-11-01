//! AC-AXHK2J.7, the library side: the PDM path tells a port held by another
//! program from a missing or vanished one, as the settings path does.
//! [`pdm::open_found`] is [`pdm::open_port`] with the listing and the open
//! passed in, so no real port is touched.

use powerz_protocol::pdm::{self, Error};
use powerz_protocol::transport::cdc::PortCandidate;

const PORT: &str = "/dev/cu.usbmodem1101";

fn meter_port() -> PortCandidate {
    PortCandidate {
        name: PORT.into(),
        vid: powerz_protocol::VID,
        pid: 0x0001,
        serial: Some("KM1".into()),
    }
}

fn open_failing(
    ports: &[PortCandidate],
    serial: Option<&str>,
    kind: serialport::ErrorKind,
    text: &str,
) -> Result<(), Error> {
    pdm::open_found(ports, serial, |_| {
        Err::<(), _>(serialport::Error::new(kind, text))
    })
}

#[test]
fn a_listed_port_that_answers_held_is_named_held() {
    for serial in [Some("KM1"), None] {
        for text in [
            "Resource busy",
            "Unable to acquire exclusive lock on serial port",
            "Access is denied.",
        ] {
            let result = open_failing(
                &[meter_port()],
                serial,
                serialport::ErrorKind::NoDevice,
                text,
            );
            assert_eq!(result, Err(Error::PortHeld { port: PORT.into() }), "{text}");
        }
    }
}

#[test]
fn the_held_error_says_so_in_plain_words() {
    let text = Error::PortHeld { port: PORT.into() }.to_string();
    assert!(text.contains("serial port"), "{text}");
    assert!(text.contains("another program"), "{text}");
    assert!(text.contains(PORT), "{text}");
}

#[test]
fn a_port_that_vanished_before_the_open_is_not_held() {
    for (kind, text) in [
        (
            serialport::ErrorKind::NoDevice,
            "The system cannot find the file specified.",
        ),
        (
            serialport::ErrorKind::Io(std::io::ErrorKind::NotFound),
            "No such file or directory",
        ),
        (serialport::ErrorKind::Unknown, "No such device"),
        (serialport::ErrorKind::Unknown, "No such device or address"),
    ] {
        let result = open_failing(&[meter_port()], Some("KM1"), kind, text);
        assert_eq!(result, Err(Error::Disconnected), "{text}");
    }
}

#[test]
fn no_listed_port_stays_not_found_and_opens_nothing() {
    let result = pdm::open_found(&[], Some("KM1"), |_| -> Result<(), serialport::Error> {
        panic!("no port matched, nothing to open")
    });
    assert_eq!(result, Err(Error::NotFound));
}

#[test]
fn an_open_port_comes_back() {
    let opened = pdm::open_found(&[meter_port()], None, Ok);
    assert_eq!(opened, Ok(PORT.to_owned()));
}
