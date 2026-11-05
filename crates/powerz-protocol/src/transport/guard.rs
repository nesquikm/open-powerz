//! Command guard: refuses frame types that could damage or brick the meter.
//!
//! The shared [`check`] refuses every type in [`FORBIDDEN_TYPES`]. A few
//! narrow exceptions each get their own path with their own exact-layout
//! check: the record-table write ([`check_record_table_write`]), the
//! settings write ([`check_settings_write`]), and the startup-logo flash
//! chunk and flag write ([`check_logo_chunk`], [`check_logo_flag_write`],
//! FR R1ST8Z).

use std::time::Duration;

use super::{Error, Transport};
use crate::decode::records::ENTRY_LEN;
use crate::decode::settings::{
    SETTINGS_WRITE_KEYS, SETTINGS_WRITE_TYPE, settings_entry_word, settings_key_range,
};

/// Frame types never sent on the shared path: RESET, JUMP_APROM, JUMP_DFU,
/// the settings write, firmware download, SET_DATA and memory / flash write.
/// Only the dedicated paths above let a frame of type 0x0A, 0x48 or 0x4A
/// through, and only in the one layout each of them checks.
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

/// SET_DATA attribute of the startup-logo flag. \[static\]
pub const LOGO_FLAG_ATT: u16 = 0x1C;

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
    let word = |at: usize| le_word(request, at);
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
    let word = |at: usize| le_word(request, at);
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

/// Frame type of the memory / flash write that carries logo chunks.
/// \[static\]
pub const LOGO_CHUNK_TYPE: u8 = 0x4A;

/// First flash address of the startup-logo region. \[static\]
pub const LOGO_REGION_START: u32 = 0x0030_0000;

/// One past the last flash address of the startup-logo region
/// (0x0031D500): the vendor's largest blob \[static\], padded to whole
/// chunks by us \[inferred\].
pub const LOGO_REGION_END: u32 =
    LOGO_REGION_START + crate::logo::padded_len(crate::logo::MAX_BLOB) as u32;

/// Accept `plain` only if it is a plaintext startup-logo chunk frame
/// (FR R1ST8Z) laid out as in `docs/protocol.md` \[static\]: main header
/// `0x4A | id << 8 | 1 << 16 | (len + 16) << 22` (extend bit clear,
/// bits 17..21 clear), a 16-byte block header (address, `len`,
/// 0xFFFFFFFF, CRC-32 of the first 12 bytes), then `len` data bytes, with
/// `len` 16 or 256 and the address a 256-byte step inside
/// `LOGO_REGION_START..LOGO_REGION_END`.
///
/// Every other frame is refused with [`Error::Forbidden`].
pub fn check_logo_chunk(plain: &[u8]) -> Result<(), Error> {
    let refuse = Error::Forbidden {
        ty: crate::frame::frame_type(plain).unwrap_or(LOGO_CHUNK_TYPE),
    };
    let word = |at: usize| le_word(plain, at);
    let (Some(main), Some(addr), Some(len2), Some(word3), Some(crc)) =
        (word(0), word(4), word(8), word(12), word(16))
    else {
        return Err(refuse);
    };
    let Some(len) = plain.len().checked_sub(20) else {
        return Err(refuse);
    };
    let main_ok = main & 0x00FF == u32::from(LOGO_CHUNK_TYPE)
        && (main >> 16) & 0x3F == 1
        && (main >> 22) as usize == len + 16;
    let len_ok = (len == 16 || len == 256) && len2 as usize == len;
    let addr_ok = addr >= LOGO_REGION_START
        && (addr - LOGO_REGION_START).is_multiple_of(crate::logo::CHUNK as u32)
        && addr
            .checked_add(len as u32)
            .is_some_and(|end| end <= LOGO_REGION_END);
    let crc_ok = plain
        .get(4..16)
        .is_some_and(|h| crate::crypto::crc32(h) == crc);
    let data = plain.get(20..).unwrap_or(&[]);
    let data_ok = if len == 16 {
        // Remove: only at the region start, only 16 bytes of 0xFF.
        addr == LOGO_REGION_START && data.iter().all(|&b| b == 0xFF)
    } else {
        addr != LOGO_REGION_START || first_chunk_ok(data)
    };
    if main_ok && len_ok && addr_ok && word3 == 0xFFFF_FFFF && crc_ok && data_ok {
        Ok(())
    } else {
        Err(refuse)
    }
}

/// The first 256-byte chunk must open with a valid logo header (the
/// encoder's invariants) whose padded blob fits the logo region.
fn first_chunk_ok(data: &[u8]) -> bool {
    let word = |at: usize| le_word(data, at);
    let (Some(info), Some(count), Some(zero), Some(info2)) = (word(0), word(4), word(8), word(12))
    else {
        return false;
    };
    if zero != 0 || info2 != info || crate::logo::validate_header(info, count).is_err() {
        return false;
    }
    let padded = crate::logo::padded_len(crate::logo::HEADER_LEN + count as usize);
    u32::try_from(padded)
        .ok()
        .and_then(|p| LOGO_REGION_START.checked_add(p))
        .is_some_and(|end| end <= LOGO_REGION_END)
}

/// Accept `frame` only if it is the startup-logo flag write (FR R1ST8Z).
///
/// Layout \[static\]: exactly 12 bytes, main header
/// `0x48 | id << 8 | 3 << 22` (extend bit clear), one extended section header
/// (`att` 0x1C, `next` 0, `chunk` 0, `size` 4), then a u32 value of 0 or 2.
/// Every other frame is refused with [`Error::Forbidden`].
pub fn check_logo_flag_write(frame: &[u8]) -> Result<(), Error> {
    let refuse = Error::Forbidden {
        ty: crate::frame::frame_type(frame).unwrap_or(0x48),
    };
    let word = |at: usize| le_word(frame, at);
    let (Some(main), Some(ext), Some(value), 12) = (word(0), word(4), word(8), frame.len()) else {
        return Err(refuse);
    };
    let main_ok = main & 0x003F_00FF == 0x48 && main >> 22 == 3;
    let ext_ok = ext & 0x003F_FFFF == u32::from(LOGO_FLAG_ATT) && ext >> 22 == 4;
    if main_ok && ext_ok && (value == 0 || value == 2) {
        Ok(())
    } else {
        Err(refuse)
    }
}

/// The little-endian u32 at byte offset `at`, if `bytes` holds all four.
fn le_word(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Wraps a transport and runs [`check`] before every write; each narrow
/// path runs its own check instead: [`check_record_table_write`],
/// [`check_settings_write`], [`check_logo_chunk`] and
/// [`check_logo_flag_write`].
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

    /// Runs [`check_logo_chunk`] instead of [`check`], then hands the
    /// plaintext chunk to the inner transport's logo-chunk path.
    fn transact_logo_chunk(&mut self, plain: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        check_logo_chunk(plain)?;
        self.inner.transact_logo_chunk(plain, timeout)
    }

    /// Runs [`check_logo_flag_write`] instead of [`check`], then hands the
    /// frame to the inner transport's logo-flag path.
    fn transact_logo_flag(&mut self, frame: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        check_logo_flag_write(frame)?;
        self.inner.transact_logo_flag(frame, timeout)
    }

    /// The inner pipe's settings twin, unguarded: whoever opens a session on
    /// it puts its own guard in front.
    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, Error>> {
        self.inner.settings_pipe()
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

    // ---- FR R1ST8Z: guarded logo chunk, flag and read paths ----
    //
    // Layout from ../notes/protocol/logo.md §1 and §4, rewritten in
    // docs/protocol.md: a plaintext chunk frame is the main header
    // `0x4A | id << 8 | 1 << 16 | (len + 16) << 22`, a 16-byte block header
    // (address, len, 0xFFFFFFFF, CRC-32 of the first 12 bytes), then `len`
    // data bytes; bytes 4.. are encrypted with key slot 0 before sending.

    use crate::crypto;

    const LOGO_BASE: u32 = 0x0030_0000;
    const LOGO_END: u32 = 0x0031_D500;
    const LAST_CHUNK: u32 = LOGO_END - 256;

    /// The documented example flag write (id 1, value 2).
    const FLAG_EXAMPLE: [u8; 12] = [
        0x48, 0x01, 0xC0, 0x00, 0x1C, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00,
    ];

    fn le(w: u32) -> [u8; 4] {
        w.to_le_bytes()
    }

    /// Main header word with an explicit `obj`.
    fn logo_main_obj(id: u8, obj: u32) -> u32 {
        0x4A | (u32::from(id) << 8) | (1 << 16) | (obj << 22)
    }

    /// Block header with explicit fields; the CRC is computed.
    fn block_header_with(addr: u32, len2: u32, word3: u32) -> Vec<u8> {
        let mut h = [le(addr), le(len2), le(word3)].concat();
        let crc = crypto::crc32(&h);
        h.extend(le(crc));
        h
    }

    fn block_header(addr: u32, len: u32) -> Vec<u8> {
        block_header_with(addr, len, 0xFFFF_FFFF)
    }

    /// A well-formed plaintext chunk frame for `data` at `addr`.
    fn logo_plain(id: u8, addr: u32, data: &[u8]) -> Vec<u8> {
        let len = data.len() as u32;
        let mut f = le(logo_main_obj(id, len + 16)).to_vec();
        f.extend(block_header(addr, len));
        f.extend_from_slice(data);
        f
    }

    /// 256 bytes of pixel-like data.
    fn pixels() -> Vec<u8> {
        (0..256u32).map(|i| (i * 7 + 3) as u8).collect()
    }

    /// The 16-byte blob header (info, count, 0, info) followed by pixels,
    /// 256 bytes in all.
    fn first_chunk_data(info: u32, count: u32) -> Vec<u8> {
        let mut d = [le(info), le(count), le(0), le(info)].concat();
        d.extend(&pixels()[16..]);
        d
    }

    /// A valid first chunk: opaque `w`×`h` (format 4) or alpha (format 5).
    fn first_chunk(format: u32, w: u32, h: u32) -> Vec<u8> {
        let bpp = if format == 5 { 3 } else { 2 };
        first_chunk_data(crate::logo::info_word(format, w, h), w * h * bpp)
    }

    fn remove_plain(id: u8) -> Vec<u8> {
        logo_plain(id, LOGO_BASE, &[0xFF; 16])
    }

    fn chunk_refused(f: &[u8]) -> bool {
        matches!(check_logo_chunk(f), Err(Error::Forbidden { .. }))
    }

    fn flag_refused(f: &[u8]) -> bool {
        matches!(check_logo_flag_write(f), Err(Error::Forbidden { .. }))
    }

    /// A SET_DATA flag-shaped frame with explicit fields.
    fn flag_frame(id: u8, obj: u32, att: u32, size: u32, value: &[u8]) -> Vec<u8> {
        let mut f = le(0x48 | (u32::from(id) << 8) | (obj << 22)).to_vec();
        f.extend(le(att | (size << 22)));
        f.extend_from_slice(value);
        f
    }

    /// The frame the default path sends for a plaintext chunk.
    fn encrypted(plain: &[u8]) -> Vec<u8> {
        let mut f = plain[..4].to_vec();
        f.extend(crypto::encrypt(&crypto::KEYS[0], &plain[4..]));
        f
    }

    // AC-R1ST8Z.1 / .2: the worked examples of logo.md, byte for byte.

    #[test]
    fn r1st8z_chunk_check_accepts_the_worked_example_frames() {
        let chunk0_header: [u8; 16] = [
            0x00, 0x00, 0x30, 0x00, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x6F,
            0xEA, 0x6B,
        ];
        let chunk1_header: [u8; 16] = [
            0x00, 0x01, 0x30, 0x00, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xC0, 0xB0,
            0x64, 0xAA,
        ];
        let remove_header: [u8; 16] = [
            0x00, 0x00, 0x30, 0x00, 0x10, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x9F, 0x55,
            0x26, 0xB1,
        ];
        let main_256 = [0x4A, 0x01, 0x01, 0x44];
        let main_16 = [0x4A, 0x01, 0x01, 0x08];

        let chunk0 = [&main_256[..], &chunk0_header, &first_chunk(4, 240, 240)].concat();
        let chunk1 = [&main_256[..], &chunk1_header, &pixels()].concat();
        let remove = [&main_16[..], &remove_header, &[0xFF; 16]].concat();
        assert_eq!(chunk0.len(), 4 + 16 + 256);
        assert_eq!(remove.len(), 4 + 16 + 16);
        assert_eq!(check_logo_chunk(&chunk0), Ok(()), "chunk 0");
        assert_eq!(check_logo_chunk(&chunk1), Ok(()), "chunk 1");
        assert_eq!(check_logo_chunk(&remove), Ok(()), "Remove");
        // The test builders agree with the worked examples.
        assert_eq!(logo_plain(1, LOGO_BASE + 256, &pixels()), chunk1);
        assert_eq!(remove_plain(1), remove);
    }

    #[test]
    fn r1st8z_chunk_check_accepts_every_chunk_address_in_the_region() {
        // First chunk (a valid header), every later chunk up to the last
        // allowed one, with any id and any data.
        assert_eq!(
            check_logo_chunk(&logo_plain(9, LOGO_BASE, &first_chunk(5, 200, 200))),
            Ok(())
        );
        let mut addr = LOGO_BASE + 256;
        while addr <= LAST_CHUNK {
            let f = logo_plain((addr >> 8) as u8, addr, &pixels());
            assert_eq!(check_logo_chunk(&f), Ok(()), "chunk at {addr:#x}");
            addr += 256;
        }
        assert_eq!(addr, LOGO_END, "the last accepted chunk ends at 0x31D500");
        assert_eq!(
            check_logo_chunk(&logo_plain(0xFF, LAST_CHUNK, &[0xFF; 256])),
            Ok(()),
            "last chunk, padding bytes"
        );
    }

    #[test]
    fn r1st8z_chunk_check_accepts_valid_logo_headers_at_the_first_chunk() {
        for (format, w, h) in [
            (4, 240, 240),
            (4, 48, 48),
            (4, 2, 1),
            (4, 240, 1),
            (5, 200, 200),
            (5, 48, 64),
        ] {
            let f = logo_plain(1, LOGO_BASE, &first_chunk(format, w, h));
            assert_eq!(check_logo_chunk(&f), Ok(()), "format {format} {w}x{h}");
        }
    }

    // AC-R1ST8Z.1 / .7: every boundary and field refused.

    #[test]
    fn r1st8z_chunk_check_refuses_addresses_outside_the_region() {
        for addr in [
            LOGO_END,          // one chunk past the end
            LOGO_END + 256,    // further past
            LOGO_BASE - 1,     // one byte low
            LOGO_BASE - 256,   // one chunk low
            0x0010_0000,       // the record area
            0x0000_0000,       // the boot loader
            0x9830_0100,       // the read window is not a flash address
            LOGO_BASE + 1,     // not on a 256-byte step
            LOGO_BASE + 0x10,  // not on a 256-byte step
            LAST_CHUNK + 0x80, // straddles the end
            u32::MAX - 255,    // address + length overflows
        ] {
            let f = logo_plain(1, addr, &pixels());
            assert!(chunk_refused(&f), "chunk at {addr:#x}");
        }
    }

    #[test]
    fn r1st8z_chunk_check_refuses_wrong_lengths() {
        // Chunk lengths other than 16 and 256, each with a matching obj and
        // len2 (so only the length itself is wrong).
        for len in [0usize, 1, 15, 17, 32, 208, 240, 255, 257, 512] {
            let f = logo_plain(1, LOGO_BASE + 256, &vec![0xFF; len]);
            assert!(chunk_refused(&f), "length {len}");
        }
        // A 16-byte chunk anywhere but 0x300000.
        for addr in [LOGO_BASE + 256, LOGO_BASE + 16, LAST_CHUNK] {
            assert!(
                chunk_refused(&logo_plain(1, addr, &[0xFF; 16])),
                "16 bytes at {addr:#x}"
            );
        }
        // len2 that does not match the data.
        let mut f = le(logo_main_obj(1, 272)).to_vec();
        f.extend(block_header(LOGO_BASE + 256, 16));
        f.extend(pixels());
        assert!(chunk_refused(&f), "len2 16 with 256 data bytes");
        let mut f = le(logo_main_obj(1, 32)).to_vec();
        f.extend(block_header(LOGO_BASE, 256));
        f.extend([0xFF; 16]);
        assert!(chunk_refused(&f), "len2 256 with 16 data bytes");
        // Truncated and over-long frames.
        let good = logo_plain(1, LOGO_BASE + 256, &pixels());
        assert!(chunk_refused(&good[..good.len() - 1]), "one byte short");
        let mut long = good.clone();
        long.push(0xFF);
        assert!(chunk_refused(&long), "one byte too many");
        assert!(chunk_refused(&good[..20]), "header only");
        assert!(chunk_refused(&good[..4]), "main header only");
        assert!(chunk_refused(&[]), "empty");
    }

    #[test]
    fn r1st8z_chunk_check_refuses_bad_main_headers() {
        let good = logo_plain(1, LOGO_BASE + 256, &pixels());
        let header = |f: &[u8]| u32::from_le_bytes([f[0], f[1], f[2], f[3]]);
        let with_main = |w: u32| {
            let mut f = good.clone();
            f[..4].copy_from_slice(&le(w));
            f
        };
        let h = header(&good);
        for (what, w) in [
            ("obj 256", logo_main_obj(1, 256)),
            ("obj 273", logo_main_obj(1, 273)),
            ("obj 68 (words)", logo_main_obj(1, 68)),
            ("obj 0", logo_main_obj(1, 0)),
            ("bit 16 clear", h & !(1 << 16)),
            ("bit 17 set", h | (1 << 17)),
            ("bit 21 set", h | (1 << 21)),
            ("extend bit", h | 0x80),
            ("type 0x48", (h & !0x7F) | 0x48),
            ("type 0x44", (h & !0x7F) | 0x44),
            ("type 0x42", (h & !0x7F) | 0x42),
        ] {
            assert!(chunk_refused(&with_main(w)), "{what}");
        }
        // obj for the 16-byte chunk must be 32.
        let mut f = remove_plain(1);
        f[..4].copy_from_slice(&le(logo_main_obj(1, 16)));
        assert!(chunk_refused(&f), "Remove with obj 16");
    }

    #[test]
    fn r1st8z_chunk_check_refuses_bad_block_headers() {
        let data = pixels();
        let addr = LOGO_BASE + 256;
        let with_header = |h: Vec<u8>| {
            let mut f = le(logo_main_obj(1, 272)).to_vec();
            f.extend(h);
            f.extend(&data);
            f
        };
        assert_eq!(
            check_logo_chunk(&with_header(block_header(addr, 256))),
            Ok(())
        );
        // Bad CRC: one bit flipped in the CRC, or in a covered field.
        let mut h = block_header(addr, 256);
        h[12] ^= 0x01;
        assert!(chunk_refused(&with_header(h)), "CRC bit flipped");
        let mut h = block_header(addr, 256);
        h[15] ^= 0x80;
        assert!(chunk_refused(&with_header(h)), "CRC top bit flipped");
        let mut h = block_header(addr, 256);
        h[1] ^= 0x02; // address changed, CRC left as it was
        assert!(
            chunk_refused(&with_header(h)),
            "address changed under the CRC"
        );
        let mut h = block_header(addr, 256);
        h[12..16].copy_from_slice(&[0; 4]);
        assert!(chunk_refused(&with_header(h)), "CRC zero");
        // Word three other than 0xFFFFFFFF, CRC made right.
        for w3 in [0u32, 0xFFFF_FFFE, 0x7FFF_FFFF, 0x0000_00FF] {
            assert!(
                chunk_refused(&with_header(block_header_with(addr, 256, w3))),
                "word three {w3:#x}"
            );
        }
    }

    #[test]
    fn r1st8z_chunk_check_refuses_a_16_byte_chunk_other_than_remove() {
        for (what, data) in [
            ("one byte not 0xFF", {
                let mut d = [0xFF; 16];
                d[15] = 0xFE;
                d
            }),
            ("zeros", [0u8; 16]),
            ("a logo header", {
                let i = crate::logo::info_word(4, 48, 48);
                let d = [le(i), le(48 * 48 * 2), le(0), le(i)].concat();
                <[u8; 16]>::try_from(d).unwrap()
            }),
        ] {
            assert!(chunk_refused(&logo_plain(1, LOGO_BASE, &data)), "{what}");
        }
    }

    #[test]
    fn r1st8z_chunk_check_refuses_a_first_chunk_without_a_valid_logo_header() {
        let i = crate::logo::info_word(4, 240, 240);
        let n = 240 * 240 * 2;
        let mut bad: Vec<(&str, Vec<u8>)> = vec![
            ("all 0xFF", vec![0xFF; 256]),
            ("all zero", vec![0; 256]),
            ("any pixels", pixels()),
            (
                "format 6",
                first_chunk_data(crate::logo::info_word(6, 240, 240), n),
            ),
            (
                "format 0",
                first_chunk_data(crate::logo::info_word(0, 240, 240), n),
            ),
            ("reserved bits", first_chunk_data(i | 0x20, n)),
            ("odd width", first_chunk(4, 239, 240)),
            ("width 0", first_chunk(4, 0, 240)),
            ("opaque 242 wide", first_chunk(4, 242, 240)),
            ("opaque 241 high", first_chunk(4, 240, 241)),
            ("alpha 202 wide", first_chunk(5, 202, 200)),
            ("alpha 201 high", first_chunk(5, 200, 201)),
            ("count one short", first_chunk_data(i, n - 1)),
            ("count one over", first_chunk_data(i, n + 1)),
            (
                "count of the other mode",
                first_chunk_data(i, 240 * 240 * 3),
            ),
        ];
        let mut d = first_chunk(4, 240, 240);
        d[8] = 1;
        bad.push(("word two not zero", d));
        let mut d = first_chunk(4, 240, 240);
        d[12..16].copy_from_slice(&le(crate::logo::info_word(4, 238, 240)));
        bad.push(("second info word differs", d));
        for (what, data) in bad {
            assert!(
                chunk_refused(&logo_plain(1, LOGO_BASE, &data)),
                "first chunk: {what}"
            );
        }
    }

    // AC-R1ST8Z.3: the logo flag write.

    #[test]
    fn r1st8z_flag_check_accepts_values_0_and_2_only() {
        assert_eq!(check_logo_flag_write(&FLAG_EXAMPLE), Ok(()));
        assert_eq!(flag_frame(1, 3, 0x1C, 4, &le(2)), FLAG_EXAMPLE);
        for id in [0u8, 1, 0x7F, 0xFF] {
            for v in [0u32, 2] {
                assert_eq!(
                    check_logo_flag_write(&flag_frame(id, 3, 0x1C, 4, &le(v))),
                    Ok(()),
                    "id {id} value {v}"
                );
            }
        }
        for v in [1u32, 3, 4, 0x100, 0x0200_0000, u32::MAX] {
            assert!(
                flag_refused(&flag_frame(1, 3, 0x1C, 4, &le(v))),
                "value {v:#x}"
            );
        }
    }

    #[test]
    fn r1st8z_flag_check_refuses_malformed_frames() {
        let good = FLAG_EXAMPLE.to_vec();
        let mut bad: Vec<(&str, Vec<u8>)> = vec![
            ("attribute 0x1B", flag_frame(1, 3, 0x1B, 4, &le(2))),
            ("attribute 0x1D", flag_frame(1, 3, 0x1D, 4, &le(2))),
            ("attribute 0x28", flag_frame(1, 3, 0x28, 4, &le(2))),
            (
                "attribute 0x0A (calibration)",
                flag_frame(1, 3, 0x0A, 4, &le(2)),
            ),
            (
                "size 8",
                flag_frame(1, 4, 0x1C, 8, &[le(2), le(0)].concat()),
            ),
            ("size 2", flag_frame(1, 3, 0x1C, 2, &le(2))),
            ("obj 4", flag_frame(1, 4, 0x1C, 4, &le(2))),
            ("obj 2", flag_frame(1, 2, 0x1C, 4, &le(2))),
            ("next bit", flag_frame(1, 3, 0x1C | (1 << 15), 4, &le(2))),
            ("chunk bits", flag_frame(1, 3, 0x1C | (1 << 16), 4, &le(2))),
            ("truncated", good[..11].to_vec()),
            ("no value", good[..8].to_vec()),
            ("header only", good[..4].to_vec()),
            ("empty", Vec::new()),
        ];
        let mut f = good.clone();
        f.push(0);
        bad.push(("trailing byte", f));
        let mut f = good.clone();
        f[0] |= 0x80;
        bad.push(("extend bit", f));
        let mut f = good.clone();
        f[2] |= 0x01;
        bad.push(("main bit 16", f));
        let mut f = good.clone();
        f[0] = 0x4A;
        bad.push(("type 0x4A", f));
        let mut f = good.clone();
        f[0] = 0x0A;
        bad.push(("type 0x0A", f));
        for (what, f) in bad {
            assert!(flag_refused(&f), "{what}: {f:02x?}");
        }
    }

    // AC-R1ST8Z.4: the shared check and every other path refuse them.

    #[test]
    fn r1st8z_shared_check_still_refuses_every_logo_frame() {
        assert!(FORBIDDEN_TYPES.contains(&0x4A), "0x4A stays forbidden");
        assert!(FORBIDDEN_TYPES.contains(&0x48), "0x48 stays forbidden");
        let chunk = logo_plain(1, LOGO_BASE, &first_chunk(4, 240, 240));
        for f in [chunk.clone(), encrypted(&chunk), remove_plain(1)] {
            assert_eq!(check(&f), Err(Error::Forbidden { ty: 0x4A }));
        }
        for v in [0u32, 2] {
            assert_eq!(
                check(&flag_frame(1, 3, 0x1C, 4, &le(v))),
                Err(Error::Forbidden { ty: 0x48 })
            );
        }
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        assert_eq!(g.transact(&chunk, T), Err(Error::Forbidden { ty: 0x4A }));
        assert_eq!(
            g.transact(&encrypted(&chunk), T),
            Err(Error::Forbidden { ty: 0x4A })
        );
        assert_eq!(
            g.transact(&FLAG_EXAMPLE, T),
            Err(Error::Forbidden { ty: 0x48 })
        );
        assert!(g.inner().written().is_empty());
    }

    #[test]
    fn r1st8z_each_path_refuses_the_other_paths_frames() {
        let chunk = logo_plain(1, LOGO_BASE + 256, &pixels());
        let remove = remove_plain(1);
        let flag = FLAG_EXAMPLE.to_vec();
        let record = set_data(1, 0x28, &table(1));
        let settings = settings_frame(1, &good_entries());
        // Sanity: each frame passes its own check.
        assert_eq!(check_logo_chunk(&chunk), Ok(()));
        assert_eq!(check_logo_chunk(&remove), Ok(()));
        assert_eq!(check_logo_flag_write(&flag), Ok(()));
        assert_eq!(check_record_table_write(&record), Ok(()));
        assert_eq!(check_settings_write(&settings), Ok(()));

        let enc = encrypted(&chunk);
        type Check = fn(&[u8]) -> Result<(), Error>;
        let table: [(&str, Check, Vec<&[u8]>); 4] = [
            (
                "logo chunk",
                check_logo_chunk,
                vec![&enc, &flag, &record, &settings],
            ),
            (
                "logo flag",
                check_logo_flag_write,
                vec![&chunk, &remove, &enc, &record, &settings],
            ),
            (
                "record table",
                check_record_table_write,
                vec![&chunk, &remove, &enc, &flag, &settings],
            ),
            (
                "settings",
                check_settings_write,
                vec![&chunk, &remove, &enc, &flag, &record],
            ),
        ];
        for (path, check_fn, others) in table {
            for f in others {
                assert!(
                    matches!(check_fn(f), Err(Error::Forbidden { .. })),
                    "{path} path took a foreign frame: {f:02x?}"
                );
            }
        }
        // The same through a guard: nothing reaches the pipe.
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        for f in [&flag, &record, &settings] {
            assert!(matches!(
                g.transact_logo_chunk(f, T),
                Err(Error::Forbidden { .. })
            ));
        }
        for f in [&chunk, &remove, &record, &settings] {
            assert!(matches!(
                g.transact_logo_flag(f, T),
                Err(Error::Forbidden { .. })
            ));
        }
        for f in [&chunk, &remove, &flag] {
            assert!(matches!(
                g.transact_record_table(f, T),
                Err(Error::Forbidden { .. })
            ));
            assert!(matches!(
                g.transact_settings(f, T),
                Err(Error::Forbidden { .. })
            ));
        }
        assert!(g.inner().written().is_empty());
    }

    // AC-R1ST8Z.2 / .6: the default path encrypts after the check; guards
    // and wrappers forward to the inner path.

    #[test]
    fn r1st8z_default_chunk_path_sends_the_encrypted_frame() {
        let finished = vec![0x07, 0x01, 0x00, 0x00];
        for plain in [
            logo_plain(1, LOGO_BASE, &first_chunk(4, 240, 240)),
            logo_plain(2, LOGO_BASE + 256, &pixels()),
            remove_plain(3),
        ] {
            let want = encrypted(&plain);
            let mut m = MockTransport::new(vec![Step::transact(want.clone(), finished.clone())]);
            assert_eq!(m.transact_logo_chunk(&plain, T), Ok(finished.clone()));
            let sent = &m.written()[0];
            assert_eq!(m.written().len(), 1);
            assert_eq!(sent[..4], plain[..4], "main header stays plain");
            assert_ne!(sent[4..], plain[4..], "payload is encrypted");
            assert_eq!(sent.len() % 16, 4, "whole AES blocks after the header");
            assert_eq!(sent.len(), plain.len());
            assert_eq!(crypto::decrypt(&crypto::KEYS[0], &sent[4..]), plain[4..]);
        }
    }

    #[test]
    fn r1st8z_default_chunk_path_checks_the_plaintext_before_sending() {
        let mut m = MockTransport::new(vec![]);
        let bad = logo_plain(1, LOGO_END, &pixels());
        assert!(matches!(
            m.transact_logo_chunk(&bad, T),
            Err(Error::Forbidden { .. })
        ));
        // An already encrypted frame is not a valid plaintext chunk.
        let good = logo_plain(1, LOGO_BASE + 256, &pixels());
        assert!(matches!(
            m.transact_logo_chunk(&encrypted(&good), T),
            Err(Error::Forbidden { .. })
        ));
        assert!(matches!(
            m.transact_logo_chunk(&FLAG_EXAMPLE, T),
            Err(Error::Forbidden { .. })
        ));
        assert!(m.written().is_empty());
    }

    #[test]
    fn r1st8z_default_flag_path_sends_the_frame_as_is() {
        let accept = vec![0x05, 0x01, 0x00, 0x00];
        let mut m = MockTransport::new(vec![Step::transact(FLAG_EXAMPLE, accept.clone())]);
        assert_eq!(m.transact_logo_flag(&FLAG_EXAMPLE, T), Ok(accept));
        assert_eq!(m.written(), &[FLAG_EXAMPLE.to_vec()]);
        let mut m = MockTransport::new(vec![]);
        assert!(matches!(
            m.transact_logo_flag(&flag_frame(1, 3, 0x1C, 4, &le(1)), T),
            Err(Error::Forbidden { .. })
        ));
        assert!(m.written().is_empty());
    }

    #[test]
    fn r1st8z_guarded_transport_passes_logo_frames_on_their_own_paths() {
        let finished = vec![0x07, 0x04, 0x00, 0x00];
        let plain = logo_plain(4, LOGO_BASE, &first_chunk(5, 200, 200));
        let mut g = GuardedTransport::new(MockTransport::new(vec![
            Step::transact(encrypted(&plain), finished.clone()),
            Step::transact(FLAG_EXAMPLE, finished.clone()),
        ]));
        assert_eq!(g.transact_logo_chunk(&plain, T), Ok(finished.clone()));
        assert_eq!(g.transact_logo_flag(&FLAG_EXAMPLE, T), Ok(finished));
        assert_eq!(
            g.inner().written(),
            &[encrypted(&plain), FLAG_EXAMPLE.to_vec()]
        );
    }

    #[test]
    fn r1st8z_guarded_transport_refuses_bad_logo_frames_without_writing() {
        let mut g = GuardedTransport::new(MockTransport::new(vec![]));
        assert!(matches!(
            g.transact_logo_chunk(&logo_plain(1, LOGO_END, &pixels()), T),
            Err(Error::Forbidden { .. })
        ));
        assert!(matches!(
            g.transact_logo_chunk(&logo_plain(1, LOGO_BASE, &pixels()), T),
            Err(Error::Forbidden { .. })
        ));
        assert!(matches!(
            g.transact_logo_flag(&flag_frame(1, 3, 0x1C, 4, &le(3)), T),
            Err(Error::Forbidden { .. })
        ));
        assert!(g.inner().written().is_empty());
    }

    /// An inner pipe that records which path each frame arrived on.
    #[derive(Default)]
    struct PathProbe {
        calls: Vec<(&'static str, Vec<u8>)>,
    }

    impl Transport for PathProbe {
        fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, Error> {
            self.calls.push(("transact", request.to_vec()));
            Ok(vec![0x07, 0x00, 0x00, 0x00])
        }
        fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, Error> {
            Err(Error::Timeout)
        }
        fn transact_logo_chunk(&mut self, plain: &[u8], _t: Duration) -> Result<Vec<u8>, Error> {
            self.calls.push(("logo_chunk", plain.to_vec()));
            Ok(vec![0x07, 0x00, 0x00, 0x00])
        }
        fn transact_logo_flag(&mut self, frame: &[u8], _t: Duration) -> Result<Vec<u8>, Error> {
            self.calls.push(("logo_flag", frame.to_vec()));
            Ok(vec![0x05, 0x00, 0x00, 0x00])
        }
    }

    #[test]
    fn r1st8z_guarded_transport_forwards_to_the_inner_logo_paths() {
        let plain = logo_plain(1, LOGO_BASE + 256, &pixels());
        let mut g = GuardedTransport::new(PathProbe::default());
        g.transact_logo_chunk(&plain, T).unwrap();
        g.transact_logo_flag(&FLAG_EXAMPLE, T).unwrap();
        assert_eq!(
            g.inner().calls,
            vec![("logo_chunk", plain), ("logo_flag", FLAG_EXAMPLE.to_vec())]
        );
        // A refused frame never reaches the inner path.
        let _ = g.transact_logo_chunk(&logo_plain(1, LOGO_END, &pixels()), T);
        let _ = g.transact_logo_flag(&flag_frame(1, 3, 0x1C, 4, &le(1)), T);
        assert_eq!(g.inner().calls.len(), 2);
    }

    #[test]
    fn r1st8z_boxed_transport_forwards_the_logo_paths() {
        let plain = logo_plain(1, LOGO_BASE + 256, &pixels());
        let mut b: Box<PathProbe> = Box::default();
        Transport::transact_logo_chunk(&mut b, &plain, T).unwrap();
        Transport::transact_logo_flag(&mut b, &FLAG_EXAMPLE, T).unwrap();
        assert_eq!(
            b.calls,
            vec![("logo_chunk", plain), ("logo_flag", FLAG_EXAMPLE.to_vec())]
        );
    }

    #[test]
    fn r1st8z_nested_guards_each_check_the_logo_paths() {
        let plain = logo_plain(4, LAST_CHUNK, &pixels());
        let finished = vec![0x07, 0x04, 0x00, 0x00];
        let mut g = GuardedTransport::new(GuardedTransport::new(MockTransport::new(vec![
            Step::transact(encrypted(&plain), finished.clone()),
            Step::transact(FLAG_EXAMPLE, finished.clone()),
        ])));
        assert_eq!(g.transact_logo_chunk(&plain, T), Ok(finished.clone()));
        assert_eq!(g.transact_logo_flag(&FLAG_EXAMPLE, T), Ok(finished));
        assert!(matches!(
            g.transact_logo_chunk(&logo_plain(4, LOGO_END, &pixels()), T),
            Err(Error::Forbidden { .. })
        ));
        assert_eq!(
            g.inner().inner().written(),
            &[encrypted(&plain), FLAG_EXAMPLE.to_vec()]
        );
        // The inner guard checks on its own: a frame handed straight to it
        // is checked there too.
        let mut inner = GuardedTransport::new(PathProbe::default());
        let _ = inner.transact_logo_flag(&flag_frame(1, 3, 0x1C, 4, &le(3)), T);
        assert!(inner.inner().calls.is_empty());
    }
}
