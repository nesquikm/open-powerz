//! CDC serial transport on `serialport` for the binary protocol.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use super::{Error, GuardedTransport, Transport, map_io_error, without_port_names};
use crate::adcnew::{AdcnewCounters, CdcDemux, CdcItem, Frame, Request};
use crate::frame::{frame_type, msg};
use crate::pdm::reply::{ADCNEW, REPLY_TYPES, is_text};

pub use super::cdc_frame::{check_cdc, frame_complete};

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

/// Binary-protocol transport over the CDC data interface.
#[derive(Debug)]
pub struct CdcTransport<P> {
    port: P,
    /// Bytes read past the end of the last returned frame.
    pending: Vec<u8>,
    /// Splits the byte stream once an ADCNEW stream was started: from then
    /// on stream frames may sit between binary replies.
    demux: Option<CdcDemux>,
    /// Whether the ADCNEW stream runs (started and not stopped).
    streaming: bool,
    /// Stream frames read but not yet handed out.
    frames: Vec<Frame>,
    /// Stream frames dropped at the hold cap.
    overflowed: u64,
    /// The last start request, until the first bytes after it show whether
    /// the meter echoed it.
    start_sent: Option<Vec<u8>>,
    /// Bytes read since that start, while they may still be its echo.
    after_start: Vec<u8>,
    /// Start requests the meter echoed.
    start_echoes: u64,
}

/// Stream frames kept between polls before the oldest are dropped.
const MAX_HELD_FRAMES: usize = 4096;

impl<P: Read + Write> CdcTransport<P> {
    /// Wrap an already-open, already-cleared port.
    pub fn with_port(port: P) -> GuardedTransport<CdcTransport<P>> {
        GuardedTransport::new(CdcTransport {
            port,
            pending: Vec::new(),
            demux: None,
            streaming: false,
            frames: Vec::new(),
            overflowed: 0,
            start_sent: None,
            after_start: Vec::new(),
            start_echoes: 0,
        })
    }

    /// Write `bytes` and flush.
    fn write_request(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let os = std::env::consts::OS;
        self.port
            .write_all(bytes)
            .map_err(|e| map_io_error(&e, os))?;
        self.port.flush().map_err(|e| map_io_error(&e, os))
    }

    /// One port read into `chunk`: `Ok(None)` when nothing arrived in the
    /// port's own read timeout; end of stream means the port went away.
    fn read_chunk(&mut self, chunk: &mut [u8]) -> Result<Option<usize>, Error> {
        match self.port.read(chunk) {
            Ok(0) => Err(Error::Disconnected),
            Ok(n) => Ok(Some(n)),
            Err(e) => match e.kind() {
                io::ErrorKind::TimedOut
                | io::ErrorKind::WouldBlock
                | io::ErrorKind::Interrupted => Ok(None),
                _ => Err(map_io_error(&e, std::env::consts::OS)),
            },
        }
    }

    /// Hand stream bytes to the demux. Right after a start request they are
    /// held until [`settle_start`](Self::settle_start) decides whether they
    /// begin with an echo of it: right after a bulk session the meter sends
    /// the start back instead of streaming `[device]`.
    fn push_stream(&mut self, bytes: &[u8]) {
        if self.start_sent.is_some() {
            self.after_start.extend_from_slice(bytes);
        } else if let Some(demux) = self.demux.as_mut() {
            demux.push(bytes);
        }
    }

    /// Decide on the bytes held since the last start, once a read found no
    /// more: an echo is the start request alone, or the start followed by
    /// bytes that are not a valid frame (a 50/s ADC+PD start, `02 12`,
    /// begins like every 50/s record frame). Too few bytes to tell wait for
    /// the next poll.
    fn settle_start(&mut self) {
        let Some(start) = self.start_sent.as_ref() else {
            return;
        };
        let held = &self.after_start;
        let echoed = if held.len() <= start.len() {
            if start.starts_with(held) && held.len() < start.len() {
                return;
            }
            held == start
        } else if held.starts_with(start) {
            match frame_at_start(held) {
                // Not enough bytes yet to check the frame.
                None => return,
                Some(is_frame) => !is_frame,
            }
        } else {
            false
        };
        let mut held = std::mem::take(&mut self.after_start);
        if echoed {
            self.start_echoes += 1;
            held.drain(..start.len());
        }
        self.start_sent = None;
        if let Some(demux) = self.demux.as_mut() {
            demux.push(&held);
        }
    }

    /// Take every complete item out of the demux: stream frames are kept
    /// (while streaming), text lines dropped; the first binary reply that
    /// `want` accepts is returned, other replies are dropped.
    fn drain_demux(&mut self, want: Option<ReplyFor>) -> Option<Vec<u8>> {
        let demux = self.demux.as_mut()?;
        while let Some(item) = demux.next_item() {
            match item {
                Ok(CdcItem::Frame(frame)) if self.streaming => {
                    // Callers that never poll must not grow this forever.
                    if self.frames.len() >= MAX_HELD_FRAMES {
                        self.frames.remove(0);
                        self.overflowed += 1;
                    }
                    self.frames.push(frame);
                }
                Ok(CdcItem::Reply(bytes)) if want.is_some_and(|w| w.accepts(&bytes)) => {
                    return Some(bytes);
                }
                _ => {}
            }
        }
        None
    }

    /// Write `request` and read through the demux until its binary reply
    /// arrives (see [`ReplyFor`]), keeping the stream frames read on the way
    /// and dropping text lines and replies to other requests.
    fn transact_demuxed(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        // Replies still buffered answer an earlier request that timed out.
        self.drain_demux(None);
        let want = ReplyFor::request(request);
        self.write_request(request)?;
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; READ_CHUNK];
        loop {
            if let Some(reply) = self.drain_demux(Some(want)) {
                return Ok(reply);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            if let Some(n) = self.read_chunk(&mut chunk)?
                && let Some(demux) = self.demux.as_mut()
            {
                demux.push(&chunk[..n]);
            }
        }
    }

    /// Read the byte stream through the demux from now on (bytes already
    /// buffered go in first).
    fn start_demux(&mut self) {
        if self.demux.is_none() {
            let mut demux = CdcDemux::new();
            demux.push(&std::mem::take(&mut self.pending));
            self.demux = Some(demux);
        }
    }

    /// Read until the reply `want` accepts is complete or `timeout` passes.
    /// Stream frames, an echoed start, text lines and replies to other
    /// requests in front of it are skipped; bytes after it stay buffered
    /// for a follow-up [`Transport::read`].
    fn read_frame(&mut self, want: ReplyFor, timeout: Duration) -> Result<Vec<u8>, Error> {
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; READ_CHUNK];
        loop {
            loop {
                match plain_head(&self.pending, want) {
                    Head::Reply(len) => {
                        let rest = self.pending.split_off(len);
                        return Ok(std::mem::replace(&mut self.pending, rest));
                    }
                    Head::Skip(len) => {
                        self.pending.drain(..len.min(self.pending.len()));
                    }
                    Head::NeedMore => break,
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            if let Some(n) = self.read_chunk(&mut chunk)? {
                self.pending.extend_from_slice(&chunk[..n]);
            }
        }
    }
}

/// Which demuxed reply answers a request: the one with its id, except for
/// authentication, whose reply carries id 0 \[device\] and is taken by type.
#[derive(Debug, Clone, Copy)]
enum ReplyFor {
    Id(u8),
    Auth,
}

impl ReplyFor {
    fn request(request: &[u8]) -> ReplyFor {
        match frame_type(request) {
            Some(msg::AUTH_REPLY) => ReplyFor::Auth,
            _ => ReplyFor::Id(request.get(1).copied().unwrap_or(0)),
        }
    }

    fn accepts(self, reply: &[u8]) -> bool {
        match self {
            ReplyFor::Id(id) => reply.get(1) == Some(&id),
            ReplyFor::Auth => frame_type(reply) == Some(msg::AUTH_REPLY),
        }
    }
}

/// Bytes requested per serial read.
const READ_CHUNK: usize = 4096;
/// Per-read timeout on the real port; callers loop to their own deadline.
const PORT_READ_TIMEOUT: Duration = Duration::from_millis(20);
/// Baud rate the vendor tools use (CDC ignores it).
const BAUD: u32 = 2_000_000;

/// The USB serial ports the OS reports.
pub(crate) fn usb_ports() -> Result<Vec<PortCandidate>, serialport::Error> {
    Ok(without_tty_twins(usb_port_list()?))
}

/// Drop every `/dev/tty.X` whose `/dev/cu.X` twin is also listed: macOS
/// lists each USB serial port under both names \[device\], and the call-out
/// `cu.` device is the one to open (`tty.` waits for carrier detect).
pub fn without_tty_twins(ports: Vec<PortCandidate>) -> Vec<PortCandidate> {
    let cu: Vec<String> = ports
        .iter()
        .filter_map(|p| p.name.strip_prefix("/dev/cu.").map(str::to_string))
        .collect();
    ports
        .into_iter()
        .filter(|p| {
            p.name
                .strip_prefix("/dev/tty.")
                .is_none_or(|rest| !cu.iter().any(|c| c == rest))
        })
        .collect()
}

fn usb_port_list() -> Result<Vec<PortCandidate>, serialport::Error> {
    Ok(serialport::available_ports()?
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
        .collect())
}

/// Open port `name` at 2 Mbaud 8N1, no flow control, a short read timeout,
/// with its input buffer cleared.
pub(crate) fn open_serial(
    name: String,
) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
    let port = serialport::new(name, BAUD)
        .data_bits(serialport::DataBits::Eight)
        .parity(serialport::Parity::None)
        .stop_bits(serialport::StopBits::One)
        .flow_control(serialport::FlowControl::None)
        .timeout(PORT_READ_TIMEOUT)
        .open()?;
    port.clear(serialport::ClearBuffer::Input)?;
    Ok(port)
}

/// The OS text [`port_gone`] reads as a port gone.
const NOT_FOUND_TEXT: &str = "The system cannot find the file specified.";

/// `e` from opening port `name`, as a port gone when it answered `NoDevice`
/// and has left `ports`, a fresh listing. Windows words its errors in the
/// system's language, so [`port_gone`] cannot read them there. A listing that
/// failed proves nothing.
pub(crate) fn gone_if_unlisted<E>(
    e: serialport::Error,
    name: &str,
    ports: Result<Vec<PortCandidate>, E>,
) -> serialport::Error {
    let left = ports.is_ok_and(|ports| ports.iter().all(|port| port.name != name));
    if left {
        serialport::Error::new(serialport::ErrorKind::NoDevice, NOT_FOUND_TEXT)
    } else {
        e
    }
}

/// `open_serial` for a port just listed, with the failure read through
/// [`gone_if_unlisted`].
pub(crate) fn open_listed_serial(
    name: String,
) -> Result<Box<dyn serialport::SerialPort>, serialport::Error> {
    open_serial(name.clone()).map_err(|e| {
        if e.kind == serialport::ErrorKind::NoDevice {
            gone_if_unlisted(e, &name, usb_ports())
        } else {
            e
        }
    })
}

/// Whether `e`, from opening a port, says the port itself has gone.
/// `serialport` keeps no code for these, only the OS text: on Windows
/// ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND share kind `NoDevice` with a
/// held port, and ENXIO and ENODEV come as `Unknown` with nix's text.
pub(crate) fn port_gone(e: &serialport::Error) -> bool {
    let text = e.description.as_str();
    match e.kind {
        serialport::ErrorKind::NoDevice => matches!(
            text,
            NOT_FOUND_TEXT | "The system cannot find the path specified."
        ),
        serialport::ErrorKind::Unknown => {
            matches!(text, "No such device or address" | "No such device")
        }
        _ => false,
    }
}

/// Whether `e`, from opening a port discovery just listed, means another
/// program holds it. `serialport` 4.10.1 reports a held port (EBUSY on unix,
/// ERROR_ACCESS_DENIED on Windows) as kind `NoDevice`, the kind of a missing
/// one; a port just listed is not missing, unless [`port_gone`] says so.
pub(crate) fn port_held(e: &serialport::Error) -> bool {
    e.kind == serialport::ErrorKind::NoDevice && !port_gone(e)
}

/// Open the port in `ports` that [`match_port`] picks for `serial`, through
/// `open`. A port someone else holds is [`Error::PortHeld`] (see
/// `port_held`), no matching port is [`Error::NotFound`].
pub fn open_listed<P>(
    ports: &[PortCandidate],
    serial: &str,
    open: impl FnOnce(String) -> Result<P, serialport::Error>,
) -> Result<P, Error> {
    let name = match_port(ports, serial)
        .ok_or(Error::NotFound)?
        .name
        .clone();
    open(name).map_err(|e| {
        if port_held(&e) {
            Error::PortHeld
        } else {
            serial_error(e)
        }
    })
}

/// The CDC transport on an OS serial port, as [`CdcTransport::open`] opens it.
pub type SerialCdcTransport = CdcTransport<Box<dyn serialport::SerialPort>>;

impl SerialCdcTransport {
    /// Open the meter's port by serial number, clear input and wait
    /// [`FIRST_FRAME_DELAY`].
    pub fn open(serial: &str) -> Result<GuardedTransport<Self>, Error> {
        let ports = usb_ports().map_err(serial_error)?;
        let port = open_listed(&ports, serial, open_listed_serial)?;
        std::thread::sleep(FIRST_FRAME_DELAY);
        Ok(CdcTransport::with_port(port))
    }
}

fn serial_error(e: serialport::Error) -> Error {
    if port_gone(&e) {
        return Error::Disconnected;
    }
    match e.kind {
        serialport::ErrorKind::NoDevice => Error::NotFound,
        serialport::ErrorKind::Io(kind) => {
            map_io_error(&io::Error::new(kind, e.description), std::env::consts::OS)
        }
        _ => Error::Io(without_port_names(&e.description)),
    }
}

impl<P: Read + Write> Transport for CdcTransport<P> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        check_cdc(request)?;
        if self.demux.is_some() {
            return self.transact_demuxed(request, timeout);
        }
        // One request in flight: anything still buffered belongs to an
        // earlier reply that timed out, so it must not prefix this one.
        self.pending.clear();
        let want = ReplyFor::request(request);
        self.write_request(request)?;
        self.read_frame(want, timeout)
    }

    /// Follow-up transfers (memory-read data) carry no header, so they are
    /// returned raw: whatever is buffered, else the next chunk that arrives.
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        // Headerless data (a memory read's payload) cannot be told apart
        // from stream frames; memory reads get no reply on CDC anyway.
        if self.streaming {
            return Err(Error::UnsupportedOnCdc { ty: 0x44 });
        }
        if !self.pending.is_empty() {
            return Ok(std::mem::take(&mut self.pending));
        }
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; READ_CHUNK];
        while Instant::now() < deadline {
            if let Some(n) = self.read_chunk(&mut chunk)? {
                return Ok(chunk[..n].to_vec());
            }
        }
        Err(Error::Timeout)
    }

    /// The settings write and everything after it go through the demux: the
    /// meter may answer it late or not at all \[static\], so its reply, a
    /// stream frame or a text line must never pass for the reply to the
    /// next request (the read-back), which is matched by id.
    fn transact_settings(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        super::guard::check_settings_write(request)?;
        check_cdc(request)?;
        self.start_demux();
        self.transact_demuxed(request, timeout)
    }

    fn adcnew_send(&mut self, request: Request) -> Result<(), Error> {
        self.start_demux();
        let bytes = request.encode();
        self.write_request(&bytes)?;
        self.after_start.clear();
        self.start_sent = None;
        match request {
            Request::Stop => {
                self.streaming = false;
                self.frames.clear();
            }
            // Only `02 <att>` is checked for an echo: a bare `02` could not be
            // told from the first byte of a frame.
            Request::Start { .. } => {
                self.streaming = true;
                self.start_sent = Some(bytes);
            }
            Request::StartDefault | Request::KeepAlive => {
                self.streaming = true;
            }
        }
        Ok(())
    }

    /// Reads until the port has nothing more buffered (at most
    /// `MAX_STREAM_READS` reads, so a fast stream cannot hold it forever).
    fn adcnew_frames(&mut self) -> Result<Vec<Frame>, Error> {
        if self.demux.is_none() {
            return Ok(Vec::new());
        }
        let mut chunk = [0u8; READ_CHUNK];
        for _ in 0..MAX_STREAM_READS {
            let Some(n) = self.read_chunk(&mut chunk)? else {
                break;
            };
            self.push_stream(&chunk[..n]);
        }
        self.settle_start();
        self.drain_demux(None);
        Ok(std::mem::take(&mut self.frames))
    }

    fn adcnew_counters(&self) -> AdcnewCounters {
        AdcnewCounters {
            crc_errors: self.demux.as_ref().map_or(0, |d| d.dropped_frames() as u64),
            dropped_frames: self.overflowed,
            start_echoes: self.start_echoes,
        }
    }
}

/// Whether `bytes` begin with a whole, valid ADCNEW frame (length and
/// CRC); `None` while too few bytes are there to tell.
fn frame_at_start(bytes: &[u8]) -> Option<bool> {
    let obj = usize::from(*bytes.get(2)?);
    let len = 4 + obj * 4;
    if bytes.len() < len.max(4) {
        return None;
    }
    Some(obj > 0 && crate::adcnew::decode_frame(&bytes[..len]).is_ok())
}

/// What the buffer of a plain request starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Head {
    /// The awaited reply, this many bytes long.
    Reply(usize),
    /// Bytes that are not the reply: drop them.
    Skip(usize),
    /// Too few bytes to tell.
    NeedMore,
}

/// The binary reply (known type, whole) at the start of `buf`, by length.
fn reply_at(buf: &[u8]) -> Option<usize> {
    let &first = buf.first()?;
    if !REPLY_TYPES.contains(&(first & 0x7F)) {
        return None;
    }
    frame_complete(buf).map(|len| len.max(1))
}

/// Whether `buf` starts with a whole item: a valid ADCNEW frame, a text
/// line or a binary reply.
fn item_at(buf: &[u8]) -> bool {
    match buf.first() {
        Some(&ADCNEW) => frame_at_start(buf) == Some(true),
        Some(_) => text_line_at(buf).is_some_and(|l| l.is_some()) || reply_at(buf).is_some(),
        None => false,
    }
}

/// A text line at the start of `buf`: `Some(Some(len))` with its line end,
/// `Some(None)` while it may still be one, `None` when it is not one.
fn text_line_at(buf: &[u8]) -> Option<Option<usize>> {
    for (i, &b) in buf.iter().enumerate() {
        match b {
            b'\n' if i > 0 => return Some(Some(i + 1)),
            b if is_text(b) => {}
            _ => return None,
        }
    }
    Some(None)
}

/// Classify the start of a plain request's buffer: the reply `want`
/// accepts, or what to skip in front of it \[device\]: ADCNEW frames, an
/// echoed stream start (`02 <att>`), text lines and replies to other ids
/// arrive there even when this session started no stream.
fn plain_head(buf: &[u8], want: ReplyFor) -> Head {
    let Some(&first) = buf.first() else {
        return Head::NeedMore;
    };
    if first == ADCNEW {
        if buf.len() < 2 {
            return Head::NeedMore;
        }
        return match frame_at_start(buf) {
            Some(true) => Head::Skip(4 + usize::from(buf[2]) * 4),
            // A start echo: what follows it is a whole item.
            _ if item_at(&buf[2..]) => Head::Skip(2),
            Some(false) => Head::Skip(1),
            None => Head::NeedMore,
        };
    }
    let reply = reply_at(buf);
    if let Some(len) = reply
        && want.accepts(&buf[..len])
    {
        return Head::Reply(len);
    }
    match text_line_at(buf) {
        Some(Some(len)) => return Head::Skip(len),
        Some(None) if first != b'\n' => return Head::NeedMore,
        _ => {}
    }
    match reply {
        // A whole reply to another request.
        Some(len) => Head::Skip(len),
        None if REPLY_TYPES.contains(&(first & 0x7F)) => Head::NeedMore,
        // Neither text nor a known frame: noise.
        None => Head::Skip(1),
    }
}

/// Port reads per [`Transport::adcnew_frames`] call at most.
const MAX_STREAM_READS: usize = 64;

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

    // ---- AC-PR41FT.1: macOS lists each port as cu.* and tty.* ----

    #[test]
    fn the_tty_twin_of_a_cu_port_is_dropped() {
        let port = |name: &str| PortCandidate {
            name: name.into(),
            vid: 0x5FC9,
            pid: 0x0063,
            serial: Some("SERIAL-0001".into()),
        };
        let listed = vec![
            port("/dev/tty.usbmodem1101"),
            port("/dev/cu.usbmodem1101"),
            port("/dev/tty.other"),
            port("COM5"),
        ];
        let names: Vec<String> = without_tty_twins(listed)
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["/dev/cu.usbmodem1101", "/dev/tty.other", "COM5"]);
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
    fn a_headerless_read_is_refused_while_streaming() {
        let port = FakePort::with_chunks(vec![Ok(vec![0x5A; 16])]);
        let mut t = CdcTransport::with_port(port);
        t.adcnew_send(Request::StartDefault).unwrap();
        assert_eq!(
            t.read(Duration::from_millis(50)),
            Err(Error::UnsupportedOnCdc { ty: 0x44 })
        );
    }

    #[test]
    fn frames_nobody_polls_are_capped_to_the_newest() {
        let frame = |n: u32| {
            let mut body = n.to_le_bytes().to_vec();
            body.extend([0u8; 16]);
            let mut f = vec![0x02, 0x12, 0x05, crate::adcnew::crc8(&body)];
            f.extend(body);
            f
        };
        let bytes: Vec<u8> = (0..MAX_HELD_FRAMES as u32 + 3).flat_map(frame).collect();
        let port = FakePort::with_chunks(vec![Ok(bytes)]);
        let mut t = CdcTransport::with_port(port);
        t.adcnew_send(Request::StartDefault).unwrap();
        let frames = t.adcnew_frames().unwrap();
        assert_eq!(frames.len(), MAX_HELD_FRAMES);
        match &frames[0] {
            Frame::Records { records, .. } => assert_eq!(records[0].time_ms(), 3),
            other => panic!("{other:?}"),
        }
    }

    /// A one-record ADCNEW frame (att 0x12) with device time `n` ms.
    fn record_frame(n: u32) -> Vec<u8> {
        let mut body = n.to_le_bytes().to_vec();
        body.extend([0u8; 16]);
        let mut f = vec![0x02, 0x12, 0x05, crate::adcnew::crc8(&body)];
        f.extend(body);
        f
    }

    #[test]
    fn frames_dropped_for_the_cap_and_for_a_bad_crc_are_counted() {
        let mut bad = record_frame(1);
        bad[3] ^= 0xFF;
        let mut bytes = record_frame(0);
        bytes.extend(bad);
        bytes.extend((2..MAX_HELD_FRAMES as u32 + 4).flat_map(record_frame));
        let port = FakePort::with_chunks(vec![Ok(bytes)]);
        let mut t = CdcTransport::with_port(port);
        assert_eq!(t.adcnew_counters(), AdcnewCounters::default());
        t.adcnew_send(Request::StartDefault).unwrap();
        let frames = t.adcnew_frames().unwrap();
        assert_eq!(frames.len(), MAX_HELD_FRAMES);
        assert_eq!(
            t.adcnew_counters(),
            AdcnewCounters {
                crc_errors: 1,
                dropped_frames: 3,
                start_echoes: 0,
            }
        );
    }

    // ---- JKWT3W: the settings write goes over CDC ----

    const SETTINGS_188: &str = include_str!("../../tests/vectors/settings_188.hex");

    /// A type-0x0A frame with the documented keys, `next` on all but the last.
    fn settings_write(id: u8) -> Vec<u8> {
        let entries: [(u16, u32); 8] = [
            (0x01, 1),
            (0x03, 20),
            (0x04, 0),
            (0x06, 0),
            (0x07, 1),
            (0x14, 2),
            (0x15, 1),
            (0x7FFF, 0x1234),
        ];
        let mut f = (0x0Au32 | (u32::from(id) << 8) | (8 << 23))
            .to_le_bytes()
            .to_vec();
        for (i, &(key, value)) in entries.iter().enumerate() {
            let next = if i + 1 < entries.len() { 1u32 << 15 } else { 0 };
            f.extend((0x0100_0000u32 | next | u32::from(key)).to_le_bytes());
            f.extend(value.to_le_bytes());
        }
        f
    }

    #[test]
    fn a_settings_write_skips_stream_frames_text_and_other_ids_for_its_reply() {
        let mut bytes = record_frame(1);
        bytes.extend(b"ok\r\n");
        bytes.extend([0x05, 0x09, 0x00, 0x00]); // a reply to another request
        bytes.extend([0x07, 0x03, 0x00, 0x00]);
        let port = FakePort::with_chunks(vec![Ok(bytes)]);
        let mut t = CdcTransport::with_port(port.clone());
        let f = settings_write(3);
        assert_eq!(t.transact_settings(&f, T), Ok(vec![0x07, 0x03, 0x00, 0x00]));
        assert_eq!(port.written(), f, "only the write is sent");
    }

    #[test]
    fn the_read_back_after_a_settings_write_is_matched_by_id() {
        let port = FakePort::with_chunks(vec![]);
        let mut t = CdcTransport::with_port(port.clone());
        // No reply to the write in time.
        assert_eq!(
            t.transact_settings(&settings_write(3), Duration::from_millis(30)),
            Err(Error::Timeout)
        );
        let mut settings = hex_vector(SETTINGS_188);
        settings[1] = 0x04;
        let mut late = vec![0x07, 0x03, 0x00, 0x00]; // the write's late reply
        late.extend(record_frame(2));
        late.extend(b"ok\r\n");
        port.0.borrow_mut().chunks.push_back(Ok(late));
        port.0.borrow_mut().chunks.push_back(Ok(settings.clone()));
        assert_eq!(t.transact(&[0x0C, 0x04, 0x10, 0x00], T), Ok(settings));
    }

    /// The start request for ADC at 50/s, as the meter echoes it right after
    /// a bulk session (2026-10-28 probe on the meter: `02 02` in, `02 02`
    /// back, no stream). [device]
    const ECHOED_START: [u8; 2] = [0x02, 0x02];

    #[test]
    fn an_echoed_start_is_dropped_and_counted_not_read_as_a_frame() {
        let mut bytes = ECHOED_START.to_vec();
        bytes.extend(record_frame(20));
        let port = FakePort::with_chunks(vec![Ok(bytes)]);
        let mut t = CdcTransport::with_port(port.clone());
        t.adcnew_send(Request::Start {
            content: crate::adcnew::Content::Adc,
            rate: crate::decode::Rate::Sps50,
        })
        .unwrap();
        assert_eq!(
            port.written(),
            ECHOED_START.to_vec(),
            "the real request bytes"
        );
        let frames = t.adcnew_frames().unwrap();
        assert_eq!(frames.len(), 1, "{frames:?}");
        let c = t.adcnew_counters();
        assert_eq!((c.start_echoes, c.crc_errors), (1, 0));
    }

    #[test]
    fn an_echo_split_across_reads_is_still_recognised() {
        let mut rest = vec![ECHOED_START[1]];
        rest.extend(record_frame(20));
        let port = FakePort::with_chunks(vec![Ok(vec![ECHOED_START[0]]), Ok(rest)]);
        let mut t = CdcTransport::with_port(port);
        t.adcnew_send(Request::Start {
            content: crate::adcnew::Content::Adc,
            rate: crate::decode::Rate::Sps50,
        })
        .unwrap();
        assert_eq!(t.adcnew_frames().unwrap().len(), 1);
        assert_eq!(t.adcnew_counters().start_echoes, 1);
    }

    #[test]
    fn a_frame_that_begins_like_the_start_is_not_an_echo() {
        // ADC + PD at 50/s is `02 12`, the first two bytes of every 50/s
        // record frame (`02 12 05 crc`).
        let port = FakePort::with_chunks(vec![Ok(record_frame(20))]);
        let mut t = CdcTransport::with_port(port.clone());
        t.adcnew_send(Request::Start {
            content: crate::adcnew::Content::AdcPd,
            rate: crate::decode::Rate::Sps50,
        })
        .unwrap();
        assert_eq!(port.written(), vec![0x02, 0x12]);
        assert_eq!(t.adcnew_frames().unwrap().len(), 1);
        assert_eq!(t.adcnew_counters(), AdcnewCounters::default());
    }

    #[test]
    fn an_echo_with_nothing_after_it_is_counted() {
        let port = FakePort::with_chunks(vec![Ok(ECHOED_START.to_vec())]);
        let mut t = CdcTransport::with_port(port);
        t.adcnew_send(Request::Start {
            content: crate::adcnew::Content::Adc,
            rate: crate::decode::Rate::Sps50,
        })
        .unwrap();
        assert!(t.adcnew_frames().unwrap().is_empty());
        let c = t.adcnew_counters();
        assert_eq!((c.start_echoes, c.crc_errors), (1, 0));
    }

    #[test]
    fn frames_right_after_a_start_are_not_taken_for_an_echo() {
        let port = FakePort::with_chunks(vec![Ok(record_frame(20))]);
        let mut t = CdcTransport::with_port(port);
        t.adcnew_send(Request::Start {
            content: crate::adcnew::Content::Adc,
            rate: crate::decode::Rate::Sps50,
        })
        .unwrap();
        assert_eq!(t.adcnew_frames().unwrap().len(), 1);
        assert_eq!(t.adcnew_counters(), AdcnewCounters::default());
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

    // AC-4PJRVM.7: `serialport` 4.10.1 reports a port someone else holds as
    // kind `NoDevice` (EBUSY in posix/error.rs, a failed flock in
    // posix/flock.rs, ERROR_ACCESS_DENIED in windows/error.rs), so the kind
    // alone cannot tell it from a missing port: what can is that discovery
    // listed the port a moment ago.

    fn held() -> serialport::Error {
        serialport::Error::new(serialport::ErrorKind::NoDevice, "Resource busy")
    }

    fn meter_port(name: &str, serial: &str) -> PortCandidate {
        PortCandidate {
            name: name.into(),
            vid: crate::VID,
            pid: 0x0001,
            serial: Some(serial.into()),
        }
    }

    #[test]
    fn a_port_gone_from_a_fresh_listing_is_not_held_in_any_language() {
        // Windows words a missing port in the system's language.
        let german = || {
            serialport::Error::new(
                serialport::ErrorKind::NoDevice,
                "Das System kann die angegebene Datei nicht finden.",
            )
        };
        let name = "COM7";
        let empty: Result<Vec<PortCandidate>, ()> = Ok(Vec::new());
        assert!(port_gone(&gone_if_unlisted(german(), name, empty)));

        let listed: Result<_, ()> = Ok(vec![meter_port(name, "KM1")]);
        assert!(port_held(&gone_if_unlisted(german(), name, listed)));

        let unlistable: Result<Vec<PortCandidate>, ()> = Err(());
        assert!(port_held(&gone_if_unlisted(german(), name, unlistable)));
    }

    #[test]
    fn no_device_from_opening_a_listed_port_means_it_is_held() {
        let ports = [meter_port("/dev/cu.usbmodem1101", "KM1")];
        let mut opened = None;
        let result = open_listed(&ports, "KM1", |name| {
            opened = Some(name);
            Err::<(), _>(held())
        });
        assert_eq!(opened.as_deref(), Some("/dev/cu.usbmodem1101"));
        assert!(matches!(result, Err(Error::PortHeld)), "{result:?}");
    }

    // ---- AC-AXHK2J.7: a vanished port is not a held one ----

    fn open_failing(kind: serialport::ErrorKind, text: &str) -> Result<(), Error> {
        let ports = [meter_port("/dev/cu.usbmodem1101", "KM1")];
        open_listed(&ports, "KM1", |_| {
            Err::<(), _>(serialport::Error::new(kind, text))
        })
    }

    #[test]
    fn every_way_serialport_says_held_stays_held() {
        for text in [
            "Resource busy",
            "Device or resource busy",
            "Unable to acquire exclusive lock on serial port",
            "Unable to acquire shared lock on serial port",
            "Access is denied.",
        ] {
            let result = open_failing(serialport::ErrorKind::NoDevice, text);
            assert!(matches!(result, Err(Error::PortHeld)), "{text}: {result:?}");
        }
    }

    #[test]
    fn a_port_that_vanished_before_the_open_is_not_held() {
        // ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND share kind `NoDevice`
        // with a held port on Windows (windows/error.rs).
        for text in [
            "The system cannot find the file specified.",
            "The system cannot find the path specified.",
        ] {
            let result = open_failing(serialport::ErrorKind::NoDevice, text);
            assert_eq!(result, Err(Error::Disconnected), "{text}");
        }
        // ENOENT on unix.
        let result = open_failing(
            serialport::ErrorKind::Io(io::ErrorKind::NotFound),
            "No such file or directory",
        );
        assert_eq!(result, Err(Error::Disconnected));
    }

    // ---- AC-AXHK2J.5: ENXIO and ENODEV at open mean the meter is gone ----

    #[test]
    fn enxio_and_enodev_at_open_are_the_meter_gone() {
        // serialport has no kind for them (posix/error.rs): both are
        // `Unknown` with nix's text.
        for text in ["No such device or address", "No such device"] {
            let result = open_failing(serialport::ErrorKind::Unknown, text);
            assert_eq!(result, Err(Error::Disconnected), "{text}");
        }
    }

    #[test]
    fn another_unknown_open_failure_stays_an_io_error() {
        let result = open_failing(serialport::ErrorKind::Unknown, "Input/output error");
        assert!(matches!(result, Err(Error::Io(_))), "{result:?}");
    }

    #[test]
    fn no_device_with_no_listed_port_stays_not_found_and_opens_nothing() {
        let other_meter = [meter_port("/dev/cu.usbmodem1101", "KM2"), {
            let mut twin = meter_port("/dev/cu.usbmodem1102", "KM3");
            twin.vid = 0x1234;
            twin
        }];
        for ports in [&other_meter[..], &[]] {
            let result = open_listed(ports, "KM1", |_| -> Result<(), _> {
                panic!("no port matched, nothing to open")
            });
            assert_eq!(result, Err(Error::NotFound));
        }
    }

    #[test]
    fn no_device_from_discovery_stays_not_found() {
        assert_eq!(serial_error(held()), Error::NotFound);
    }

    #[test]
    fn other_open_failures_of_a_listed_port_keep_their_own_mapping() {
        let ports = [meter_port("/dev/cu.usbmodem1101", "KM1")];
        let refused = serialport::Error::new(
            serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied),
            "Permission denied",
        );
        let result = open_listed(&ports, "KM1", |_| Err::<(), _>(refused));
        assert!(matches!(result, Err(Error::Permission(_))), "{result:?}");
        let gone = serialport::Error::new(serialport::ErrorKind::Unknown, "odd");
        let result = open_listed(&ports, "KM1", |_| Err::<(), _>(gone));
        assert_eq!(result, Err(Error::Io("odd".into())));
    }

    #[test]
    fn a_listed_port_that_opens_is_handed_back() {
        let ports = [meter_port("/dev/cu.usbmodem1101", "KM1")];
        assert_eq!(open_listed(&ports, "KM1", |_| Ok(7)), Ok(7));
    }

    /// The whole chain on a real tty: discovery names a pty, this process
    /// already holds it, and the port opener the transport uses meets it.
    #[cfg(unix)]
    #[test]
    fn a_real_pty_already_open_comes_back_as_held() {
        use serialport::SerialPort as _;

        // Held through `set_exclusive`: on macOS `open` cannot finish on a
        // pty, its baud ioctl fails with ENOTTY after taking the lock.
        let (_master, mut holder) = serialport::TTYPort::pair().expect("a pty");
        let path = holder.name().expect("a pty slave has a path");
        holder.set_exclusive(true).expect("the first hold succeeds");

        let ports = [meter_port(&path, "KM1")];
        let result = open_listed(&ports, "KM1", open_serial).map(|_| ());
        assert!(matches!(result, Err(Error::PortHeld)), "{result:?}");
    }

    // ---- AC-3B70S8.1: a plain request's reply is the frame with its id ----

    /// The 52-byte ADC reply carrying request id `id`.
    fn adc_reply(id: u8) -> Vec<u8> {
        let mut f = hex_vector(ADC_52);
        f[1] = id;
        f
    }

    /// GET_DATA for the ADC section with request id `id`.
    fn get_adc(id: u8) -> [u8; 4] {
        [0x0C, id, 0x02, 0x00]
    }

    /// A plain `transact` (no ADCNEW stream started, no settings write)
    /// whose port first delivers `stray`, then the reply to id 5.
    fn reply_after(stray: &[u8]) -> Result<Vec<u8>, Error> {
        let mut bytes = stray.to_vec();
        bytes.extend(adc_reply(5));
        let port = FakePort::with_chunks(vec![Ok(bytes)]);
        let mut t = CdcTransport::with_port(port.clone());
        let r = t.transact(&get_adc(5), T);
        assert_eq!(
            port.written(),
            get_adc(5).to_vec(),
            "only the request is sent"
        );
        r
    }

    #[test]
    fn an_echoed_start_before_a_reply_is_skipped_without_a_stream() {
        assert_eq!(reply_after(&ECHOED_START), Ok(adc_reply(5)));
    }

    #[test]
    fn a_stream_frame_before_a_reply_is_skipped_without_a_stream() {
        assert_eq!(reply_after(&record_frame(7)), Ok(adc_reply(5)));
    }

    #[test]
    fn a_text_line_before_a_reply_is_skipped_without_a_stream() {
        assert_eq!(reply_after(b"ok\n"), Ok(adc_reply(5)));
    }

    #[test]
    fn a_reply_to_another_id_is_skipped_without_a_stream() {
        assert_eq!(reply_after(&[0x05, 0x09, 0x00, 0x00]), Ok(adc_reply(5)));
        assert_eq!(reply_after(&adc_reply(4)), Ok(adc_reply(5)));
    }

    #[test]
    fn mixed_stray_frames_split_across_reads_are_skipped_without_a_stream() {
        let mut bytes = ECHOED_START.to_vec();
        bytes.extend(record_frame(7));
        bytes.extend(b"ok\r\n");
        bytes.extend(ECHOED_START);
        bytes.extend([0x05, 0x09, 0x00, 0x00]);
        bytes.extend(record_frame(8));
        bytes.extend(adc_reply(4));
        bytes.extend(adc_reply(5));
        // Odd cuts: through the echo, a frame header, the text and the reply.
        let n = bytes.len();
        let cuts = [1, 3, 9, 15, 4, 6, 20, 33];
        let used: usize = cuts.iter().sum();
        let mut sizes = cuts.to_vec();
        sizes.push(n - used);
        let port = FakePort::with_chunks(split(&bytes, &sizes));
        let mut t = CdcTransport::with_port(port.clone());
        assert_eq!(t.transact(&get_adc(5), T), Ok(adc_reply(5)));
        assert_eq!(port.written(), get_adc(5).to_vec());
    }

    #[test]
    fn every_plain_request_gets_its_own_reply_with_stray_frames_between() {
        let port = FakePort::with_chunks(vec![]);
        let mut t = CdcTransport::with_port(port.clone());
        for id in 1..=4u8 {
            let mut bytes = ECHOED_START.to_vec();
            bytes.extend(record_frame(u32::from(id)));
            bytes.extend(b"ok\n");
            bytes.extend(adc_reply(id));
            // A stream frame right behind the reply, before the next request.
            bytes.extend(record_frame(100 + u32::from(id)));
            port.0.borrow_mut().chunks.push_back(Ok(bytes));
            assert_eq!(t.transact(&get_adc(id), T), Ok(adc_reply(id)), "id {id}");
        }
        // None of the stray frames got kept as stream frames: none was asked for.
        assert!(t.adcnew_frames().unwrap().is_empty());
    }

    #[test]
    fn a_stray_control_type_2_never_comes_back_as_the_reply() {
        // The bug: `02 02` plus the reply's first two bytes made a 4-byte
        // control frame of type 2 that `transact` handed back.
        let r = reply_after(&ECHOED_START).expect("the ADC reply");
        assert_eq!(r[0] & 0x7F, 0x41, "{r:02x?}");
        assert_eq!(r.len(), 52);
    }
}
