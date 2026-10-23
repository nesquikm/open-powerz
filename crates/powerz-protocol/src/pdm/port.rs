//! Finding and opening the meter's virtual serial port for the PDM session.

use super::{Error, map_io};
use crate::transport::cdc::{PortCandidate, match_port, open_serial, usb_ports};

/// Pick the meter's serial port among `ports`: only VID 0x5FC9 ports count.
/// With `serial`, the port with that serial number (or, as on Windows, the
/// only POWER-Z port when it reports no serial). Without `serial`, the only
/// POWER-Z port; several are [`Error::Ambiguous`], none is [`Error::NotFound`].
pub fn find_port<'a>(
    ports: &'a [PortCandidate],
    serial: Option<&str>,
) -> Result<&'a PortCandidate, Error> {
    if let Some(serial) = serial {
        return match_port(ports, serial).ok_or(Error::NotFound);
    }
    let mut ours = ports.iter().filter(|p| p.vid == crate::VID);
    match (ours.next(), ours.next()) {
        (None, _) => Err(Error::NotFound),
        (Some(only), None) => Ok(only),
        (Some(_), Some(_)) => Err(Error::Ambiguous {
            count: 2 + ours.count(),
        }),
    }
}

/// Open the meter's serial port (picked by [`find_port`]) at 2 Mbaud 8N1,
/// no flow control, a short read timeout, with its input buffer cleared.
/// Hand the result to [`PdmSession::open`](super::PdmSession::open).
pub fn open_port(serial: Option<&str>) -> Result<Box<dyn serialport::SerialPort>, Error> {
    let ports = usb_ports().map_err(serial_error)?;
    let name = find_port(&ports, serial)?.name.clone();
    open_serial(name).map_err(serial_error)
}

/// Map a `serialport` error to a PDM [`Error`].
fn serial_error(e: serialport::Error) -> Error {
    match e.kind {
        serialport::ErrorKind::NoDevice => Error::NotFound,
        serialport::ErrorKind::Io(kind) => map_io(&std::io::Error::new(kind, e.description)),
        _ => Error::Io(e.description),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, vid: u16, serial: Option<&str>) -> PortCandidate {
        PortCandidate {
            name: name.into(),
            vid,
            pid: 0x0063,
            serial: serial.map(Into::into),
        }
    }

    // ---- AC-PR41FT.1: the library finds the meter's virtual serial port ----

    #[test]
    fn finds_the_only_power_z_port_by_vid() {
        let ports = vec![
            port("/dev/cu.other", 0x1234, Some("X")),
            port("/dev/cu.usbmodem0001", 0x5FC9, Some("123456")),
        ];
        assert_eq!(
            find_port(&ports, None).map(|p| p.name.as_str()),
            Ok("/dev/cu.usbmodem0001")
        );
    }

    #[test]
    fn picks_by_serial_among_several() {
        let ports = vec![
            port("COM5", 0x5FC9, Some("111111")),
            port("COM6", 0x5FC9, Some("123456")),
        ];
        assert_eq!(
            find_port(&ports, Some("123456")).map(|p| p.name.as_str()),
            Ok("COM6")
        );
        assert_eq!(find_port(&ports, Some("999999")), Err(Error::NotFound));
    }

    #[test]
    fn several_ports_without_a_serial_are_ambiguous() {
        let ports = vec![port("COM5", 0x5FC9, None), port("COM6", 0x5FC9, None)];
        assert_eq!(find_port(&ports, None), Err(Error::Ambiguous { count: 2 }));
    }

    #[test]
    fn no_power_z_port_is_not_found() {
        let ports = vec![port("COM3", 0x1234, None)];
        assert_eq!(find_port(&ports, None), Err(Error::NotFound));
        assert_eq!(find_port(&[], None), Err(Error::NotFound));
    }
}
