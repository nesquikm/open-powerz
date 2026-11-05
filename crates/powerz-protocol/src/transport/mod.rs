//! Device discovery and transports: the [`Transport`] trait, a scripted
//! [`mock::MockTransport`], vendor bulk ([`bulk`]), CDC serial ([`cdc`]) and
//! the restricted read-only HID fallback ([`hid`]). Every real transport is
//! wrapped in a [`guard::GuardedTransport`] that refuses dangerous frame
//! types before any byte leaves the host. See `docs/protocol.md`.

use std::time::Duration;

pub mod bulk;
pub mod cdc;
pub mod discovery;
pub mod guard;
#[cfg(feature = "hid")]
pub mod hid;
pub mod mock;
pub mod select;

pub use guard::GuardedTransport;
pub use mock::MockTransport;

/// Default time to wait for one reply.
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(1000);
/// Time to wait for the reply to CONNECT.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(2000);
/// Time to wait for the ACCEPT to the first CONNECT before the pipe is
/// recovered and CONNECT is retried.
pub const FIRST_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Errors from discovery and transports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// No reply arrived before the timeout expired.
    #[error("timed out waiting for the meter")]
    Timeout,
    /// The meter was unplugged or the pipe broke.
    #[error("meter disconnected")]
    Disconnected,
    /// The OS refused access to the device.
    #[error("permission denied: {0}")]
    Permission(String),
    /// The guard refused a frame type that could damage the meter.
    #[error("refusing to send frame type {ty:#04x}: it could damage the meter")]
    Forbidden {
        /// Frame type (bits 0..6 of the main header).
        ty: u8,
    },
    /// The request is not allowed over HID (only the ADC snapshot is).
    #[error("request not supported over HID (only the ADC snapshot is)")]
    UnsupportedOnHid,
    /// A HID reply declares more bytes than one 64-byte report holds.
    #[error("HID reply declares {declared} bytes, more than one 64-byte report")]
    HidReplyTooLong {
        /// Total bytes the reply headers declare.
        declared: usize,
    },
    /// The frame type means something else on CDC (ADCNEW stream control).
    #[error("frame type {ty:#04x} is not supported over CDC")]
    UnsupportedOnCdc {
        /// Frame type (bits 0..6 of the main header).
        ty: u8,
    },
    /// The settings write (type 0x0A) goes only over the CDC port: over
    /// bulk the KM003C ignored it (2026-10-29) \[device\]; nothing was sent.
    #[error("the settings write is only taken over the meter's CDC serial port")]
    SettingsNeedCdc,
    /// ADCNEW stream requests exist only on the CDC port; nothing was sent.
    #[error("the ADCNEW stream is only available over the CDC serial port")]
    AdcnewNeedsCdc,
    /// No matching device or port was found.
    #[error("no matching device found")]
    NotFound,
    /// Discovery listed the meter's serial port, but opening it failed the
    /// way a port someone else holds does.
    #[error("the meter's serial port is in use by another program")]
    PortHeld,
    /// The mock transport saw something its script did not expect.
    #[error("mock transport: {0}")]
    Mock(String),
    /// Any other I/O failure.
    #[error("I/O error: {0}")]
    Io(String),
}

/// One request in flight over a pipe to the meter.
pub trait Transport {
    /// Write `request` and return the next reply buffer.
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error>;

    /// Read one follow-up transfer without writing.
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error>;

    /// Write the record-table SET_DATA (attribute 0x28, the delete path) and
    /// return the reply. The only route by which a 0x48 frame may pass a
    /// [`GuardedTransport`], which checks it with
    /// [`check_record_table_write`](guard::check_record_table_write).
    /// Default: run that check, then [`transact`](Transport::transact).
    /// Wrappers that sit outside a guard must forward this method.
    fn transact_record_table(
        &mut self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        guard::check_record_table_write(request)?;
        self.transact(request, timeout)
    }

    /// Write the settings write (type 0x0A) and return the reply. The only
    /// route by which a 0x0A frame may pass a [`GuardedTransport`], which
    /// checks it with [`check_settings_write`](guard::check_settings_write).
    /// Default: run that check, then [`transact`](Transport::transact).
    /// Wrappers that sit outside a guard must forward this method.
    fn transact_settings(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        guard::check_settings_write(request)?;
        self.transact(request, timeout)
    }

    /// Write one startup-logo flash chunk (type 0x4A). `plain` is the
    /// plaintext frame; checked with
    /// [`check_logo_chunk`](guard::check_logo_chunk) before encryption.
    /// Wrappers that sit outside a guard must forward this method.
    fn transact_logo_chunk(&mut self, plain: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        guard::check_logo_chunk(plain)?;
        let (Some(header), Some(payload)) = (plain.get(..4), plain.get(4..)) else {
            return Err(Error::Forbidden {
                ty: guard::LOGO_CHUNK_TYPE,
            });
        };
        let mut frame = header.to_vec();
        frame.extend(crate::crypto::encrypt(&crate::crypto::KEYS[0], payload));
        self.transact(&frame, timeout)
    }

    /// Write the startup-logo flag (SET_DATA 0x48, attribute 0x1C).
    /// Wrappers that sit outside a guard must forward this method.
    fn transact_logo_flag(&mut self, frame: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        guard::check_logo_flag_write(frame)?;
        self.transact(frame, timeout)
    }

    /// Open the pipe on which this meter takes the settings write (type
    /// 0x0A): its CDC port, since over bulk the KM003C ignored the write
    /// (2026-10-29) \[device\]. `None` when this pipe has no such twin, or
    /// is that pipe itself. Wrappers that sit outside a guard must forward
    /// this method. Default: `None`.
    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, Error>> {
        None
    }

    /// Bring a pipe left mid-transfer back to a clean state (e.g. clear a
    /// halted endpoint, drain stale data). Called once by
    /// [`Session::connect`](crate::session::Session::connect) when CONNECT
    /// gets no matching ACCEPT. Sends no device command. Default: nothing.
    fn recover(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Write an ADCNEW stream request (start, keep-alive, stop). Only the
    /// CDC transport sends these; the default refuses without writing.
    fn adcnew_send(&mut self, request: crate::adcnew::Request) -> Result<(), Error> {
        let _ = request;
        Err(Error::AdcnewNeedsCdc)
    }

    /// Read what the port has buffered and return the ADCNEW frames
    /// received since the last call, in stream order. Only the CDC
    /// transport streams; the default refuses.
    fn adcnew_frames(&mut self) -> Result<Vec<crate::adcnew::Frame>, Error> {
        Err(Error::AdcnewNeedsCdc)
    }

    /// Frames of the ADCNEW stream lost on the host side so far. Only the
    /// CDC transport streams; the default counts nothing.
    fn adcnew_counters(&self) -> crate::adcnew::AdcnewCounters {
        crate::adcnew::AdcnewCounters::default()
    }
}

/// Default reply timeout for `request`: [`CONNECT_TIMEOUT`] for CONNECT,
/// [`REPLY_TIMEOUT`] for everything else.
pub fn default_timeout(request: &[u8]) -> Duration {
    match crate::frame::frame_type(request) {
        Some(crate::frame::msg::CONNECT) => CONNECT_TIMEOUT,
        _ => REPLY_TIMEOUT,
    }
}

/// Build an [`Error::Permission`] for `target_os` (e.g. `"linux"`); on Linux
/// the message names the udev rule `packaging/linux/60-powerz.rules`. A
/// serial port's name in `detail` is dropped, see `without_port_names`.
pub fn permission_error(target_os: &str, detail: &str) -> Error {
    let detail = without_port_names(detail);
    if target_os == "linux" {
        Error::Permission(format!(
            "{detail}; install the udev rule packaging/linux/60-powerz.rules \
             into /etc/udev/rules.d/ and replug the meter"
        ))
    } else {
        Error::Permission(detail)
    }
}

/// `text` with each serial port name or path in it replaced by "the meter's
/// serial port". On macOS a port's name holds the meter's serial, and OS and
/// library texts may quote it, so no such text reaches an error unread.
pub(crate) fn without_port_names(text: &str) -> String {
    let is_edge = |c: char| c.is_whitespace() || "\"'`()[]<>,;:.=".contains(c);
    text.split_inclusive(|c: char| c.is_whitespace() || c == '=' || c == ':')
        .map(|word| {
            let name = word.trim_matches(is_edge);
            if names_a_port(name) {
                word.replacen(name, "the meter's serial port", 1)
            } else {
                word.to_owned()
            }
        })
        .collect()
}

/// Whether `word` looks like a serial port's name or path on some OS. USB bus
/// and hidraw paths are not serial ports and carry no serial, and a folder
/// that is merely called `dev` is no device path.
fn names_a_port(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    if word.contains("/dev/bus/") || word.contains("/dev/hidraw") {
        return false;
    }
    let com_number = word
        .strip_prefix("com")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    com_number
        || word.starts_with("/dev/")
        || ["usbmodem", "usbserial", r"\\.\"]
            .iter()
            .any(|part| word.contains(part))
        || word.starts_with("cu.")
        || (word.starts_with("tty") && word.len() > 3)
}

/// Map an I/O error to a transport [`Error`].
pub fn map_io_error(err: &std::io::Error, target_os: &str) -> Error {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Error::Timeout,
        ErrorKind::BrokenPipe
        | ErrorKind::NotConnected
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::UnexpectedEof
        | ErrorKind::NotFound => Error::Disconnected,
        ErrorKind::PermissionDenied => permission_error(target_os, &err.to_string()),
        // What a serial port read answers once the cable is out.
        _ if err
            .raw_os_error()
            .is_some_and(|code| gone_at_open(code, target_os)) =>
        {
            Error::Disconnected
        }
        _ => Error::Io(without_port_names(&err.to_string())),
    }
}

/// Whether OS error `code` from opening the meter means it has gone: ENXIO
/// (6) and ENODEV (19). The same numbers mean something else on Windows.
pub fn gone_at_open(code: i32, target_os: &str) -> bool {
    target_os != "windows" && matches!(code, 6 | 19)
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        (**self).transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        (**self).read(timeout)
    }

    fn transact_record_table(
        &mut self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        (**self).transact_record_table(request, timeout)
    }

    fn transact_settings(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        (**self).transact_settings(request, timeout)
    }

    fn transact_logo_chunk(&mut self, plain: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        (**self).transact_logo_chunk(plain, timeout)
    }

    fn transact_logo_flag(&mut self, frame: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        (**self).transact_logo_flag(frame, timeout)
    }

    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, Error>> {
        (**self).settings_pipe()
    }

    fn recover(&mut self) -> Result<(), Error> {
        (**self).recover()
    }

    fn adcnew_send(&mut self, request: crate::adcnew::Request) -> Result<(), Error> {
        (**self).adcnew_send(request)
    }

    fn adcnew_frames(&mut self) -> Result<Vec<crate::adcnew::Frame>, Error> {
        (**self).adcnew_frames()
    }

    fn adcnew_counters(&self) -> crate::adcnew::AdcnewCounters {
        (**self).adcnew_counters()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn every_kind_of_port_name_leaves_a_message() {
        for name in [
            "/dev/cu.usbmodemFAKE0011",
            "/dev/tty.usbserial-FAKE0011",
            "cu.usbmodemFAKE0011",
            "tty.usbmodemFAKE0011",
            "ttyACM0",
            "ttyUSB3",
            "/dev/ttyACM0",
            "COM3",
            "com12",
            r"\\.\COM3",
        ] {
            for text in [
                format!("cannot open {name}"),
                format!("\"{name}\": Resource busy"),
                format!("{name}: denied"),
                format!("port ({name}), again"),
                format!("path={name}"),
                format!("port:{name}"),
            ] {
                let clean = without_port_names(&text);
                assert!(!clean.contains(name), "{clean}");
                assert!(clean.contains("the meter's serial port"), "{clean}");
            }
        }
    }

    #[test]
    fn words_that_only_look_like_port_names_stay() {
        for text in [
            "/dev/bus/usb/001/002 is busy",
            "/dev/hidraw3 is busy",
            "/home/dev/powerz.log is busy",
            "/Users/dev/meter is busy",
            "command failed",
            "the com port replied",
            "tty",
        ] {
            assert_eq!(without_port_names(text), text);
        }
    }

    // ---- AC-SJGK7E.5: default timeouts and error mapping ----

    #[test]
    fn default_timeout_is_2000_ms_for_connect() {
        assert_eq!(
            default_timeout(&crate::frame::connect(1)),
            Duration::from_millis(2000)
        );
    }

    #[test]
    fn default_timeout_is_1000_ms_for_other_requests() {
        let req = crate::frame::get_data(1, crate::frame::Attribute::ADC);
        assert_eq!(default_timeout(&req), Duration::from_millis(1000));
        assert_eq!(
            default_timeout(&crate::frame::stop_queue(2)),
            Duration::from_millis(1000)
        );
    }

    #[test]
    fn permission_error_on_linux_names_the_udev_rule() {
        let e = permission_error("linux", "cannot open /dev/bus/usb/001/004");
        assert!(matches!(e, Error::Permission(_)), "got {e:?}");
        let msg = e.to_string();
        assert!(msg.contains("60-powerz.rules"), "message: {msg}");
        assert!(msg.contains("/dev/bus/usb/001/004"), "message: {msg}");
    }

    #[test]
    fn permission_error_on_macos_keeps_the_detail() {
        let e = permission_error("macos", "access denied");
        assert!(matches!(e, Error::Permission(_)), "got {e:?}");
        assert!(e.to_string().contains("access denied"));
    }

    #[test]
    fn map_io_error_timed_out_is_timeout() {
        let e = io::Error::new(io::ErrorKind::TimedOut, "t");
        assert_eq!(map_io_error(&e, "linux"), Error::Timeout);
    }

    #[test]
    fn map_io_error_unplug_kinds_are_disconnected() {
        for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::NotConnected] {
            let e = io::Error::new(kind, "gone");
            assert_eq!(map_io_error(&e, "macos"), Error::Disconnected, "{kind:?}");
        }
    }

    #[test]
    fn map_io_error_no_such_device_codes_are_disconnected_off_windows() {
        for code in [6, 19] {
            let e = io::Error::from_raw_os_error(code);
            assert_eq!(map_io_error(&e, "macos"), Error::Disconnected, "{code}");
            assert_eq!(map_io_error(&e, "linux"), Error::Disconnected, "{code}");
            assert!(
                matches!(map_io_error(&e, "windows"), Error::Io(_)),
                "{code}"
            );
        }
    }

    #[test]
    fn map_io_error_permission_denied_on_linux_names_the_udev_rule() {
        let e = io::Error::new(io::ErrorKind::PermissionDenied, "EACCES");
        match map_io_error(&e, "linux") {
            Error::Permission(msg) => assert!(msg.contains("60-powerz.rules"), "{msg}"),
            other => panic!("expected Permission, got {other:?}"),
        }
    }
}
