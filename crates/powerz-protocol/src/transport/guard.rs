//! Command guard: refuses frame types that could damage or brick the meter.

use std::time::Duration;

use super::{Error, Transport};

/// Frame types never sent: RESET, JUMP_APROM, JUMP_DFU, firmware download,
/// SET_DATA and memory / flash write.
pub const FORBIDDEN_TYPES: [u8; 6] = [0x04, 0x08, 0x09, 0x42, 0x48, 0x4A];

/// Refuse `request` if its frame type (bits 0..6 of byte 0) is forbidden.
pub fn check(request: &[u8]) -> Result<(), Error> {
    match crate::frame::frame_type(request) {
        Some(ty) if FORBIDDEN_TYPES.contains(&ty) => Err(Error::Forbidden { ty }),
        _ => Ok(()),
    }
}

/// Wraps a transport and runs [`check`] before every write.
#[derive(Debug)]
pub struct GuardedTransport<T> {
    inner: T,
}

impl<T> GuardedTransport<T> {
    /// Wrap `inner`.
    pub fn new(inner: T) -> GuardedTransport<T> {
        GuardedTransport { inner }
    }

    /// The wrapped transport.
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

impl<T: Transport> Transport for GuardedTransport<T> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        check(request)?;
        self.inner.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        self.inner.read(timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::{MockTransport, Step};

    const T: Duration = Duration::from_millis(1000);

    // ---- AC-SJGK7E.8: shared guard ----

    #[test]
    fn check_refuses_every_forbidden_type() {
        for ty in [0x04, 0x08, 0x09, 0x42, 0x48, 0x4A] {
            assert_eq!(
                check(&[ty, 0x01, 0x00, 0x00]),
                Err(Error::Forbidden { ty }),
                "{ty:#x}"
            );
        }
    }

    #[test]
    fn check_masks_the_extend_bit() {
        // 0x84 = type 0x04 with the extend bit set.
        assert_eq!(
            check(&[0x84, 0x01, 0, 0]),
            Err(Error::Forbidden { ty: 0x04 })
        );
        assert_eq!(
            check(&[0xC8, 0x01, 0, 0]),
            Err(Error::Forbidden { ty: 0x48 })
        );
    }

    #[test]
    fn check_allows_safe_types() {
        for ty in [0x02, 0x0C, 0x0E, 0x0F, 0x10, 0x11, 0x44, 0x4C] {
            assert_eq!(check(&[ty, 0x01, 0x00, 0x00]), Ok(()), "{ty:#x}");
        }
    }

    #[test]
    fn guarded_transport_refuses_without_writing() {
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        for ty in FORBIDDEN_TYPES {
            assert_eq!(
                g.transact(&[ty, 0x01, 0x00, 0x00], T),
                Err(Error::Forbidden { ty })
            );
        }
        assert!(g.inner().written().is_empty());
    }

    #[test]
    fn guarded_transport_passes_safe_requests_through() {
        let req = [0x0C, 0x01, 0x02, 0x00];
        let mut g = GuardedTransport::new(MockTransport::new(vec![
            Step::transact(req, vec![0x41, 0x01]),
            Step::read(vec![0x41, 0x02]),
        ]));
        assert_eq!(g.transact(&req, T).unwrap(), vec![0x41, 0x01]);
        assert_eq!(g.read(T).unwrap(), vec![0x41, 0x02]);
        assert_eq!(g.inner().written(), &[req.to_vec()]);
    }
}
