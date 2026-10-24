//! Typed parsers for the binary capability blocks carried by PDM replies.
//!
//! Every block starts with its ASCII tag. Layouts, offsets from the tag:
//!
//! - `pdo:` \[device\]: +4 one marker byte (do not depend on it), +5 slot
//!   count (u8), +6 count × u32 LE source PDOs (zero = unused slot).
//! - `ufcs:` \[static\]: +5 count (u8), +6 count × u64 LE output capabilities.
//! - `scp:` \[static\]: nine u16 LE at +4..+0x14.
//! - `vfcp:` \[static\]: u16 LE at +6..+0x10, then a raw tail up to the line end.
//! - `tfcp:` \[static\]: u32 LE at +6, u16 LE at +0xA..+0x10.
//!
//! The returned length never includes the `\n` that may follow a block.

use super::{BlockTag, Error};
use crate::pd::SourcePdo;

/// Most PDO slots one `pdo:` block may declare (EPR source caps hold 11).
pub const MAX_PDO_SLOTS: u8 = 11;
/// Most entries one `ufcs:` block may declare (the index is 4 bits).
pub const MAX_UFCS_ENTRIES: u8 = 15;

/// A `pdo:` block: the charger's source PDOs by slot (object position).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdoBlock {
    /// The byte at tag+4 (`'4'` SPR / `'9'` EPR on the device); informative only.
    pub marker: u8,
    /// Raw PDO per slot, slot 1 first; zero means unused.
    pub slots: Vec<u32>,
}

impl PdoBlock {
    /// The PDO at 1-based `position`, `None` for an unused or missing slot.
    pub fn pdo(&self, position: u8) -> Option<SourcePdo> {
        let index = usize::from(position).checked_sub(1)?;
        let raw = *self.slots.get(index)?;
        (raw != 0).then(|| SourcePdo::decode(raw))
    }

    /// Every used slot as `(position, PDO)`, in slot order.
    pub fn offers(&self) -> Vec<(u8, SourcePdo)> {
        (1..=MAX_PDO_SLOTS)
            .filter_map(|position| Some((position, self.pdo(position)?)))
            .collect()
    }
}

/// One UFCS output capability (u64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UfcsCap {
    /// The 64 raw bits.
    pub raw: u64,
    /// Capability index (bits 63..60).
    pub index: u8,
    /// Minimum voltage, mV (bits 39..24, 10 mV units).
    pub min_voltage_mv: u32,
    /// Maximum voltage, mV (bits 55..40, 10 mV units).
    pub max_voltage_mv: u32,
    /// Minimum current, mA (bits 7..0, 10 mA units).
    pub min_current_ma: u32,
    /// Maximum current, mA (bits 23..8, 10 mA units).
    pub max_current_ma: u32,
    /// Voltage step, mV ((bit 56 + 1) × 10).
    pub voltage_step_mv: u32,
    /// Current step, mA ((bits 59..57 + 1) × 10).
    pub current_step_ma: u32,
}

/// A `ufcs:` block. Entries whose upper 32 bits are zero are skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UfcsCaps {
    /// The used entries, in block order.
    pub entries: Vec<UfcsCap>,
}

/// An `scp:` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScpCaps {
    /// Maximum power, mW (u16 at +4, 0.1 W units).
    pub max_power_mw: u32,
    /// Minimum voltage, mV (+6).
    pub min_voltage_mv: u32,
    /// Maximum voltage, mV (+8).
    pub max_voltage_mv: u32,
    /// Minimum current, mA (+0xA).
    pub min_current_ma: u32,
    /// Maximum current, mA (+0xC).
    pub max_current_ma: u32,
    /// Voltage step, mV (+0xE).
    pub voltage_step_mv: u32,
    /// Current step, mA (+0x10).
    pub current_step_ma: u32,
    /// Bound voltage, mV (+0x12).
    pub bound_voltage_mv: u32,
    /// Bound current, mA (+0x14).
    pub bound_current_ma: u32,
}

/// A `vfcp:` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfcpCaps {
    /// Maximum power, mW (u16 at +0x10, 0.1 W units).
    pub max_power_mw: u32,
    /// First mode voltage, mV (+8).
    pub num1_voltage_mv: u32,
    /// First mode current, mA (+0xC).
    pub num1_current_ma: u32,
    /// Second mode voltage, mV (+0xA).
    pub num2_voltage_mv: u32,
    /// Second mode current, mA (+0xE).
    pub num2_current_ma: u32,
    /// The whole block from the tag on (line end excluded).
    pub raw: Vec<u8>,
}

/// A `tfcp:` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TfcpCaps {
    /// Maximum power, mW (u32 at +6, 0.1 W units).
    pub max_power_mw: u32,
    /// Minimum voltage, mV (+0xA).
    pub min_voltage_mv: u32,
    /// Maximum voltage, mV (+0xE).
    pub max_voltage_mv: u32,
    /// Minimum current, mA (+0xC).
    pub min_current_ma: u32,
    /// Maximum current, mA (+0x10).
    pub max_current_ma: u32,
}

/// One parsed capability block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapBlock {
    /// `pdo:`
    Pdo(PdoBlock),
    /// `ufcs:`
    Ufcs(UfcsCaps),
    /// `scp:`
    Scp(ScpCaps),
    /// `vfcp:`
    Vfcp(VfcpCaps),
    /// `tfcp:`
    Tfcp(TfcpCaps),
}

impl CapBlock {
    /// The block's tag.
    pub fn tag(&self) -> BlockTag {
        match self {
            CapBlock::Pdo(_) => BlockTag::Pdo,
            CapBlock::Ufcs(_) => BlockTag::Ufcs,
            CapBlock::Scp(_) => BlockTag::Scp,
            CapBlock::Vfcp(_) => BlockTag::Vfcp,
            CapBlock::Tfcp(_) => BlockTag::Tfcp,
        }
    }
}

/// The last known capabilities per protocol.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Latest `pdo:` block (the EPR list replaces the SPR one when it follows).
    pub pd: Option<PdoBlock>,
    /// Latest `ufcs:` block.
    pub ufcs: Option<UfcsCaps>,
    /// Latest `scp:` block.
    pub scp: Option<ScpCaps>,
    /// Latest `vfcp:` block.
    pub vfcp: Option<VfcpCaps>,
    /// Latest `tfcp:` block.
    pub tfcp: Option<TfcpCaps>,
}

impl Capabilities {
    /// Record `block`, replacing the previous block of the same tag.
    pub fn update(&mut self, block: &CapBlock) {
        match block {
            CapBlock::Pdo(b) => self.pd = Some(b.clone()),
            CapBlock::Ufcs(b) => self.ufcs = Some(b.clone()),
            CapBlock::Scp(b) => self.scp = Some(*b),
            CapBlock::Vfcp(b) => self.vfcp = Some(b.clone()),
            CapBlock::Tfcp(b) => self.tfcp = Some(*b),
        }
    }
}

/// The block tag `buf` starts with, unless what follows the tag reads as an
/// `entry list` text line: a result word (`ufcs:null`, `scp:true`), or a
/// bare `vfcp:` line (see below). `None` also while the bytes so far could
/// still become a result word.
///
/// A bare `vfcp:` line (the tag, then `\n`) is text only when what comes
/// after the `\n` is nothing yet or printable text up to a line end (the
/// next list line); a non-printable byte there makes it a block whose first
/// data byte is 0x0A, and printable bytes with no line end yet keep the tag
/// so the reader waits. `scp:` / `tfcp:` followed by `\n` are always blocks
/// (an SCP max power whose low byte is 0x0A starts that way).
pub fn block_tag(buf: &[u8]) -> Option<BlockTag> {
    let (tag, text) = TAGS.iter().find(|(_, text)| buf.starts_with(text))?;
    let rest = buf.get(text.len()..).filter(|r| !r.is_empty())?;
    // `entry list` prints `<proto>:null` / `:true` / `:false` as text
    // [device]; any other byte after the tag is block data, which may well
    // be a letter (e.g. an SCP max power of 10 W is 0x64 'd'). While the
    // bytes so far could still become a word, wait: it reads as text.
    let maybe_word = TEXT_WORDS
        .iter()
        .any(|w| rest.starts_with(w) || w.starts_with(rest));
    // A cut-off `entry list` ended on a bare `vfcp:` line [device]
    // 2026-10-23. Only VFCP: no field is read from its byte after the tag
    // [static], while SCP's is the low byte of its max power.
    let bare = *tag == BlockTag::Vfcp
        && rest.first() == Some(&b'\n')
        && text_follows(rest.get(1..).unwrap_or_default());
    (!maybe_word && !bare).then_some(*tag)
}

/// Whether the bytes after a bare `vfcp:` line read as text: nothing yet
/// (the cut-off list went quiet), or printable bytes up to a `\n`. A
/// non-printable byte first, or printable bytes without a line end yet,
/// is not (yet) text.
fn text_follows(after: &[u8]) -> bool {
    if after.is_empty() {
        return true;
    }
    for &b in after {
        match b {
            b'\n' => return true,
            0x20..=0x7e => {}
            _ => return false,
        }
    }
    false
}

/// Results `entry list` prints after a tag-like protocol name. \[device\]
const TEXT_WORDS: [&[u8]; 3] = [b"null", b"true", b"false"];

/// Every block tag with its ASCII text.
const TAGS: [(BlockTag, &[u8]); 5] = [
    (BlockTag::Pdo, b"pdo:"),
    (BlockTag::Ufcs, b"ufcs:"),
    (BlockTag::Scp, b"scp:"),
    (BlockTag::Vfcp, b"vfcp:"),
    (BlockTag::Tfcp, b"tfcp:"),
];

/// Parse the block at the start of `buf` (which starts with a block tag).
///
/// `Ok(None)`: more bytes are needed. `Ok(Some((block, len)))`: `len` bytes
/// form the block (any following `\n` excluded). `Err(Error::Malformed)`:
/// the block cannot be valid. Never panics.
pub fn parse(buf: &[u8]) -> Result<Option<(CapBlock, usize)>, Error> {
    let Some((tag, _)) = TAGS.iter().find(|(_, text)| buf.starts_with(text)) else {
        return Ok(None);
    };
    match tag {
        BlockTag::Pdo => parse_pdo(buf),
        BlockTag::Ufcs => parse_ufcs(buf),
        BlockTag::Scp => Ok(parse_scp(buf)),
        BlockTag::Vfcp => parse_vfcp(buf),
        BlockTag::Tfcp => Ok(parse_tfcp(buf)),
    }
}

/// u16 LE at `at`, as u32 (callers checked the length).
fn u16_at(buf: &[u8], at: usize) -> u32 {
    let b = |i: usize| buf.get(at + i).copied().unwrap_or(0);
    u32::from(u16::from_le_bytes([b(0), b(1)]))
}

/// Power in 0.1 W units to mW.
fn deciwatts_to_mw(v: u32) -> u32 {
    v.saturating_mul(100)
}

/// `ufcs:` + count + count x u64 LE. \[static\]
fn parse_ufcs(buf: &[u8]) -> Result<Option<(CapBlock, usize)>, Error> {
    let Some(&count) = buf.get(5) else {
        return Ok(None);
    };
    if count > MAX_UFCS_ENTRIES {
        return Err(Error::Malformed {
            tag: BlockTag::Ufcs,
            reason: format!("{count} entries, at most {MAX_UFCS_ENTRIES}"),
        });
    }
    let len = 6 + usize::from(count) * 8;
    let Some(body) = buf.get(6..len) else {
        return Ok(None);
    };
    let entries = body
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_le_bytes(*c))
        .filter(|raw| raw >> 32 != 0)
        .map(ufcs_cap)
        .collect();
    Ok(Some((CapBlock::Ufcs(UfcsCaps { entries }), len)))
}

/// Decode one UFCS output capability. \[static\]
fn ufcs_cap(raw: u64) -> UfcsCap {
    // Every field is masked to at most 16 bits, so the casts are lossless.
    let field = |shift: u32, bits: u32| ((raw >> shift) & ((1u64 << bits) - 1)) as u32;
    UfcsCap {
        raw,
        index: field(60, 4) as u8,
        min_voltage_mv: field(24, 16) * 10,
        max_voltage_mv: field(40, 16) * 10,
        min_current_ma: field(0, 8) * 10,
        max_current_ma: field(8, 16) * 10,
        voltage_step_mv: (field(56, 1) + 1) * 10,
        current_step_ma: (field(57, 3) + 1) * 10,
    }
}

/// `scp:` + nine u16 LE at +4..+0x14. \[static\]
fn parse_scp(buf: &[u8]) -> Option<(CapBlock, usize)> {
    const LEN: usize = 0x16;
    let b = buf.get(..LEN)?;
    let caps = ScpCaps {
        max_power_mw: deciwatts_to_mw(u16_at(b, 4)),
        min_voltage_mv: u16_at(b, 6),
        max_voltage_mv: u16_at(b, 8),
        min_current_ma: u16_at(b, 0xA),
        max_current_ma: u16_at(b, 0xC),
        voltage_step_mv: u16_at(b, 0xE),
        current_step_ma: u16_at(b, 0x10),
        bound_voltage_mv: u16_at(b, 0x12),
        bound_current_ma: u16_at(b, 0x14),
    };
    Some((CapBlock::Scp(caps), LEN))
}

/// `vfcp:` + u16 LE at +6..+0x10, then a raw tail up to the line end. \[static\]
fn parse_vfcp(buf: &[u8]) -> Result<Option<(CapBlock, usize)>, Error> {
    const FIXED: usize = 0x12;
    let Some(tail) = buf.get(FIXED..) else {
        return Ok(None);
    };
    let Some(end) = tail.iter().position(|&b| b == b'\n') else {
        if tail.len() > VFCP_MAX_TAIL {
            return Err(Error::Malformed {
                tag: BlockTag::Vfcp,
                reason: format!("no line end within {VFCP_MAX_TAIL} bytes"),
            });
        }
        return Ok(None);
    };
    let len = FIXED + end;
    let Some(b) = buf.get(..len) else {
        return Ok(None);
    };
    let caps = VfcpCaps {
        max_power_mw: deciwatts_to_mw(u16_at(b, 0x10)),
        num1_voltage_mv: u16_at(b, 8),
        num1_current_ma: u16_at(b, 0xC),
        num2_voltage_mv: u16_at(b, 0xA),
        num2_current_ma: u16_at(b, 0xE),
        raw: b.to_vec(),
    };
    Ok(Some((CapBlock::Vfcp(caps), len)))
}

/// Longest `vfcp:` tail accepted after the fixed fields; its real length
/// is unknown, so this only bounds a block whose line end never comes.
/// \[inferred\]
const VFCP_MAX_TAIL: usize = 256;

/// `tfcp:` + u32 LE at +6, u16 LE at +0xA..+0x10. \[static\]
fn parse_tfcp(buf: &[u8]) -> Option<(CapBlock, usize)> {
    const LEN: usize = 0x12;
    let b = buf.get(..LEN)?;
    let power = u16_at(b, 6) | (u16_at(b, 8) << 16);
    let caps = TfcpCaps {
        max_power_mw: deciwatts_to_mw(power),
        min_voltage_mv: u16_at(b, 0xA),
        max_voltage_mv: u16_at(b, 0xE),
        min_current_ma: u16_at(b, 0xC),
        max_current_ma: u16_at(b, 0x10),
    };
    Some((CapBlock::Tfcp(caps), LEN))
}

/// `pdo:` + marker + count + count x u32 LE. \[device\]
fn parse_pdo(buf: &[u8]) -> Result<Option<(CapBlock, usize)>, Error> {
    let (Some(&marker), Some(&count)) = (buf.get(4), buf.get(5)) else {
        return Ok(None);
    };
    if count > MAX_PDO_SLOTS {
        return Err(Error::Malformed {
            tag: BlockTag::Pdo,
            reason: format!("{count} slots, at most {MAX_PDO_SLOTS}"),
        });
    }
    let len = 6 + usize::from(count) * 4;
    let Some(body) = buf.get(6..len) else {
        return Ok(None);
    };
    let slots = body
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    Ok(Some((CapBlock::Pdo(PdoBlock { marker, slots }), len)))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use crate::decode::test_util::hex_bytes;

    /// `pdo:` SPR list from the 140 W charger (session 2), trailing `\n` included.
    pub(crate) fn spr_block() -> Vec<u8> {
        hex_bytes(
            "70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600 \
             00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a",
        )
    }

    /// `pdo:` EPR list from the 140 W charger (session 2), trailing `\n` included.
    pub(crate) fn epr_block() -> Vec<u8> {
        hex_bytes(
            "70646f3a 39 0b 2c918109 2cd10200 2cb10400 f4410600 \
             00000000 00000000 00000000 f4c10800 8c9630d2 00000000 00000000 0a",
        )
    }

    /// Synthetic UFCS entry: index 1, 3.4-20 V, 0.1-5 A, 10 mV / 50 mA steps.
    pub(crate) fn ufcs_entry() -> u64 {
        (1u64 << 60) | (4u64 << 57) | (2000u64 << 40) | (340u64 << 24) | (500u64 << 8) | 10
    }

    /// Synthetic `ufcs:` block with one used and one zero entry.
    pub(crate) fn ufcs_block() -> Vec<u8> {
        let mut b = b"ufcs:".to_vec();
        b.push(2);
        b.extend(ufcs_entry().to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b
    }

    /// Synthetic `scp:` block: 40 W, 3.4-20 V, 0.1-4 A, 10 mV / 50 mA steps,
    /// bound 11 V / 5 A.
    pub(crate) fn scp_block() -> Vec<u8> {
        let mut b = b"scp:".to_vec();
        for v in [400u16, 3400, 20000, 100, 4000, 10, 50, 11000, 5000] {
            b.extend(v.to_le_bytes());
        }
        b
    }

    /// Synthetic `tfcp:` block: 60 W, 5-20 V, 0.1-3 A.
    pub(crate) fn tfcp_block() -> Vec<u8> {
        let mut b = b"tfcp:".to_vec();
        b.push(0);
        b.extend(600u32.to_le_bytes());
        for v in [5000u16, 100, 20000, 3000] {
            b.extend(v.to_le_bytes());
        }
        b
    }

    /// Synthetic `vfcp:` block: 18 W, 9 V 2 A / 12 V 1.5 A, raw tail, no `\n`.
    pub(crate) fn vfcp_block() -> Vec<u8> {
        let mut b = b"vfcp:".to_vec();
        b.push(0);
        for v in [7000u16, 9000, 12000, 2000, 1500, 180] {
            b.extend(v.to_le_bytes());
        }
        b.extend([0x04, 0x00, 0x01, 0x00, 0x0D, 0x20, 0xA6, 0x00]);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::pd::SourcePdo;

    fn parsed(buf: &[u8]) -> (CapBlock, usize) {
        parse(buf)
            .expect("well-formed block")
            .expect("complete block")
    }

    // ---- AC-PR41FT.3: pdo blocks (device golden vectors) ----

    #[test]
    fn spr_pdo_block_parses_to_four_fixed_offers() {
        let bytes = spr_block();
        let (block, len) = parsed(&bytes);
        assert_eq!(
            len,
            bytes.len() - 1,
            "the trailing \\n is not part of the block"
        );
        let CapBlock::Pdo(pdo) = block else {
            panic!("expected a pdo block, got {block:?}");
        };
        assert_eq!(pdo.marker, b'4');
        assert_eq!(pdo.slots.len(), 11);
        let offers = pdo.offers();
        let positions: Vec<u8> = offers.iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, vec![1, 2, 3, 4]);
        let volts: Vec<(u32, u32)> = offers
            .iter()
            .map(|(_, o)| match o {
                SourcePdo::Fixed(f) => (f.voltage_mv, f.max_current_ma),
                other => panic!("expected fixed, got {other:?}"),
            })
            .collect();
        assert_eq!(
            volts,
            vec![(5000, 3000), (9000, 3000), (15000, 3000), (20000, 4700)]
        );
    }

    #[test]
    fn epr_pdo_block_keeps_slot_positions_8_and_9() {
        let (block, _) = parsed(&epr_block());
        let CapBlock::Pdo(pdo) = block else {
            panic!("expected a pdo block");
        };
        assert_eq!(pdo.marker, b'9');
        let positions: Vec<u8> = pdo.offers().iter().map(|(p, _)| *p).collect();
        assert_eq!(positions, vec![1, 2, 3, 4, 8, 9]);
        match pdo.pdo(8) {
            Some(SourcePdo::Fixed(f)) => {
                assert_eq!((f.voltage_mv, f.max_current_ma), (28000, 5000));
            }
            other => panic!("slot 8: {other:?}"),
        }
        match pdo.pdo(9) {
            Some(SourcePdo::EprAvs(a)) => {
                assert_eq!(
                    (a.min_voltage_mv, a.max_voltage_mv, a.pdp_w),
                    (15000, 28000, 140)
                );
            }
            other => panic!("slot 9: {other:?}"),
        }
        assert_eq!(pdo.pdo(5), None, "zero slot is unused");
        assert_eq!(pdo.pdo(0), None);
        assert_eq!(pdo.pdo(12), None);
    }

    #[test]
    fn pdo_block_needs_all_its_slots() {
        let bytes = spr_block();
        for cut in [4, 5, 6, 10, bytes.len() - 2] {
            assert_eq!(parse(&bytes[..cut]), Ok(None), "cut at {cut}");
        }
    }

    #[test]
    fn pdo_block_with_too_many_slots_is_malformed() {
        let mut bytes = b"pdo:4".to_vec();
        bytes.push(0x20);
        bytes.extend([0u8; 0x20 * 4]);
        match parse(&bytes) {
            Err(Error::Malformed { tag, .. }) => assert_eq!(tag, BlockTag::Pdo),
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    // ---- AC-PR41FT.3: ufcs / scp / vfcp / tfcp blocks (synthetic, [static] layouts) ----

    #[test]
    fn ufcs_block_decodes_fields_and_skips_zero_entries() {
        let bytes = ufcs_block();
        let (block, len) = parsed(&bytes);
        assert_eq!(len, bytes.len());
        let CapBlock::Ufcs(u) = block else {
            panic!("expected ufcs");
        };
        assert_eq!(
            u.entries,
            vec![UfcsCap {
                raw: ufcs_entry(),
                index: 1,
                min_voltage_mv: 3400,
                max_voltage_mv: 20000,
                min_current_ma: 100,
                max_current_ma: 5000,
                voltage_step_mv: 10,
                current_step_ma: 50,
            }]
        );
    }

    #[test]
    fn ufcs_block_with_too_many_entries_is_malformed() {
        let mut bytes = b"ufcs:".to_vec();
        bytes.push(0x40);
        bytes.extend([0u8; 0x40 * 8]);
        assert!(
            matches!(
                parse(&bytes),
                Err(Error::Malformed {
                    tag: BlockTag::Ufcs,
                    ..
                })
            ),
            "{:?}",
            parse(&bytes)
        );
    }

    #[test]
    fn scp_block_decodes_nine_fields() {
        let bytes = scp_block();
        let (block, len) = parsed(&bytes);
        assert_eq!(len, 22);
        assert_eq!(
            block,
            CapBlock::Scp(ScpCaps {
                max_power_mw: 40_000,
                min_voltage_mv: 3400,
                max_voltage_mv: 20000,
                min_current_ma: 100,
                max_current_ma: 4000,
                voltage_step_mv: 10,
                current_step_ma: 50,
                bound_voltage_mv: 11000,
                bound_current_ma: 5000,
            })
        );
        assert_eq!(parse(&bytes[..21]), Ok(None));
    }

    #[test]
    fn tfcp_block_reads_u32_power_and_ranges() {
        let bytes = tfcp_block();
        let (block, len) = parsed(&bytes);
        assert_eq!(len, 18);
        assert_eq!(
            block,
            CapBlock::Tfcp(TfcpCaps {
                max_power_mw: 60_000,
                min_voltage_mv: 5000,
                max_voltage_mv: 20000,
                min_current_ma: 100,
                max_current_ma: 3000,
            })
        );
    }

    #[test]
    fn vfcp_block_without_a_line_end_is_malformed_past_the_cap() {
        let mut bytes = vfcp_block();
        bytes.extend(vec![0x55; 600]);
        assert!(
            matches!(
                parse(&bytes),
                Err(Error::Malformed {
                    tag: BlockTag::Vfcp,
                    ..
                })
            ),
            "{:?}",
            parse(&bytes)
        );
    }

    #[test]
    fn vfcp_block_runs_to_the_line_end() {
        let mut bytes = vfcp_block();
        let block_len = bytes.len();
        // Without the line end the tail is not known to be complete.
        assert_eq!(parse(&bytes), Ok(None));
        bytes.push(b'\n');
        let (block, len) = parsed(&bytes);
        assert_eq!(len, block_len);
        let CapBlock::Vfcp(v) = block else {
            panic!("expected vfcp");
        };
        assert_eq!(v.max_power_mw, 18_000);
        assert_eq!((v.num1_voltage_mv, v.num1_current_ma), (9000, 2000));
        assert_eq!((v.num2_voltage_mv, v.num2_current_ma), (12000, 1500));
        assert_eq!(v.raw, bytes[..block_len].to_vec());
    }

    #[test]
    fn block_tag_tells_blocks_from_text() {
        assert_eq!(block_tag(&spr_block()), Some(BlockTag::Pdo));
        assert_eq!(block_tag(&ufcs_block()), Some(BlockTag::Ufcs));
        assert_eq!(block_tag(&scp_block()), Some(BlockTag::Scp));
        assert_eq!(block_tag(&vfcp_block()), Some(BlockTag::Vfcp));
        assert_eq!(block_tag(&tfcp_block()), Some(BlockTag::Tfcp));
        // `entry list` prints `ufcs:null` / `vfcp:null` as text [device].
        assert_eq!(block_tag(b"ufcs:null\n"), None);
        assert_eq!(block_tag(b"vfcp:null\n"), None);
        assert_eq!(block_tag(b"ready:5191mV,0mA\n"), None);
        assert_eq!(block_tag(b"scp:true 9000mV,0mA\n"), None);
        assert_eq!(block_tag(b"tfcp:false\n"), None);
    }

    #[test]
    fn scp_block_with_a_letter_byte_after_the_tag_is_still_a_block() {
        // 10 W max power = 100 = 0x64 'd': data, not the start of a word.
        let mut b = b"scp:".to_vec();
        for v in [100u16, 3400, 20000, 100, 4000, 10, 50, 11000, 5000] {
            b.extend(v.to_le_bytes());
        }
        assert_eq!(block_tag(&b), Some(BlockTag::Scp));
        assert!(matches!(parsed(&b).0, CapBlock::Scp(_)));
    }

    #[test]
    fn scp_block_whose_first_data_byte_is_a_line_end_is_a_block() {
        // 26.6 W max power = 266 = 0x010A: the byte after the tag is 0x0A.
        let mut b = b"scp:".to_vec();
        for v in [266u16, 3400, 20000, 100, 4000, 10, 50, 11000, 5000] {
            b.extend(v.to_le_bytes());
        }
        assert_eq!(&b[4..6], [0x0A, 0x01]);
        assert_eq!(block_tag(&b), Some(BlockTag::Scp));
        assert_eq!(block_tag(b"scp:\n"), Some(BlockTag::Scp));
        match parsed(&b) {
            (CapBlock::Scp(s), 22) => assert_eq!(s.max_power_mw, 26_600),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tfcp_block_whose_first_data_byte_is_a_line_end_is_a_block() {
        let mut b = b"tfcp:".to_vec();
        b.push(0x0A);
        b.extend(600u32.to_le_bytes());
        for v in [5000u16, 100, 20000, 3000] {
            b.extend(v.to_le_bytes());
        }
        assert_eq!(block_tag(&b), Some(BlockTag::Tfcp));
        assert_eq!(block_tag(b"tfcp:\n"), Some(BlockTag::Tfcp));
        assert!(matches!(parsed(&b).0, CapBlock::Tfcp(_)));
    }

    #[test]
    fn a_bare_vfcp_line_is_text_only_when_text_or_nothing_follows() {
        // The cut-off list's last line, nothing after it yet [device].
        assert_eq!(block_tag(b"vfcp:\n"), None);
        // Another list line after it.
        assert_eq!(block_tag(b"vfcp:\nready:5191mV,0mA\n"), None);
        assert_eq!(block_tag(b"vfcp:\n\n"), None);
        // A printable line not yet ended: undecided, the tag is kept so the
        // reader waits for more bytes.
        assert_eq!(block_tag(b"vfcp:\nready:51"), Some(BlockTag::Vfcp));
        // A block whose first data byte is 0x0A: binary bytes follow.
        let mut b = vfcp_block();
        b[5] = 0x0A;
        assert_eq!(block_tag(&b), Some(BlockTag::Vfcp));
    }

    #[test]
    fn capabilities_keep_the_latest_block_per_tag() {
        let (spr, _) = parsed(&spr_block());
        let (epr, _) = parsed(&epr_block());
        let (scp, _) = parsed(&scp_block());
        let mut caps = Capabilities::default();
        caps.update(&spr);
        caps.update(&scp);
        caps.update(&epr);
        assert_eq!(caps.pd.as_ref().map(|p| p.marker), Some(b'9'));
        assert!(caps.scp.is_some());
        assert_eq!(epr.tag(), BlockTag::Pdo);
        assert_eq!(scp.tag(), BlockTag::Scp);
    }
}
