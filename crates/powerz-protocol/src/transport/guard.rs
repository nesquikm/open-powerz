//! Command guard: refuses frame types that could damage or brick the meter.

use std::time::Duration;

use super::{Error, Transport};
use crate::decode::records::ENTRY_LEN;

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

/// SET_DATA attribute of the record table (the offline-recording list).
/// \[static\]
pub const RECORD_TABLE_ATT: u16 = 0x28;

/// Accept `request` only if it is the record-table write laid out as in
/// `docs/protocol.md` "Deleting records" \[static\]: main header
/// `0x48 | id << 8 | ((len / 4) + 2) << 22` (extend bit clear), one extended
/// section header (`att` 0x28, `next` 0, `chunk` 0, `size` = `len`), then
/// `len` data bytes made of whole 48-byte entries and four zero bytes.
///
/// Every other frame, including SET_DATA with any other attribute
/// (calibration, reset buttons, the logo flag, ...), is refused with
/// [`Error::Forbidden`]. The shared [`check`] still refuses every 0x48; this
/// rule applies only on the record-table path
/// ([`Transport::transact_record_table`]).
pub fn check_record_table_write(request: &[u8]) -> Result<(), Error> {
    let refuse = Error::Forbidden {
        ty: crate::frame::frame_type(request).unwrap_or(0x48),
    };
    let word = |at: usize| -> Option<u32> {
        let b = request.get(at..at + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let (Some(main), Some(ext)) = (word(0), word(4)) else {
        return Err(refuse);
    };
    let data = &request[8..];
    let len = data.len();
    let main_ok = main & 0x003F_00FF == 0x48 && (main >> 22) as usize == len / 4 + 2;
    let ext_ok = ext & 0x003F_FFFF == u32::from(RECORD_TABLE_ATT) && (ext >> 22) as usize == len;
    let data_ok = len >= 4 && (len - 4).is_multiple_of(ENTRY_LEN) && data[len - 4..] == [0u8; 4];
    if main_ok && ext_ok && data_ok {
        Ok(())
    } else {
        Err(refuse)
    }
}

/// Wraps a transport and runs [`check`] before every write
/// ([`check_record_table_write`] on the record-table path).
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

    /// Runs [`check_record_table_write`] instead of [`check`], then hands the
    /// frame to the inner transport's record-table path (so nested guards
    /// each check it).
    fn transact_record_table(
        &mut self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        check_record_table_write(request)?;
        self.inner.transact_record_table(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        self.inner.read(timeout)
    }

    fn recover(&mut self) -> Result<(), Error> {
        self.inner.recover()
    }

    fn adcnew_send(&mut self, request: crate::adcnew::Request) -> Result<(), Error> {
        self.inner.adcnew_send(request)
    }

    fn adcnew_frames(&mut self) -> Result<Vec<crate::adcnew::Frame>, Error> {
        self.inner.adcnew_frames()
    }

    fn adcnew_counters(&self) -> crate::adcnew::AdcnewCounters {
        self.inner.adcnew_counters()
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

    // ---- AC-JKWT3V.5: the record-table write is the only SET_DATA allowed ----
    //
    // Layout from docs/protocol.md "Deleting records": main header
    // `0x48 | id << 8 | ((len / 4) + 2) << 22`, one extended section header
    // (att 0x28, next 0, chunk 0, size = len), then `len` data bytes: the
    // remaining 48-byte entries and four zero bytes.

    /// A SET_DATA frame laid out like the record-table write, for any `att`.
    fn set_data(id: u8, att: u16, data: &[u8]) -> Vec<u8> {
        let words = (data.len() / 4 + 2) as u32;
        let mut f = (0x48u32 | (u32::from(id) << 8) | (words << 22))
            .to_le_bytes()
            .to_vec();
        f.extend((u32::from(att) | ((data.len() as u32 & 0x3FF) << 22)).to_le_bytes());
        f.extend_from_slice(data);
        f
    }

    /// `n` synthetic 48-byte entries, then the four zero bytes.
    fn table(n: usize) -> Vec<u8> {
        let mut d: Vec<u8> = (0..n)
            .flat_map(|k| {
                let mut e = [0u8; 48];
                e[0] = b'R';
                e[1] = b'0' + (k % 10) as u8;
                e[0x11] = 1;
                e[0x12] = 3;
                e[0x24..0x28].copy_from_slice(&((k as u32) * 0x1000).to_le_bytes());
                e[0x28..0x30].copy_from_slice(&[0xA5; 8]);
                e
            })
            .collect();
        d.extend([0u8; 4]);
        d
    }

    fn ext_word(f: &[u8]) -> u32 {
        u32::from_le_bytes([f[4], f[5], f[6], f[7]])
    }

    fn with_ext_word(mut f: Vec<u8>, w: u32) -> Vec<u8> {
        f[4..8].copy_from_slice(&w.to_le_bytes());
        f
    }

    #[test]
    fn shared_check_still_refuses_a_well_formed_record_table_write() {
        for n in [0, 1, 21] {
            assert_eq!(
                check(&set_data(1, 0x28, &table(n))),
                Err(Error::Forbidden { ty: 0x48 }),
                "{n} entries"
            );
        }
        assert!(FORBIDDEN_TYPES.contains(&0x48), "0x48 stays forbidden");
    }

    #[test]
    fn record_table_check_accepts_the_documented_layout() {
        for n in [0, 1, 2, 21] {
            let f = set_data(7, 0x28, &table(n));
            assert_eq!(f.len(), 8 + n * 48 + 4);
            assert_eq!(check_record_table_write(&f), Ok(()), "{n} entries");
        }
    }

    #[test]
    fn record_table_check_refuses_every_other_set_data_attribute() {
        // Calibration (10, 11), the reset buttons (8, 9), the logo flag
        // (0x1C) and every other attribute stay refused.
        for att in (0u16..0x8000).filter(|&a| a != 0x28) {
            assert_eq!(
                check_record_table_write(&set_data(1, att, &table(1))),
                Err(Error::Forbidden { ty: 0x48 }),
                "att {att:#x}"
            );
        }
    }

    #[test]
    fn record_table_check_refuses_malformed_frames() {
        let good = set_data(1, 0x28, &table(2));
        let w = ext_word(&good);
        let mut bad: Vec<(&str, Vec<u8>)> = vec![
            ("next bit set", with_ext_word(good.clone(), w | (1 << 15))),
            ("chunk not zero", with_ext_word(good.clone(), w | (1 << 16))),
            (
                "size shorter than the data",
                with_ext_word(good.clone(), 0x28 | (96u32 << 22)),
            ),
            ("truncated frame", good[..good.len() - 4].to_vec()),
            ("header only", good[..4].to_vec()),
            ("empty", Vec::new()),
        ];
        // Main header word count that does not match the data.
        let mut f = good.clone();
        let h = u32::from_le_bytes([f[0], f[1], f[2], f[3]]);
        f[0..4].copy_from_slice(&((h & 0x003F_FFFF) | (3 << 22)).to_le_bytes());
        bad.push(("main header size mismatch", f));
        // Extend bit set on the main header.
        let mut f = good.clone();
        f[0] |= 0x80;
        bad.push(("extend bit", f));
        // Data that does not end in four zero bytes.
        let mut d = table(2);
        let n = d.len();
        d[n - 1] = 1;
        bad.push(("no zero terminator", set_data(1, 0x28, &d)));
        // Data that is not whole 48-byte entries plus the terminator.
        bad.push(("partial entry", set_data(1, 0x28, &[0u8; 48])));
        bad.push(("no terminator at all", set_data(1, 0x28, &[])));
        // Over 1023 bytes: the 10-bit size cannot hold it (22 entries).
        bad.push(("table over 1023 bytes", set_data(1, 0x28, &table(22))));
        for (what, f) in bad {
            assert!(
                matches!(check_record_table_write(&f), Err(Error::Forbidden { .. })),
                "{what}: {f:02x?}"
            );
        }
    }

    #[test]
    fn record_table_check_refuses_other_frame_types() {
        let mut flash = set_data(1, 0x28, &table(1));
        flash[0] = 0x4A;
        for f in [vec![0x0C, 0x01, 0x00, 0x04], vec![0x02, 0x01, 0, 0], flash] {
            assert!(
                matches!(check_record_table_write(&f), Err(Error::Forbidden { .. })),
                "{f:02x?}"
            );
        }
    }

    #[test]
    fn guarded_transport_refuses_the_record_table_write_on_the_shared_path() {
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        let f = set_data(4, 0x28, &table(1));
        assert_eq!(g.transact(&f, T), Err(Error::Forbidden { ty: 0x48 }));
        assert!(g.inner().written().is_empty());
    }

    #[test]
    fn guarded_transport_passes_the_record_table_write_on_its_own_path() {
        let f = set_data(4, 0x28, &table(2));
        let accept = vec![0x05, 0x04, 0x00, 0x00];
        let mut g = GuardedTransport::new(MockTransport::new(vec![Step::transact(
            f.clone(),
            accept.clone(),
        )]));
        assert_eq!(g.transact_record_table(&f, T), Ok(accept));
        assert_eq!(g.inner().written(), &[f]);
    }

    #[test]
    fn guarded_transport_refuses_anything_else_on_the_record_table_path() {
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        for att in [0x08, 0x09, 0x0A, 0x0B, 0x1C, 0x27, 0x29] {
            assert_eq!(
                g.transact_record_table(&set_data(4, att, &table(1)), T),
                Err(Error::Forbidden { ty: 0x48 }),
                "att {att:#x}"
            );
        }
        assert!(matches!(
            g.transact_record_table(&[0x0C, 0x01, 0x00, 0x04], T),
            Err(Error::Forbidden { .. })
        ));
        assert!(g.inner().written().is_empty());
    }

    #[test]
    fn nested_guards_each_check_the_record_table_path() {
        let f = set_data(4, 0x28, &table(1));
        let accept = vec![0x05, 0x04, 0x00, 0x00];
        let mut g = GuardedTransport::new(GuardedTransport::new(MockTransport::new(vec![
            Step::transact(f.clone(), accept.clone()),
        ])));
        assert_eq!(g.transact_record_table(&f, T), Ok(accept));
        let bad = set_data(5, 0x0A, &table(1));
        assert_eq!(
            g.transact_record_table(&bad, T),
            Err(Error::Forbidden { ty: 0x48 })
        );
        assert_eq!(g.inner().inner().written(), &[f]);
    }
}
