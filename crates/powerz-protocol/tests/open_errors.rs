//! AC-AXHK2J.5: which OS errors at open mean the meter has gone. nusb hands
//! its OS code over beside the error (`nusb::Error::os_error`), so the code
//! is pinned on its own here, not through an `io::Error`.

use powerz_protocol::transport::{self, gone_at_open};

#[test]
fn os_errors_6_and_19_at_open_mean_the_meter_is_gone() {
    for os in ["linux", "macos"] {
        assert!(gone_at_open(6, os), "ENXIO on {os}");
        assert!(gone_at_open(19, os), "ENODEV on {os}");
    }
}

#[test]
fn the_same_numbers_mean_something_else_on_windows() {
    assert!(!gone_at_open(6, "windows"));
    assert!(!gone_at_open(19, "windows"));
}

#[test]
fn no_other_code_counts() {
    for code in [0, 1, 2, 5, 13, 16, 60, 61, 71, 190] {
        for os in ["linux", "macos"] {
            assert!(!gone_at_open(code, os), "{code} on {os}");
        }
    }
}

#[test]
fn the_io_error_mapping_agrees_with_the_pin() {
    for code in [6, 19] {
        let e = std::io::Error::from_raw_os_error(code);
        assert_eq!(
            transport::map_io_error(&e, "linux"),
            transport::Error::Disconnected,
            "{code}"
        );
    }
}
