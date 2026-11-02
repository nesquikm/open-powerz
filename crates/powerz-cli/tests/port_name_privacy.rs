//! AC-AXHK2Q.8, the CLI side: no message the CLI prints for a failed serial
//! open carries the port's name, which on macOS holds the meter's serial.
//! The errors come through the real open mappings and the real conversion to
//! [`CliError`]. The serial and the port name are made up; no port is opened.

use powerz_cli::error::CliError;
use powerz_protocol::transport::cdc::{self, PortCandidate};
use powerz_protocol::{pdm, session};
use serialport::ErrorKind;
use std::io::ErrorKind as Io;

const SERIAL: &str = "FAKE00123456";
const PORT: &str = "/dev/cu.usbmodemFAKE001234561";

fn meter_port() -> PortCandidate {
    PortCandidate {
        name: PORT.into(),
        vid: powerz_protocol::VID,
        pid: 0x0001,
        serial: Some(SERIAL.into()),
    }
}

/// Every `serialport` kind with a bare OS text and with one naming the port.
fn failures() -> Vec<(ErrorKind, String)> {
    let kinds = [
        ErrorKind::NoDevice,
        ErrorKind::InvalidInput,
        ErrorKind::Unknown,
        ErrorKind::Io(Io::NotFound),
        ErrorKind::Io(Io::PermissionDenied),
        ErrorKind::Io(Io::TimedOut),
        ErrorKind::Io(Io::Other),
    ];
    kinds
        .into_iter()
        .flat_map(|kind| {
            [
                (kind, "Resource busy".to_owned()),
                (kind, format!("Could not open {PORT}")),
            ]
        })
        .collect()
}

fn assert_clean(error: CliError, case: &str) {
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(!text.contains(PORT), "{case}: {text}");
        assert!(!text.contains(SERIAL), "{case}: {text}");
    }
}

#[test]
fn no_trigger_path_message_names_the_port() {
    for (kind, description) in failures() {
        let opened = pdm::open_found(&[meter_port()], Some(SERIAL), |_| {
            Err::<(), _>(serialport::Error::new(kind, description.clone()))
        })
        .expect_err("the open fails");
        let case = format!("{kind:?} / {description}");
        assert_clean(CliError::from(opened.clone()), &case);
        assert_clean(CliError::from(pdm::pd::Error::Pdm(opened.clone())), &case);
        assert_clean(CliError::from(pdm::legacy::Error::Pdm(opened)), &case);
    }
}

#[test]
fn no_settings_path_message_names_the_port() {
    for (kind, description) in failures() {
        let opened = cdc::open_listed(&[meter_port()], SERIAL, |_| {
            Err::<(), _>(serialport::Error::new(kind, description.clone()))
        })
        .expect_err("the open fails");
        let case = format!("{kind:?} / {description}");
        assert_clean(CliError::from(opened.clone()), &case);
        assert_clean(CliError::from(session::Error::Transport(opened)), &case);
    }
}
