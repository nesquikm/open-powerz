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
    /// ADCNEW stream requests exist only on the CDC port; nothing was sent.
    #[error("the ADCNEW stream is only available over the CDC serial port")]
    AdcnewNeedsCdc,
    /// No matching device or port was found.
    #[error("no matching device found")]
    NotFound,
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
/// the message names the udev rule `packaging/linux/60-powerz.rules`.
pub fn permission_error(target_os: &str, detail: &str) -> Error {
    if target_os == "linux" {
        Error::Permission(format!(
            "{detail}; install the udev rule packaging/linux/60-powerz.rules \
             into /etc/udev/rules.d/ and replug the meter"
        ))
    } else {
        Error::Permission(detail.to_string())
    }
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
        _ => Error::Io(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

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
    fn map_io_error_permission_denied_on_linux_names_the_udev_rule() {
        let e = io::Error::new(io::ErrorKind::PermissionDenied, "EACCES");
        match map_io_error(&e, "linux") {
            Error::Permission(msg) => assert!(msg.contains("60-powerz.rules"), "{msg}"),
            other => panic!("expected Permission, got {other:?}"),
        }
    }
}
