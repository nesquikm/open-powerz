//! AC-4PJRVM.7, the premise of the held-port mapping: on a real tty, an
//! exclusive open of a path someone already holds exclusively comes back from
//! `serialport` as kind `NoDevice`, the same kind as a missing device, not as
//! an I/O error.
//!
//! The first hold is `set_exclusive` (the same TIOCEXCL and flock `open` takes)
//! on the pty's own slave: on macOS `open` cannot finish on a pty, its baud
//! ioctl fails with ENOTTY.
#![cfg(unix)]

use serialport::{ErrorKind, SerialPort as _, TTYPort};

#[test]
fn an_exclusive_open_of_a_held_pty_is_kind_no_device() {
    let (_master, mut holder) = TTYPort::pair().expect("a pty");
    let path = holder.name().expect("a pty slave has a path");
    holder.set_exclusive(true).expect("the first hold succeeds");

    match serialport::new(&path, 115_200).open_native() {
        Ok(_) => panic!("the second open of {path} succeeded"),
        Err(e) => assert_eq!(e.kind(), ErrorKind::NoDevice, "{e:?}"),
    }
}
