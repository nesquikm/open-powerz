//! The field tree: every decoded field with its name, bit range, raw value
//! and meaning, for the packet detail view.

/// Inclusive bit range of a field, `hi` down to `lo`.
///
/// Header and data-object fields count bits inside their 16- or 32-bit
/// word; data-block fields count from bit 0 of the block's first byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitRange {
    /// Highest bit.
    pub hi: u16,
    /// Lowest bit.
    pub lo: u16,
}

/// Why a field value deserves attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Flag {
    /// A reserved value, or a reserved bit that is set.
    Reserved,
    /// A value the spec does not allow here.
    Invalid,
}

/// One node of the field tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Spec name of the field.
    pub name: String,
    /// Where the field sits.
    pub bits: BitRange,
    /// The field's bits, shifted down to bit 0.
    pub raw: u64,
    /// Human-readable value, with units where the spec has them.
    pub meaning: String,
    /// Set when the value is reserved or invalid.
    pub flag: Option<Flag>,
    /// Sub-fields (a header's bit fields, a PDO's fields, ...).
    pub children: Vec<Field>,
}

impl Field {
    /// A leaf field with no flag.
    pub fn new(
        name: impl Into<String>,
        hi: u16,
        lo: u16,
        raw: u64,
        meaning: impl Into<String>,
    ) -> Field {
        Field {
            name: name.into(),
            bits: BitRange { hi, lo },
            raw,
            meaning: meaning.into(),
            flag: None,
            children: Vec::new(),
        }
    }

    /// The same field with `flag` set.
    pub fn flagged(mut self, flag: Option<Flag>) -> Field {
        self.flag = flag;
        self
    }

    /// The same field with `children` as its sub-fields.
    pub fn with_children(mut self, children: Vec<Field>) -> Field {
        self.children = children;
        self
    }
}

// Shared helpers for the decoders: bit extraction, unit formatting and
// the common leaf fields.

/// Bits `hi..=lo` of `raw`, shifted down to bit 0.
pub(crate) fn bits(raw: u32, hi: u32, lo: u32) -> u32 {
    (raw >> lo) & (u32::MAX >> (31 - (hi - lo)))
}

/// Bit `n` of `raw`.
pub(crate) fn bit(raw: u32, n: u32) -> bool {
    bits(raw, n, n) == 1
}

/// `mv` millivolts as "5.00 V".
pub(crate) fn volts(mv: u32) -> String {
    format!("{}.{:02} V", mv / 1000, (mv % 1000) / 10)
}

/// `ma` milliamps as "3.00 A".
pub(crate) fn amps(ma: u32) -> String {
    format!("{}.{:02} A", ma / 1000, (ma % 1000) / 10)
}

/// `mw` milliwatts as "100.00 W".
pub(crate) fn watts(mw: u32) -> String {
    format!("{}.{:02} W", mw / 1000, (mw % 1000) / 10)
}

/// "Yes" or "No".
pub(crate) fn yes_no(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

/// A numeric field: `hi..=lo` of `raw`, its meaning from `show`.
pub(crate) fn num(name: &str, raw: u32, hi: u16, lo: u16, show: impl Fn(u32) -> String) -> Field {
    let v = bits(raw, u32::from(hi), u32::from(lo));
    Field::new(name, hi, lo, u64::from(v), show(v))
}

/// A one-bit flag field.
pub(crate) fn flag(name: &str, raw: u32, n: u16) -> Field {
    let v = bit(raw, u32::from(n));
    Field::new(name, n, n, u64::from(v), yes_no(v))
}

/// A reserved range, flagged when any of its bits is set.
pub(crate) fn reserved(raw: u32, hi: u16, lo: u16) -> Field {
    let v = bits(raw, u32::from(hi), u32::from(lo));
    Field::new("Reserved", hi, lo, u64::from(v), format!("{v:#X}"))
        .flagged((v != 0).then_some(Flag::Reserved))
}
