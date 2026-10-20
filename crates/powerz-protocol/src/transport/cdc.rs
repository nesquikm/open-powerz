//! CDC serial transport on `serialport` for the binary protocol.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use super::{Error, GuardedTransport, Transport, map_io_error};
use crate::frame::{DeclaredEnd, HEADER_LEN, MainHeader, declared_end, frame_type, msg};

/// Wait after opening the port before the first frame.
pub const FIRST_FRAME_DELAY: Duration = Duration::from_millis(200);

/// One serial port as enumerated by the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortCandidate {
    /// Port name or path (e.g. `/dev/cu.usbmodem1101`, `COM5`).
    pub name: String,
    /// USB vendor ID, if the port is a USB port.
    pub vid: u16,
    /// USB product ID.
    pub pid: u16,
    /// USB serial string.
    pub serial: Option<String>,
}

/// Pick the meter's port: VID 0x5FC9 and the given serial number.
pub fn match_port<'a>(ports: &'a [PortCandidate], serial: &str) -> Option<&'a PortCandidate> {
    let ours: Vec<&PortCandidate> = ports.iter().filter(|p| p.vid == crate::VID).collect();
    ours.iter()
        .find(|p| p.serial.as_deref() == Some(serial))
        .copied()
        // Windows often reports no serial for a composite device's COM port:
        // fall back to the only POWER-Z port, never guess between several.
        .or_else(|| match ours.as_slice() {
            [only] if only.serial.is_none() => Some(*only),
            _ => None,
        })
}

/// Length of the first complete frame at the start of `buf`, walking the
/// section headers of a PUT_DATA reply; `None` while more bytes are needed.
///
/// Control replies are 4 bytes. A PUT_DATA header with `obj == 0` carries no
/// sections. The 0x44 / 0x4C replies are framed by `obj` (payload words). A
/// PD_STATUS section has no size and takes whatever has arrived.
pub fn frame_complete(buf: &[u8]) -> Option<usize> {
    let header = MainHeader::decode(buf).ok()?.data();
    match header.ty {
        msg::PUT_DATA if header.obj == 0 => Some(HEADER_LEN),
        msg::PUT_DATA => match declared_end(buf) {
            DeclaredEnd::Complete(end) => (buf.len() >= end).then_some(end),
            DeclaredEnd::MissingHeader(_) => None,
            DeclaredEnd::PdStatus { .. } => Some(buf.len()),
        },
        msg::MEM_READ_CONFIRM | msg::AUTH_REPLY => {
            let len = HEADER_LEN + 4 * usize::from(header.obj);
            (buf.len() >= len).then_some(len)
        }
        _ => Some(HEADER_LEN),
    }
}

/// Refuse frame types 0x01..0x03 (they control the ADCNEW stream on CDC).
pub fn check_cdc(request: &[u8]) -> Result<(), Error> {
    match frame_type(request) {
        Some(ty) if (0x01..=0x03).contains(&ty) => Err(Error::UnsupportedOnCdc { ty }),
        _ => Ok(()),
    }
}

/// Binary-protocol transport over the CDC data interface.
#[derive(Debug)]
pub struct CdcTransport<P> {
    port: P,
    /// Bytes read past the end of the last returned frame.
    pending: Vec<u8>,
}

impl<P: Read + Write> CdcTransport<P> {
    /// Wrap an already-open, already-cleared port.
    pub fn with_port(port: P) -> GuardedTransport<CdcTransport<P>> {
        GuardedTransport::new(CdcTransport {
            port,
            pending: Vec::new(),
        })
    }

    /// Read until one frame is complete or `timeout` passes.
    fn read_frame(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; READ_CHUNK];
        loop {
            if let Some(len) = frame_complete(&self.pending) {
                let rest = self.pending.split_off(len);
                return Ok(std::mem::replace(&mut self.pending, rest));
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            match self.port.read(&mut chunk) {
                // End of stream: the port went away.
                Ok(0) => return Err(Error::Disconnected),
                Ok(n) => self.pending.extend_from_slice(&chunk[..n]),
                Err(e) => match e.kind() {
                    io::ErrorKind::TimedOut
                    | io::ErrorKind::WouldBlock
                    | io::ErrorKind::Interrupted => {}
                    _ => return Err(map_io_error(&e, std::env::consts::OS)),
                },
            }
        }
    }
}

/// Bytes requested per serial read.
const READ_CHUNK: usize = 4096;
/// Per-read timeout on the real port; the transport loops to its own deadline.
const PORT_READ_TIMEOUT: Duration = Duration::from_millis(20);
/// Baud rate the vendor tools use (CDC ignores it).
const BAUD: u32 = 2_000_000;

impl CdcTransport<Box<dyn serialport::SerialPort>> {
    /// Open the meter's port by serial number, clear input and wait
    /// [`FIRST_FRAME_DELAY`].
    pub fn open(serial: &str) -> Result<GuardedTransport<Self>, Error> {
        let ports: Vec<PortCandidate> = serialport::available_ports()
            .map_err(serial_error)?
            .into_iter()
            .filter_map(|p| match p.port_type {
                serialport::SerialPortType::UsbPort(usb) => Some(PortCandidate {
                    name: p.port_name,
                    vid: usb.vid,
                    pid: usb.pid,
                    serial: usb.serial_number,
                }),
                _ => None,
            })
            .collect();
        let name = match_port(&ports, serial)
            .ok_or(Error::NotFound)?
            .name
            .clone();
        let port = serialport::new(name, BAUD)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(PORT_READ_TIMEOUT)
            .open()
            .map_err(serial_error)?;
        port.clear(serialport::ClearBuffer::Input)
            .map_err(serial_error)?;
        std::thread::sleep(FIRST_FRAME_DELAY);
        Ok(CdcTransport::with_port(port))
    }
}

fn serial_error(e: serialport::Error) -> Error {
    match e.kind {
        serialport::ErrorKind::NoDevice => Error::NotFound,
        serialport::ErrorKind::Io(kind) => {
            map_io_error(&io::Error::new(kind, e.description), std::env::consts::OS)
        }
        _ => Error::Io(e.description),
    }
}

impl<P: Read + Write> Transport for CdcTransport<P> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        check_cdc(request)?;
        // One request in flight: anything still buffered belongs to an
        // earlier reply that timed out, so it must not prefix this one.
        self.pending.clear();
        let os = std::env::consts::OS;
        self.port
            .write_all(request)
            .map_err(|e| map_io_error(&e, os))?;
        self.port.flush().map_err(|e| map_io_error(&e, os))?;
        self.read_frame(timeout)
    }

    /// Follow-up transfers (memory-read data) carry no header, so they are
    /// returned raw: whatever is buffered, else the next chunk that arrives.
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        if !self.pending.is_empty() {
            return Ok(std::mem::take(&mut self.pending));
        }
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; READ_CHUNK];
        while Instant::now() < deadline {
            match self.port.read(&mut chunk) {
                Ok(0) => return Err(Error::Disconnected),
                Ok(n) => return Ok(chunk[..n].to_vec()),
                Err(e) => match e.kind() {
                    io::ErrorKind::TimedOut
                    | io::ErrorKind::WouldBlock
                    | io::ErrorKind::Interrupted => {}
                    _ => return Err(map_io_error(&e, std::env::consts::OS)),
                },
            }
        }
        Err(Error::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::test_util::hex_vector;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::io;
    use std::rc::Rc;

    const ADC_52: &str = include_str!("../../tests/vectors/adc_52.hex");
    const ADC_SETTINGS_236: &str = include_str!("../../tests/vectors/adc_settings_236.hex");
    const T: Duration = Duration::from_millis(100);

    #[derive(Default)]
    struct State {
        written: Vec<u8>,
        chunks: VecDeque<io::Result<Vec<u8>>>,
    }

    /// Fake serial port: records writes, returns scripted read chunks, then
    /// times out.
    #[derive(Clone, Default)]
    struct FakePort(Rc<RefCell<State>>);

    impl FakePort {
        fn with_chunks(chunks: Vec<io::Result<Vec<u8>>>) -> FakePort {
            let p = FakePort::default();
            p.0.borrow_mut().chunks = chunks.into();
            p
        }
        fn written(&self) -> Vec<u8> {
            self.0.borrow().written.clone()
        }
    }

    impl Read for FakePort {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut s = self.0.borrow_mut();
            match s.chunks.pop_front() {
                Some(Ok(mut chunk)) => {
                    let n = chunk.len().min(buf.len());
                    buf[..n].copy_from_slice(&chunk[..n]);
                    if n < chunk.len() {
                        let rest = chunk.split_off(n);
                        s.chunks.push_front(Ok(rest));
                    }
                    Ok(n)
                }
                Some(Err(e)) => Err(e),
                None => Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout")),
            }
        }
    }

    impl Write for FakePort {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn split(frame: &[u8], sizes: &[usize]) -> Vec<io::Result<Vec<u8>>> {
        let mut out = Vec::new();
        let mut at = 0;
        for &n in sizes {
            out.push(Ok(frame[at..at + n].to_vec()));
            at += n;
        }
        assert_eq!(at, frame.len(), "split sizes must cover the frame");
        out
    }

    // ---- AC-SJGK7E.7: port matching ----

    #[test]
    fn match_port_picks_vid_and_serial() {
        let ports = vec![
            PortCandidate {
                name: "/dev/cu.other".into(),
                vid: 0x1234,
                pid: 0x0063,
                serial: Some("SERIAL-0001".into()),
            },
            PortCandidate {
                name: "/dev/cu.km-b".into(),
                vid: 0x5FC9,
                pid: 0x0063,
                serial: Some("SERIAL-0002".into()),
            },
            PortCandidate {
                name: "/dev/cu.km-a".into(),
                vid: 0x5FC9,
                pid: 0x0063,
                serial: Some("SERIAL-0001".into()),
            },
        ];
        assert_eq!(
            match_port(&ports, "SERIAL-0001").map(|p| p.name.as_str()),
            Some("/dev/cu.km-a")
        );
        assert_eq!(match_port(&ports, "SERIAL-9999"), None);
    }

    // ---- AC-SJGK7E.7: frame completion by section walk ----

    #[test]
    fn frame_complete_walks_single_section() {
        let adc = hex_vector(ADC_52);
        assert_eq!(adc.len(), 52);
        assert_eq!(frame_complete(&adc), Some(52));
        assert_eq!(frame_complete(&adc[..51]), None);
        assert_eq!(frame_complete(&adc[..8]), None);
        assert_eq!(frame_complete(&adc[..3]), None);
    }

    #[test]
    fn frame_complete_walks_chained_sections() {
        let f = hex_vector(ADC_SETTINGS_236);
        assert_eq!(f.len(), 236);
        assert_eq!(frame_complete(&f), Some(236));
        assert_eq!(frame_complete(&f[..100]), None);
        assert_eq!(frame_complete(&f[..54]), None);
    }

    #[test]
    fn frame_complete_ignores_trailing_bytes() {
        let mut adc = hex_vector(ADC_52);
        adc.extend_from_slice(&[0x41, 0x02, 0x80]);
        assert_eq!(frame_complete(&adc), Some(52));
    }

    #[test]
    fn frame_complete_control_and_empty_put_data_are_four_bytes() {
        assert_eq!(frame_complete(&[0x05, 0x01, 0x00, 0x00]), Some(4)); // ACCEPT
        assert_eq!(frame_complete(&[0x06, 0x01, 0x00, 0x00]), Some(4)); // REJECT
        assert_eq!(frame_complete(&[0x41, 0x07, 0x00, 0x00]), Some(4)); // empty PUT_DATA
    }

    // ---- AC-SJGK7E.7: reassembly over split reads ----

    #[test]
    fn transact_reassembles_reply_split_over_reads() {
        let adc = hex_vector(ADC_52);
        let port = FakePort::with_chunks(split(&adc, &[3, 7, 30, 12]));
        let mut t = CdcTransport::with_port(port.clone());
        let req = [0x0C, 0x01, 0x02, 0x00];
        assert_eq!(t.transact(&req, T).unwrap(), adc);
        assert_eq!(port.written(), req.to_vec());
    }

    #[test]
    fn transact_reassembles_chained_reply() {
        let f = hex_vector(ADC_SETTINGS_236);
        let port = FakePort::with_chunks(split(&f, &[64, 64, 64, 44]));
        let mut t = CdcTransport::with_port(port);
        assert_eq!(t.transact(&[0x0C, 0x02, 0x12, 0x00], T).unwrap(), f);
    }

    #[test]
    fn transact_times_out_on_incomplete_reply() {
        let adc = hex_vector(ADC_52);
        let port = FakePort::with_chunks(vec![Ok(adc[..20].to_vec())]);
        let mut t = CdcTransport::with_port(port);
        assert_eq!(
            t.transact(&[0x0C, 0x01, 0x02, 0x00], T),
            Err(Error::Timeout)
        );
    }

    #[test]
    fn transact_after_timeout_discards_stale_partial_bytes() {
        let adc = hex_vector(ADC_52);
        // The first reply arrives only partly and the read times out.
        let port = FakePort::with_chunks(vec![Ok(adc[..20].to_vec())]);
        let mut t = CdcTransport::with_port(port.clone());
        let req = [0x0C, 0x01, 0x02, 0x00];
        assert_eq!(
            t.transact(&req, Duration::from_millis(30)),
            Err(Error::Timeout)
        );
        // The next request gets a full, fresh reply, not the stale prefix.
        port.0.borrow_mut().chunks.push_back(Ok(adc.clone()));
        assert_eq!(t.transact(&req, T).unwrap(), adc);
    }

    #[test]
    fn follow_up_read_returns_raw_bytes_not_a_parsed_frame() {
        // Memory-read data is headerless AES output; a block that happens to
        // start like a PUT_DATA header must still come back as-is.
        let mut data = vec![0x41, 0x01, 0x80, 0x02];
        data.extend([0x5A; 12]);
        let port = FakePort::with_chunks(vec![Ok(data.clone())]);
        let mut t = CdcTransport::with_port(port);
        assert_eq!(t.read(Duration::from_millis(50)).unwrap(), data);
    }

    #[test]
    fn follow_up_read_returns_bytes_left_after_the_confirmation() {
        // Confirmation (0x44, framed by obj = 4 words) and data in one chunk.
        let mut chunk = vec![0xC4, 0x01, 0x01, 0x01];
        chunk.extend([0x11; 16]);
        chunk.extend([0x41; 16]);
        let port = FakePort::with_chunks(vec![Ok(chunk.clone())]);
        let mut t = CdcTransport::with_port(port);
        let confirm = t.transact(&[0x44, 0x01, 0x01, 0x01], T).unwrap();
        assert_eq!(confirm, chunk[..20].to_vec());
        assert_eq!(
            t.read(Duration::from_millis(50)).unwrap(),
            chunk[20..].to_vec()
        );
    }

    #[test]
    fn transact_maps_broken_pipe_to_disconnected() {
        let port = FakePort::with_chunks(vec![Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "unplugged",
        ))]);
        let mut t = CdcTransport::with_port(port);
        assert_eq!(
            t.transact(&[0x0C, 0x01, 0x02, 0x00], T),
            Err(Error::Disconnected)
        );
    }

    // ---- AC-SJGK7E.7: CDC type refusal ----

    #[test]
    fn check_cdc_refuses_types_1_to_3() {
        for ty in [0x01, 0x02, 0x03] {
            assert_eq!(
                check_cdc(&[ty, 0x01, 0x00, 0x00]),
                Err(Error::UnsupportedOnCdc { ty })
            );
        }
        assert_eq!(check_cdc(&[0x0C, 0x01, 0x02, 0x00]), Ok(()));
    }

    #[test]
    fn transact_refuses_types_1_to_3_without_writing() {
        let port = FakePort::default();
        let mut t = CdcTransport::with_port(port.clone());
        for ty in [0x01, 0x02, 0x03] {
            assert_eq!(
                t.transact(&[ty, 0x01, 0x00, 0x00], T),
                Err(Error::UnsupportedOnCdc { ty })
            );
        }
        assert!(port.written().is_empty());
    }

    // ---- AC-SJGK7E.8: the guard runs on CDC too ----

    #[test]
    fn transact_runs_the_guard_without_writing() {
        let port = FakePort::default();
        let mut t = CdcTransport::with_port(port.clone());
        assert_eq!(
            t.transact(&[0x48, 0x01, 0x00, 0x00], T),
            Err(Error::Forbidden { ty: 0x48 })
        );
        assert!(port.written().is_empty());
    }

    #[test]
    fn eof_from_the_port_is_disconnected() {
        let port = FakePort::with_chunks(vec![Ok(Vec::new())]);
        let mut t = CdcTransport::with_port(port);
        assert_eq!(
            t.transact(&[0x0C, 0x01, 0x02, 0x00], T),
            Err(Error::Disconnected)
        );
    }

    #[test]
    fn match_port_falls_back_to_the_only_power_z_port_without_a_serial() {
        // Windows often reports no serial for a composite device's COM port.
        let ports = vec![
            PortCandidate {
                name: "COM3".into(),
                vid: 0x1234,
                pid: 0x0001,
                serial: None,
            },
            PortCandidate {
                name: "COM7".into(),
                vid: 0x5FC9,
                pid: 0x0063,
                serial: None,
            },
        ];
        assert_eq!(
            match_port(&ports, "SERIAL-0002").map(|p| p.name.as_str()),
            Some("COM7")
        );
        let two = vec![
            ports[1].clone(),
            PortCandidate {
                name: "COM8".into(),
                ..ports[1].clone()
            },
        ];
        assert!(
            match_port(&two, "SERIAL-0002").is_none(),
            "ambiguous: never guess"
        );
    }
}
