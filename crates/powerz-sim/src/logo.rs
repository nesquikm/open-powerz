//! Simulated startup-logo flash of the demo meter (FR A2XTES).
//!
//! A sparse model of the meter's SPI flash around the logo region, the
//! startup-logo flag written by SET_DATA 0x1C, per-test faults for the 0x4A
//! chunk writes and a helper that says what the screen would show at boot.
//! How the real meter erases, checks chunks and picks its boot screen is
//! not known yet; every rule here is ours `[inferred]`.

use std::collections::BTreeMap;
use std::time::Duration;

use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::logo::{self, Rgba};
use powerz_protocol::transport::guard::{LOGO_REGION_END, LOGO_REGION_START};

/// Size of one flash sector the erase policies work on.
pub const SECTOR_LEN: usize = 4096;

/// How the simulated flash erases before a write (the real meter's
/// behaviour is unknown until hardware stage 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErasePolicy {
    /// A write that starts a 4 KiB sector erases that sector first.
    SectorOnFirstWrite,
    /// A write at the logo region start (0x300000) erases the whole
    /// logo region first.
    RegionOnStartWrite,
    /// Nothing is ever erased: new bytes AND into the old ones.
    NoErase,
}

/// A fault injected into the 0x4A chunk writes. `k` counts 0x4A frames
/// from 0, starting at the first one after the fault was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoFault {
    /// Chunk `k` gets no reply.
    NoReplyAt(usize),
    /// Chunk `k` is answered with REJECT and not written.
    RejectAt(usize),
    /// Chunk `k` is answered; every later call fails as a disconnected
    /// device.
    UnplugAfter(usize),
}

/// Sparse flash: unwritten bytes read 0xFF.
#[derive(Debug, Clone)]
pub struct LogoFlash {
    sectors: BTreeMap<u32, [u8; SECTOR_LEN]>,
    policy: ErasePolicy,
}

impl LogoFlash {
    /// Empty (all 0xFF) flash erasing by `policy`.
    pub fn new(policy: ErasePolicy) -> LogoFlash {
        LogoFlash {
            sectors: BTreeMap::new(),
            policy,
        }
    }

    /// The erase policy.
    pub fn policy(&self) -> ErasePolicy {
        self.policy
    }

    /// `len` bytes at flash address `addr`.
    pub fn read(&self, addr: u32, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| {
                let a = addr.wrapping_add(i as u32);
                self.sectors
                    .get(&sector_of(a))
                    .map_or(0xFF, |s| s[offset_in(a)])
            })
            .collect()
    }

    /// Program `data` at flash address `addr` under the erase policy.
    /// Programming can only clear bits: new bytes AND into the old ones.
    pub fn write(&mut self, addr: u32, data: &[u8]) {
        let end = addr.saturating_add(data.len() as u32);
        match self.policy {
            ErasePolicy::SectorOnFirstWrite => {
                // Erase every sector whose start this write covers.
                let first = sector_of(addr.saturating_add(SECTOR_LEN as u32 - 1));
                let starts = (first..end).step_by(SECTOR_LEN).collect::<Vec<_>>();
                for s in starts {
                    self.sectors.remove(&s);
                }
            }
            ErasePolicy::RegionOnStartWrite if addr == LOGO_REGION_START && !data.is_empty() => {
                self.sectors
                    .retain(|&s, _| !(LOGO_REGION_START..LOGO_REGION_END).contains(&s));
            }
            _ => {}
        }
        for (i, &b) in data.iter().enumerate() {
            let a = addr.wrapping_add(i as u32);
            let sector = self
                .sectors
                .entry(sector_of(a))
                .or_insert([0xFF; SECTOR_LEN]);
            sector[offset_in(a)] &= b;
        }
    }
}

/// Base address of the sector holding `addr`.
fn sector_of(addr: u32) -> u32 {
    addr - addr % SECTOR_LEN as u32
}

/// Offset of `addr` inside its sector.
fn offset_in(addr: u32) -> usize {
    addr as usize % SECTOR_LEN
}

/// Block header of a 0x4A chunk: flash address, second length,
/// 0xFFFFFFFF, CRC-32 of the first 12 bytes.
const BLOCK_HEADER_LEN: usize = 16;

/// Decrypt the payload of a 0x4A frame (bytes 4.. of `req`) with key
/// slot 0 and check its block header (CRC, 0xFFFFFFFF, second length
/// equal to the data length). Returns the flash address and data, or
/// `None` for a frame the meter would REJECT `[inferred]`.
pub(crate) fn decode_chunk(req: &[u8]) -> Option<(u32, Vec<u8>)> {
    let payload = req.get(4..)?;
    if payload.len() < BLOCK_HEADER_LEN || !payload.len().is_multiple_of(16) {
        return None;
    }
    let plain = crypto::decrypt(&KEYS[0], payload);
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(plain.get(at..at + 4)?.try_into().ok()?))
    };
    let (addr, len2, word3, crc) = (word(0)?, word(4)?, word(8)?, word(12)?);
    let data = plain.get(BLOCK_HEADER_LEN..)?;
    let ok = crc == crypto::crc32(plain.get(..12)?)
        && word3 == 0xFFFF_FFFF
        && len2 as usize == data.len();
    ok.then(|| (addr, data.to_vec()))
}

/// What the meter's screen shows at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootScreen {
    /// No startup screen.
    Nothing,
    /// The built-in "POWER-Z" logo.
    BuiltIn,
    /// The custom image decoded from flash.
    Custom(Rgba),
}

/// What the screen shows at boot for `flash`, `flag` and the startup
/// screen `duration`. The rules are ours, not the firmware's:
///
/// - A zero `duration` (startup screen off) shows nothing `[inferred]`.
/// - Flag 0, any flag other than 2, or flag 2 with a blob at 0x300000
///   that breaks the encoder's invariants (header, duplicated info word,
///   zero field, byte count, 0xFF padding) shows the built-in logo
///   `[inferred]`.
/// - Flag 2 with a valid blob shows the image decoded from flash
///   `[inferred]`.
pub fn boot_render(flash: &LogoFlash, flag: u32, duration: Duration) -> BootScreen {
    if duration.is_zero() {
        return BootScreen::Nothing;
    }
    if flag != 2 {
        return BootScreen::BuiltIn;
    }
    custom_image(flash).map_or(BootScreen::BuiltIn, BootScreen::Custom)
}

/// The valid logo blob at the region start, decoded, if there is one.
fn custom_image(flash: &LogoFlash) -> Option<Rgba> {
    let header = flash.read(LOGO_REGION_START, logo::HEADER_LEN);
    let count = u32::from_le_bytes(header.get(4..8)?.try_into().ok()?) as usize;
    let total = logo::HEADER_LEN.checked_add(count)?;
    // The region is a whole number of chunks, so a blob that fits pads
    // into it too.
    if total > (LOGO_REGION_END - LOGO_REGION_START) as usize {
        return None;
    }
    let data = flash.read(LOGO_REGION_START, logo::padded_len(total));
    logo::validate_blob(&data).ok()?;
    logo::decode(&data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use powerz_protocol::logo::{self, LogoMode};

    const BASE: u32 = LOGO_REGION_START;
    const SEC: u32 = SECTOR_LEN as u32;
    const ON: Duration = Duration::from_secs(3);

    fn image(w: u32, h: u32) -> Rgba {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                for c in 0..3u32 {
                    px.push(((x * 3 + y * 5 + c * 17) % 251) as u8);
                }
                px.push(255);
            }
        }
        Rgba { w, h, px }
    }

    /// A 48×48 opaque logo: 4624 bytes, padded 4864 (19 chunks, 2 sectors).
    fn small_blob() -> logo::LogoBlob {
        logo::encode(&image(48, 48), LogoMode::Opaque).unwrap()
    }

    fn write_chunked(flash: &mut LogoFlash, data: &[u8]) {
        for (k, chunk) in data.chunks(logo::CHUNK).enumerate() {
            flash.write(BASE + (k * logo::CHUNK) as u32, chunk);
        }
    }

    fn with_logo(policy: ErasePolicy, blob: &logo::LogoBlob) -> LogoFlash {
        let mut f = LogoFlash::new(policy);
        write_chunked(&mut f, &blob.padded());
        f
    }

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) & 0x7E)
            .collect()
    }

    // ---- AC-A2XTES.2: sparse flash and the three erase policies ----

    #[test]
    fn a2xtes_fresh_flash_reads_ff_everywhere() {
        for policy in [
            ErasePolicy::SectorOnFirstWrite,
            ErasePolicy::RegionOnStartWrite,
            ErasePolicy::NoErase,
        ] {
            let f = LogoFlash::new(policy);
            assert_eq!(f.policy(), policy);
            assert_eq!(f.read(BASE, 256), vec![0xFF; 256], "{policy:?}");
            assert_eq!(f.read(0, 16), vec![0xFF; 16], "{policy:?}");
            assert_eq!(f.read(LOGO_REGION_END - 256, 512), vec![0xFF; 512]);
            // A read across a sector boundary is still all 0xFF.
            assert_eq!(f.read(BASE + SEC - 8, 16), vec![0xFF; 16]);
        }
    }

    #[test]
    fn a2xtes_write_then_read_round_trips_across_sectors() {
        for policy in [
            ErasePolicy::SectorOnFirstWrite,
            ErasePolicy::RegionOnStartWrite,
            ErasePolicy::NoErase,
        ] {
            let blob = small_blob();
            let f = with_logo(policy, &blob);
            let padded = blob.padded();
            assert_eq!(f.read(BASE, padded.len()), padded, "{policy:?}");
            // Bytes past the logo stay erased.
            assert_eq!(f.read(BASE + padded.len() as u32, 256), vec![0xFF; 256]);
        }
    }

    #[test]
    fn a2xtes_no_erase_ands_new_bytes_into_old() {
        let mut f = LogoFlash::new(ErasePolicy::NoErase);
        f.write(BASE, &[0xF0; 16]);
        f.write(BASE, &[0x3C; 16]);
        assert_eq!(f.read(BASE, 16), vec![0x30; 16]);
        // Writing 0xFF changes nothing.
        f.write(BASE, &[0xFF; 16]);
        assert_eq!(f.read(BASE, 16), vec![0x30; 16]);
        assert_eq!(f.read(BASE + 16, 16), vec![0xFF; 16]);
    }

    #[test]
    fn a2xtes_sector_policy_erases_a_sector_when_a_write_starts_it() {
        let mut f = LogoFlash::new(ErasePolicy::SectorOnFirstWrite);
        let old = pattern(2 * SECTOR_LEN, 5);
        write_chunked(&mut f, &old);
        assert_eq!(f.read(BASE, old.len()), old);
        // A 16-byte write at the sector start erases sector 0 only.
        f.write(BASE, &[0x11; 16]);
        assert_eq!(f.read(BASE, 16), vec![0x11; 16]);
        assert_eq!(
            f.read(BASE + 16, SECTOR_LEN - 16),
            vec![0xFF; SECTOR_LEN - 16]
        );
        assert_eq!(f.read(BASE + SEC, SECTOR_LEN), old[SECTOR_LEN..].to_vec());
        // A write inside a sector (not at its start) erases nothing: AND.
        f.write(BASE + SEC + 256, &[0x0F; 256]);
        let want: Vec<u8> = old[SECTOR_LEN + 256..SECTOR_LEN + 512]
            .iter()
            .map(|b| b & 0x0F)
            .collect();
        assert_eq!(f.read(BASE + SEC + 256, 256), want);
        assert_eq!(
            f.read(BASE + SEC, 256),
            old[SECTOR_LEN..SECTOR_LEN + 256].to_vec()
        );
    }

    #[test]
    fn a2xtes_region_policy_erases_the_whole_region_on_a_write_at_the_start() {
        let mut f = LogoFlash::new(ErasePolicy::RegionOnStartWrite);
        let below = pattern(256, 9);
        f.write(BASE - 256, &below);
        let old = pattern(3 * SECTOR_LEN, 1);
        write_chunked(&mut f, &old);
        let tail = pattern(256, 3);
        f.write(LOGO_REGION_END - 256, &tail);
        // A write elsewhere in the region erases nothing: AND.
        f.write(BASE + SEC, &[0x0F; 256]);
        let want: Vec<u8> = old[SECTOR_LEN..SECTOR_LEN + 256]
            .iter()
            .map(|b| b & 0x0F)
            .collect();
        assert_eq!(f.read(BASE + SEC, 256), want);
        // A write at 0x300000 wipes the region, then programs its bytes.
        f.write(BASE, &[0xFF; 16]);
        let span = (LOGO_REGION_END - BASE) as usize;
        assert_eq!(f.read(BASE, span), vec![0xFF; span]);
        // Flash below the region is untouched.
        assert_eq!(f.read(BASE - 256, 256), below);
    }

    #[test]
    fn a2xtes_large_logo_then_remove_under_each_policy() {
        let blob = logo::encode(&image(240, 240), LogoMode::Opaque).unwrap();
        let padded = blob.padded();
        let n = padded.len();
        let remove = |f: &mut LogoFlash| f.write(BASE, &[0xFF; 16]);

        let mut f = with_logo(ErasePolicy::SectorOnFirstWrite, &blob);
        remove(&mut f);
        assert_eq!(f.read(BASE, SECTOR_LEN), vec![0xFF; SECTOR_LEN]);
        assert_eq!(
            f.read(BASE + SEC, n - SECTOR_LEN),
            padded[SECTOR_LEN..].to_vec(),
            "old pixels stay above the first sector"
        );

        let mut f = with_logo(ErasePolicy::RegionOnStartWrite, &blob);
        remove(&mut f);
        assert_eq!(f.read(BASE, n), vec![0xFF; n]);

        let mut f = with_logo(ErasePolicy::NoErase, &blob);
        remove(&mut f);
        assert_eq!(f.read(BASE, n), padded, "0xFF ANDs into nothing");
    }

    // ---- AC-A2XTES.7: boot render ----

    #[test]
    fn a2xtes_boot_render_shows_nothing_when_the_duration_is_off() {
        let f = with_logo(ErasePolicy::SectorOnFirstWrite, &small_blob());
        assert_eq!(boot_render(&f, 2, Duration::ZERO), BootScreen::Nothing);
        assert_eq!(boot_render(&f, 0, Duration::ZERO), BootScreen::Nothing);
        let empty = LogoFlash::new(ErasePolicy::NoErase);
        assert_eq!(boot_render(&empty, 0, Duration::ZERO), BootScreen::Nothing);
    }

    #[test]
    fn a2xtes_boot_render_shows_built_in_for_flag_0() {
        let f = with_logo(ErasePolicy::SectorOnFirstWrite, &small_blob());
        assert_eq!(boot_render(&f, 0, ON), BootScreen::BuiltIn);
        assert_eq!(
            boot_render(&f, 0, Duration::from_secs(1)),
            BootScreen::BuiltIn
        );
        let empty = LogoFlash::new(ErasePolicy::NoErase);
        assert_eq!(boot_render(&empty, 0, ON), BootScreen::BuiltIn);
    }

    #[test]
    fn a2xtes_boot_render_shows_the_custom_image_for_flag_2() {
        for blob in [
            small_blob(),
            logo::encode(&image(64, 40), LogoMode::Alpha).unwrap(),
        ] {
            let f = with_logo(ErasePolicy::SectorOnFirstWrite, &blob);
            let want = logo::decode(&blob.bytes).unwrap();
            assert_eq!(boot_render(&f, 2, ON), BootScreen::Custom(want));
        }
    }

    #[test]
    fn a2xtes_boot_render_falls_back_to_built_in_for_a_broken_header() {
        // Empty flash: header all 0xFF.
        let empty = LogoFlash::new(ErasePolicy::NoErase);
        assert_eq!(boot_render(&empty, 2, ON), BootScreen::BuiltIn);

        // Remove under the sector policy: header erased, old pixels behind.
        let mut f = with_logo(ErasePolicy::SectorOnFirstWrite, &small_blob());
        f.write(BASE, &[0xFF; 16]);
        assert_eq!(boot_render(&f, 2, ON), BootScreen::BuiltIn);

        // Headers that break the encoder's invariants.
        let good = small_blob().padded();
        let word = |w: u32| w.to_le_bytes();
        let info = logo::info_word(4, 48, 48);
        let broken: Vec<(&str, [u8; 16])> = vec![
            ("second info word differs", {
                let mut h = [0u8; 16];
                h[..4].copy_from_slice(&word(info));
                h[4..8].copy_from_slice(&word(48 * 48 * 2));
                h[12..].copy_from_slice(&word(logo::info_word(4, 50, 48)));
                h
            }),
            ("zero field set", {
                let mut h: [u8; 16] = good[..16].try_into().unwrap();
                h[8] = 1;
                h
            }),
            ("format 6", {
                let mut h: [u8; 16] = good[..16].try_into().unwrap();
                let bad = logo::info_word(6, 48, 48);
                h[..4].copy_from_slice(&word(bad));
                h[12..].copy_from_slice(&word(bad));
                h
            }),
            ("byte count off by two", {
                let mut h: [u8; 16] = good[..16].try_into().unwrap();
                h[4..8].copy_from_slice(&word(48 * 48 * 2 - 2));
                h
            }),
            ("side over the limit", {
                let bad = logo::info_word(4, 242, 48);
                let mut h = [0u8; 16];
                h[..4].copy_from_slice(&word(bad));
                h[4..8].copy_from_slice(&word(242 * 48 * 2));
                h[12..].copy_from_slice(&word(bad));
                h
            }),
        ];
        for (what, header) in broken {
            let mut f = LogoFlash::new(ErasePolicy::SectorOnFirstWrite);
            let mut data = good.clone();
            data[..16].copy_from_slice(&header);
            write_chunked(&mut f, &data);
            assert_eq!(boot_render(&f, 2, ON), BootScreen::BuiltIn, "{what}");
        }
    }

    #[test]
    fn a2xtes_boot_render_rules_are_marked_inferred() {
        let src = include_str!("logo.rs");
        let at = src.find("pub fn boot_render").expect("boot_render exists");
        let before = &src[..at];
        let doc_start = before.rfind("\n\n").map_or(0, |i| i + 2);
        let doc = &before[doc_start..];
        assert!(
            doc.lines().all(|l| {
                let l = l.trim_start();
                l.starts_with("///") || l.starts_with("#[") || l.is_empty()
            }),
            "doc block found: {doc}"
        );
        let marks = doc.matches("inferred").count();
        assert!(
            marks >= 3,
            "each boot rule (nothing, built-in, custom) is marked [inferred]: {doc}"
        );
    }
}
