//! Maps session and transport errors to plain-language `{ kind, message,
//! hint }` for the UI (FR VHY378).

use powerz_protocol::session;
use powerz_protocol::transport;
use serde::Serialize;

/// Error category the UI branches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The OS refused access to the meter.
    PermissionDenied,
    /// The meter is busy or claimed by another driver.
    DeviceBusy,
    /// The meter did not answer in time.
    Timeout,
    /// The meter answered something unexpected.
    Protocol,
    /// The meter was unplugged.
    Disconnected,
    /// The request itself is not valid right now.
    Usage,
    /// Anything else.
    Internal,
}

/// A plain-language error for the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UiError {
    /// Category.
    pub kind: ErrorKind,
    /// What happened, in plain language.
    pub message: String,
    /// What to try next, if anything.
    pub hint: Option<String>,
}

impl UiError {
    /// A usage error (the request is not valid right now).
    pub fn usage(message: impl Into<String>) -> UiError {
        UiError {
            kind: ErrorKind::Usage,
            message: message.into(),
            hint: None,
        }
    }

    pub(crate) fn internal() -> UiError {
        UiError {
            kind: ErrorKind::Internal,
            message: "Something went wrong inside the app.".to_owned(),
            hint: None,
        }
    }
}

/// Linux hint for a permission failure: the udev rule from `docs/linux.md`.
const LINUX_PERMISSION_HINT: &str = "Install the udev rule: \
sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/, then run \
sudo udevadm control --reload-rules and sudo udevadm trigger, and replug the \
meter (see docs/linux.md).";

/// Linux hint for a busy device: the kernel's `powerz` hwmon driver.
const LINUX_BUSY_HINT: &str = "The kernel's powerz hwmon driver may be holding the \
meter. Unload it with sudo modprobe -r powerz, then connect again (see \
docs/linux.md).";

/// Whether an I/O error text says the device is busy or claimed.
fn is_busy(detail: &str) -> bool {
    let d = detail.to_ascii_lowercase();
    [
        "busy",
        "os error 16",
        "ebusy",
        "claimed",
        "in use",
        "access denied (insufficient",
    ]
    .iter()
    .any(|needle| d.contains(needle))
}

fn permission_denied(target_os: &str) -> UiError {
    let hint = if target_os == "linux" {
        LINUX_PERMISSION_HINT.to_owned()
    } else if target_os == "windows" {
        "Close other apps that use the meter, then replug it.".to_owned()
    } else {
        "Close other apps that use the meter, then replug it and try again.".to_owned()
    };
    UiError {
        kind: ErrorKind::PermissionDenied,
        message: "The system did not allow the app to open the meter.".to_owned(),
        hint: Some(hint),
    }
}

fn device_busy(target_os: &str) -> UiError {
    let hint = if target_os == "linux" {
        LINUX_BUSY_HINT.to_owned()
    } else {
        "Close other apps that use the meter (such as the vendor software), \
         then replug it."
            .to_owned()
    };
    UiError {
        kind: ErrorKind::DeviceBusy,
        message: "The meter is busy or in use by another program or driver.".to_owned(),
        hint: Some(hint),
    }
}

fn timeout() -> UiError {
    UiError {
        kind: ErrorKind::Timeout,
        message: "The meter did not answer in time.".to_owned(),
        hint: Some("Check the cable, then replug the meter and try again.".to_owned()),
    }
}

fn protocol() -> UiError {
    UiError {
        kind: ErrorKind::Protocol,
        message: "The meter sent a reply the app did not understand.".to_owned(),
        hint: Some("Replug the meter and try again; updating its firmware may help.".to_owned()),
    }
}

/// Map a transport error to a plain-language UI error.
pub fn from_transport(err: &transport::Error, target_os: &str) -> UiError {
    use transport::Error as T;
    match err {
        T::Timeout => timeout(),
        T::Disconnected => UiError {
            kind: ErrorKind::Disconnected,
            message: "The meter was unplugged.".to_owned(),
            hint: Some("Plug the meter back in.".to_owned()),
        },
        T::Permission(_) => permission_denied(target_os),
        T::Io(detail) if is_busy(detail) => device_busy(target_os),
        T::NotFound => UiError {
            kind: ErrorKind::Disconnected,
            message: "No meter was found.".to_owned(),
            hint: Some("Plug the meter in and try again.".to_owned()),
        },
        T::Forbidden { .. } | T::UnsupportedOnHid | T::UnsupportedOnCdc { .. } => {
            UiError::usage("This action is not available over the current connection.")
        }
        T::HidReplyTooLong { .. } => protocol(),
        T::Io(_) => UiError {
            kind: ErrorKind::Internal,
            message: "Talking to the meter failed.".to_owned(),
            hint: Some("Replug the meter and try again.".to_owned()),
        },
        T::Mock(_) => UiError::internal(),
    }
}

/// Map a session error to a plain-language UI error.
pub fn from_session(err: &session::Error, target_os: &str) -> UiError {
    use session::Error as S;
    match err {
        S::Transport(t) => from_transport(t, target_os),
        S::Protocol(_) | S::Decode(_) | S::Frame(_) => protocol(),
        S::Rejected { .. } => UiError {
            kind: ErrorKind::Protocol,
            message: "The meter refused the request.".to_owned(),
            hint: Some("Updating the meter's firmware may help.".to_owned()),
        },
        S::Forbidden { .. } => UiError::usage("The app refused a request that is not safe."),
        S::AuthFailed => UiError {
            kind: ErrorKind::Protocol,
            message: "The meter did not accept the app's sign-in.".to_owned(),
            hint: Some("Replug the meter and try again.".to_owned()),
        },
    }
}

/// Map a panic payload without showing its message.
pub fn from_panic(_payload: &(dyn std::any::Any + Send)) -> UiError {
    UiError::internal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use powerz_protocol::session;
    use powerz_protocol::transport::{self, permission_error};

    /// No raw Rust error text: no debug output, paths or OS error codes.
    fn assert_plain(e: &UiError) {
        assert!(!e.message.trim().is_empty(), "empty message");
        for raw in [
            "os error",
            "::",
            "Permission(",
            "Io(",
            "Timeout(",
            "Protocol(",
            "Accept(",
            "MainHeader",
            "{",
            "panicked",
            "unwrap",
        ] {
            assert!(
                !e.message.contains(raw),
                "raw text {raw:?} in message: {}",
                e.message
            );
            if let Some(h) = &e.hint {
                assert!(!h.contains(raw), "raw text {raw:?} in hint: {h}");
            }
        }
    }

    // ---- AC-VHY378.7: plain-language errors ----

    #[test]
    fn permission_denied_on_linux_names_the_udev_rule() {
        let err = permission_error("linux", "cannot open /dev/bus/usb/001/004");
        let e = from_transport(&err, "linux");
        assert_eq!(e.kind, ErrorKind::PermissionDenied);
        let hint = e.hint.clone().expect("no hint");
        assert!(hint.contains("60-powerz.rules"), "hint: {hint}");
        assert!(hint.contains("docs/linux.md"), "hint: {hint}");
        assert_plain(&e);
    }

    #[test]
    fn permission_denied_elsewhere_does_not_mention_udev() {
        let e = from_transport(
            &transport::Error::Permission("access denied".into()),
            "macos",
        );
        assert_eq!(e.kind, ErrorKind::PermissionDenied);
        assert!(!e.hint.clone().unwrap_or_default().contains("udev"));
        assert_plain(&e);
    }

    #[test]
    fn busy_device_is_reported_as_busy_with_the_kernel_driver_hint_on_linux() {
        let err = transport::Error::Io("Device or resource busy (os error 16)".into());
        let e = from_transport(&err, "linux");
        assert_eq!(e.kind, ErrorKind::DeviceBusy);
        let hint = e.hint.clone().expect("no hint");
        assert!(hint.contains("powerz"), "hint: {hint}");
        assert!(hint.contains("docs/linux.md"), "hint: {hint}");
        assert_plain(&e);

        let e = from_transport(&err, "windows");
        assert_eq!(e.kind, ErrorKind::DeviceBusy);
        assert_plain(&e);
    }

    #[test]
    fn busy_hint_does_not_offer_pipes_the_desktop_app_lacks() {
        let err = transport::Error::Io("Device or resource busy (os error 16)".into());
        let hint = from_transport(&err, "linux").hint.expect("no hint");
        assert!(!hint.contains("HID"), "hint: {hint}");
        assert!(!hint.contains("serial"), "hint: {hint}");
    }

    #[test]
    fn timeout_is_reported_as_timeout() {
        let e = from_transport(&transport::Error::Timeout, "macos");
        assert_eq!(e.kind, ErrorKind::Timeout);
        assert_plain(&e);
        let e = from_session(
            &session::Error::Transport(transport::Error::Timeout),
            "linux",
        );
        assert_eq!(e.kind, ErrorKind::Timeout);
        assert_plain(&e);
    }

    #[test]
    fn protocol_errors_hide_the_raw_reply() {
        let raw =
            session::Error::Protocol("unexpected reply to GET_DATA: Accept(MainHeader(5))".into());
        let e = from_session(&raw, "macos");
        assert_eq!(e.kind, ErrorKind::Protocol);
        assert_plain(&e);
    }

    #[test]
    fn session_transport_errors_keep_their_kind() {
        let err = session::Error::Transport(permission_error("linux", "EACCES"));
        assert_eq!(
            from_session(&err, "linux").kind,
            ErrorKind::PermissionDenied
        );
        let err = session::Error::Transport(transport::Error::Disconnected);
        let e = from_session(&err, "macos");
        assert_eq!(e.kind, ErrorKind::Disconnected);
        assert_plain(&e);
    }

    #[test]
    fn a_panic_never_shows_its_message() {
        let payload: Box<dyn std::any::Any + Send> =
            Box::new("index out of bounds: the len is 3 but the index is 7".to_string());
        let e = from_panic(payload.as_ref());
        assert_eq!(e.kind, ErrorKind::Internal);
        assert!(!e.message.contains("index out of bounds"), "{}", e.message);
        assert_plain(&e);
    }

    #[test]
    fn serializes_as_kind_message_hint() {
        let e = from_transport(&permission_error("linux", "EACCES"), "linux");
        let v = serde_json::to_value(&e).unwrap();
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["hint", "kind", "message"]);
        assert_eq!(obj["kind"], "permission_denied");
    }
}
