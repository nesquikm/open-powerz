//! AC-AXHK2J.7 on the CLI side: `powerz pdm` and `powerz trigger` on a port
//! held by another program say so in plain words, as `settings set` does
//! (`settings_held_port.rs`), and a vanished port is not reported as held.

use powerz_cli::error::CliError;
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

fn open_failing(kind: serialport::ErrorKind, text: &str) -> pdm::Error {
    pdm::open_found(&[meter_port()], Some("KM1"), |_| {
        Err::<(), _>(serialport::Error::new(kind, text))
    })
    .expect_err("the open fails")
}

#[test]
fn a_held_port_says_so_in_plain_words_on_every_trigger_error_path() {
    let held = open_failing(serialport::ErrorKind::NoDevice, "Resource busy");
    for error in [
        CliError::from(held.clone()),
        CliError::from(pdm::pd::Error::Pdm(held.clone())),
        CliError::from(pdm::legacy::Error::Pdm(held)),
    ] {
        let text = error.to_string().to_lowercase();
        assert!(text.contains("serial port"), "{text}");
        assert!(text.contains("another program"), "{text}");
        assert!(!text.contains("no power-z"), "{text}");
        assert_ne!(error.exit_code(), 3, "a held port is not a missing meter");
    }
}

#[test]
fn a_vanished_port_is_not_reported_as_held() {
    let error = CliError::from(open_failing(
        serialport::ErrorKind::NoDevice,
        "The system cannot find the file specified.",
    ));
    assert!(
        !error.to_string().to_lowercase().contains("another program"),
        "{error}"
    );
}
