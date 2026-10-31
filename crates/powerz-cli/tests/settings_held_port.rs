//! AC-4PJRVM.7 on the CLI side: a settings write whose serial port is held by
//! another program says so in plain words. The error comes through the real
//! open mapping, from the kind `serialport` 4.10.1 returns for a held port.

use powerz_cli::error::CliError;
use powerz_protocol::session;
use powerz_protocol::transport::{self, cdc};

fn meter_port() -> cdc::PortCandidate {
    cdc::PortCandidate {
        name: "/dev/cu.usbmodem1101".into(),
        vid: powerz_protocol::VID,
        pid: 0x0001,
        serial: Some("KM1".into()),
    }
}

fn open_with_no_device(ports: &[cdc::PortCandidate]) -> transport::Error {
    cdc::open_listed(ports, "KM1", |_| {
        Err::<(), _>(serialport::Error::new(
            serialport::ErrorKind::NoDevice,
            "Resource busy",
        ))
    })
    .expect_err("the open fails")
}

#[test]
fn a_held_port_says_so_in_plain_words() {
    let held = open_with_no_device(&[meter_port()]);
    for error in [
        CliError::from(held.clone()),
        CliError::from(session::Error::Transport(held)),
    ] {
        let text = error.to_string().to_lowercase();
        assert!(text.contains("serial port"), "{text}");
        assert!(text.contains("another program"), "{text}");
        assert!(!text.contains("no matching device"), "{text}");
        assert_ne!(error.exit_code(), 3, "a held port is not a missing meter");
    }
}

#[test]
fn a_meter_that_is_not_listed_is_still_no_device() {
    let error = CliError::from(open_with_no_device(&[]));
    assert!(matches!(error, CliError::NoDevice(_)), "{error:?}");
}
