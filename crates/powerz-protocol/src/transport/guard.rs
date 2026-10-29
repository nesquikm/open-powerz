//! Command guard: refuses frame types that could damage or brick the meter.

use std::time::Duration;

use super::{Error, Transport};
use crate::decode::records::ENTRY_LEN;
use crate::decode::settings::{
    SETTINGS_WRITE_KEYS, SETTINGS_WRITE_TYPE, settings_entry_word, settings_key_range,
};

/// Frame types never sent on the shared path: RESET, JUMP_APROM, JUMP_DFU,
/// the settings write, firmware download, SET_DATA and memory / flash write.
pub const FORBIDDEN_TYPES: [u8; 7] = [0x04, 0x08, 0x09, 0x0A, 0x42, 0x48, 0x4A];

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

/// Accept `request` only if it is the settings write laid out as in
/// `docs/protocol.md` "Settings write" \[static\]: main header
/// `0x0A | id << 8 | n << 23` (extend bit clear, `n` entries), then exactly
/// the keys of [`SETTINGS_WRITE_KEYS`] in order, each as the key word
/// `0x01008000 | key` (`next` clear on the last entry only) and an i32 value
/// within that key's range, the last being the save marker.
///
/// Every other frame, including the factory-reset key 0x08 and any
/// calibration key, is refused with [`Error::Forbidden`]. The shared
/// [`check`] still refuses every 0x0A; this rule applies only on the
/// settings path ([`Transport::transact_settings`]).
pub fn check_settings_write(request: &[u8]) -> Result<(), Error> {
    let refuse = Error::Forbidden {
        ty: crate::frame::frame_type(request).unwrap_or(SETTINGS_WRITE_TYPE),
    };
    let word = |at: usize| -> Option<u32> {
        let b = request.get(at..at + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let n = SETTINGS_WRITE_KEYS.len();
    let Some(main) = word(0) else {
        return Err(refuse);
    };
    if main & 0x007F_00FF != u32::from(SETTINGS_WRITE_TYPE)
        || (main >> 23) as usize != n
        || request.len() != 4 + 8 * n
    {
        return Err(refuse);
    }
    for (i, &key) in SETTINGS_WRITE_KEYS.iter().enumerate() {
        let (Some(k), Some(v)) = (word(4 + 8 * i), word(8 + 8 * i)) else {
            return Err(refuse);
        };
        let in_range = settings_key_range(key).is_some_and(|r| r.contains(&(v as i32)));
        if Some(k) != settings_entry_word(i) || !in_range {
            return Err(refuse);
        }
    }
    Ok(())
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

    /// Runs [`check_settings_write`] instead of [`check`], then hands the
    /// frame to the inner transport's settings path.
    fn transact_settings(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        check_settings_write(request)?;
        self.inner.transact_settings(request, timeout)
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

    // ---- AC-JKWT3W.3: the settings write (type 0x0A) is the only one allowed ----
    //
    // Layout from docs/protocol.md "Settings write": main header
    // `0x0A | id << 8 | n << 23`, then n 8-byte entries in ascending key
    // order (key word `0x01008000 | key`, `next` cleared on the last), every
    // writable key once, ending with 0x7FFF = 0x1234.

    /// The documented key set with in-range values.
    fn good_entries() -> Vec<(u16, u32)> {
        vec![
            (0x01, 1),
            (0x03, 50),
            (0x04, 0),
            (0x06, 0),
            (0x07, 0),
            (0x14, 2),
            (0x15, 1),
            (0x7FFF, 0x1234),
        ]
    }

    /// A type-0x0A frame with `entries`, `next` set on all but the last.
    fn settings_frame(id: u8, entries: &[(u16, u32)]) -> Vec<u8> {
        let n = entries.len() as u32;
        let mut f = (0x0Au32 | (u32::from(id) << 8) | (n << 23))
            .to_le_bytes()
            .to_vec();
        for (i, &(key, value)) in entries.iter().enumerate() {
            let next = if i + 1 < entries.len() { 1u32 << 15 } else { 0 };
            f.extend((0x0100_0000u32 | next | u32::from(key)).to_le_bytes());
            f.extend(value.to_le_bytes());
        }
        f
    }

    fn with_value(key: u16, value: u32) -> Vec<(u16, u32)> {
        let mut e = good_entries();
        e.iter_mut().find(|(k, _)| *k == key).unwrap().1 = value;
        e
    }

    fn refused(f: &[u8]) -> bool {
        matches!(check_settings_write(f), Err(Error::Forbidden { .. }))
    }

    #[test]
    fn shared_check_refuses_every_settings_write() {
        assert!(FORBIDDEN_TYPES.contains(&0x0A), "0x0A is forbidden");
        assert_eq!(
            check(&settings_frame(1, &good_entries())),
            Err(Error::Forbidden { ty: 0x0A })
        );
        assert_eq!(
            check(&[0x0A, 0x01, 0x00, 0x00]),
            Err(Error::Forbidden { ty: 0x0A })
        );
    }

    #[test]
    fn settings_check_accepts_the_documented_layout() {
        assert_eq!(
            check_settings_write(&settings_frame(1, &good_entries())),
            Ok(())
        );
        assert_eq!(settings_frame(1, &good_entries()).len(), 4 + 8 * 8);
        // Every edge of every range.
        for (key, lo, hi) in [
            (0x01u16, 0u32, 1u32),
            (0x03, 1, 100),
            (0x04, 0, 3),
            (0x06, 0, 3),
            (0x07, 0, 6),
            (0x14, 0, 3),
            (0x15, 0, 3),
        ] {
            for v in [lo, hi] {
                assert_eq!(
                    check_settings_write(&settings_frame(9, &with_value(key, v))),
                    Ok(()),
                    "key {key:#x} = {v}"
                );
            }
        }
    }

    #[test]
    fn settings_check_refuses_values_out_of_range() {
        for (key, bad) in [
            (0x01u16, 2u32),
            (0x03, 0),
            (0x03, 101),
            (0x03, 127),
            (0x04, 4),
            (0x06, 4),
            (0x07, 7),
            (0x14, 4),
            (0x15, 4),
            (0x15, u32::MAX),
            (0x7FFF, 0),
            (0x7FFF, 0x1235),
        ] {
            assert!(
                refused(&settings_frame(1, &with_value(key, bad))),
                "key {key:#x} = {bad}"
            );
        }
    }

    #[test]
    fn settings_check_refuses_the_factory_reset_key() {
        // Key 0x08 = 1 erases settings and records.
        let mut e = good_entries();
        e.insert(5, (0x08, 1));
        assert!(refused(&settings_frame(1, &e)));
        let mut e = good_entries();
        e[4] = (0x08, 1);
        assert!(refused(&settings_frame(1, &e)));
        assert!(refused(&settings_frame(1, &[(0x08, 1), (0x7FFF, 0x1234)])));
    }

    #[test]
    fn settings_check_refuses_every_unknown_key() {
        // Calibration, OTP or anything else: no key outside the table passes,
        // neither in place of a known key nor added to the set.
        let known = [0x01u16, 0x03, 0x04, 0x06, 0x07, 0x14, 0x15, 0x7FFF];
        for key in (0u16..0x8000).filter(|k| !known.contains(k)) {
            let mut replaced = good_entries();
            replaced[6] = (key, 0);
            assert!(
                refused(&settings_frame(1, &replaced)),
                "key {key:#x} replaced"
            );
            let mut added = good_entries();
            added.insert(7, (key, 0));
            assert!(refused(&settings_frame(1, &added)), "key {key:#x} added");
        }
    }

    #[test]
    fn settings_check_refuses_malformed_frames() {
        let good = settings_frame(1, &good_entries());
        let mut bad: Vec<(&str, Vec<u8>)> = Vec::new();
        let mut missing = good_entries();
        missing.remove(2);
        bad.push(("a key missing", settings_frame(1, &missing)));
        let mut dup = good_entries();
        dup.insert(2, (0x03, 50));
        bad.push(("a key twice", settings_frame(1, &dup)));
        let mut swapped = good_entries();
        swapped.swap(1, 2);
        bad.push(("keys out of order", settings_frame(1, &swapped)));
        let mut no_marker = good_entries();
        no_marker.pop();
        bad.push(("no save marker", settings_frame(1, &no_marker)));
        // `next` set on the last entry.
        let mut f = good.clone();
        let last = f.len() - 8;
        f[last + 1] |= 0x80;
        bad.push(("next on the last entry", f));
        // `next` cleared on a middle entry.
        let mut f = good.clone();
        f[4 + 8 + 1] &= 0x7F;
        bad.push(("next cleared in the middle", f));
        // Entry size field other than 4.
        let mut f = good.clone();
        f[4 + 3] = 0x02;
        bad.push(("entry size 8", f));
        // Main header entry count that does not match.
        let mut f = good.clone();
        let h = u32::from_le_bytes([f[0], f[1], f[2], f[3]]);
        f[0..4].copy_from_slice(&((h & 0x007F_FFFF) | (7 << 23)).to_le_bytes());
        bad.push(("count mismatch", f));
        // Extend bit on the main header.
        let mut f = good.clone();
        f[0] |= 0x80;
        bad.push(("extend bit", f));
        bad.push(("truncated", good[..good.len() - 4].to_vec()));
        let mut f = good.clone();
        f.extend([0u8; 8]);
        bad.push(("trailing bytes", f));
        bad.push(("header only", good[..4].to_vec()));
        bad.push(("empty", Vec::new()));
        for (what, f) in bad {
            assert!(refused(&f), "{what}: {f:02x?}");
        }
    }

    #[test]
    fn settings_check_refuses_other_frame_types() {
        let mut set_data_cal = settings_frame(1, &good_entries());
        set_data_cal[0] = 0x48;
        for f in [
            vec![0x0C, 0x01, 0x10, 0x00],
            vec![0x02, 0x01, 0, 0],
            set_data_cal,
            set_data(1, 0x0A, &table(1)),
        ] {
            assert!(refused(&f), "{f:02x?}");
        }
    }

    #[test]
    fn guarded_transport_refuses_the_settings_write_on_the_shared_path() {
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        let f = settings_frame(3, &good_entries());
        assert_eq!(g.transact(&f, T), Err(Error::Forbidden { ty: 0x0A }));
        assert!(g.inner().written().is_empty());
    }

    #[test]
    fn guarded_transport_passes_the_settings_write_on_its_own_path() {
        let f = settings_frame(3, &good_entries());
        let accept = vec![0x05, 0x03, 0x00, 0x00];
        let mut g = GuardedTransport::new(MockTransport::new(vec![Step::transact(
            f.clone(),
            accept.clone(),
        )]));
        assert_eq!(g.transact_settings(&f, T), Ok(accept));
        assert_eq!(g.inner().written(), &[f]);
    }

    #[test]
    fn guarded_transport_refuses_anything_else_on_the_settings_path() {
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        let mut reset = good_entries();
        reset.insert(5, (0x08, 1));
        for f in [
            settings_frame(3, &reset),
            settings_frame(3, &with_value(0x03, 0)),
            set_data(3, 0x28, &table(1)),
            vec![0x0C, 0x01, 0x10, 0x00],
        ] {
            assert!(
                matches!(g.transact_settings(&f, T), Err(Error::Forbidden { .. })),
                "{f:02x?}"
            );
        }
        assert!(g.inner().written().is_empty());
        // The record-table path does not take a settings write either.
        assert!(matches!(
            g.transact_record_table(&settings_frame(3, &good_entries()), T),
            Err(Error::Forbidden { .. })
        ));
        assert!(g.inner().written().is_empty());
    }

    #[test]
    fn nested_guards_each_check_the_settings_path() {
        let f = settings_frame(4, &good_entries());
        let accept = vec![0x05, 0x04, 0x00, 0x00];
        let mut g = GuardedTransport::new(GuardedTransport::new(MockTransport::new(vec![
            Step::transact(f.clone(), accept.clone()),
        ])));
        assert_eq!(g.transact_settings(&f, T), Ok(accept));
        let bad = settings_frame(5, &with_value(0x07, 9));
        assert!(matches!(
            g.transact_settings(&bad, T),
            Err(Error::Forbidden { .. })
        ));
        assert_eq!(g.inner().inner().written(), &[f]);
    }
}
