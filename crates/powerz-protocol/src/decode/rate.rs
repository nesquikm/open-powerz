//! ADC-queue rate codes. See `docs/protocol.md`.

/// Queue sample rate. Codes 0..3 only; code 4 is not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rate {
    /// Code 0: 2 samples per second.
    Sps2,
    /// Code 1: 10 samples per second.
    Sps10,
    /// Code 2: 50 samples per second.
    Sps50,
    /// Code 3: 1000 samples per second.
    Sps1000,
}

impl Rate {
    /// Every representable rate, in code order.
    pub const ALL: [Rate; 4] = [Rate::Sps2, Rate::Sps10, Rate::Sps50, Rate::Sps1000];

    /// Rate for a wire code (0..3); `None` for anything else.
    pub fn from_code(code: u8) -> Option<Rate> {
        Rate::ALL.get(usize::from(code)).copied()
    }

    /// Wire code (0..3).
    pub fn code(self) -> u8 {
        match self {
            Rate::Sps2 => 0,
            Rate::Sps10 => 1,
            Rate::Sps50 => 2,
            Rate::Sps1000 => 3,
        }
    }

    /// Samples per second.
    pub fn sps(self) -> u32 {
        match self {
            Rate::Sps2 => 2,
            Rate::Sps10 => 10,
            Rate::Sps50 => 50,
            Rate::Sps1000 => 1000,
        }
    }

    /// Rate for a samples-per-second value; `None` unless 2, 10, 50 or 1000.
    pub fn from_sps(sps: u32) -> Option<Rate> {
        Rate::ALL.into_iter().find(|r| r.sps() == sps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- AC-E88WR5.5: code <-> SPS table ----

    #[test]
    fn decode_rate_codes_map_to_sps() {
        let table = [(0u8, 2u32), (1, 10), (2, 50), (3, 1000)];
        for (code, sps) in table {
            let r = Rate::from_code(code).unwrap();
            assert_eq!(r.code(), code);
            assert_eq!(r.sps(), sps);
            assert_eq!(Rate::from_sps(sps), Some(r));
        }
        assert_eq!(Rate::from_code(0), Some(Rate::Sps2));
        assert_eq!(Rate::from_code(1), Some(Rate::Sps10));
        assert_eq!(Rate::from_code(2), Some(Rate::Sps50));
        assert_eq!(Rate::from_code(3), Some(Rate::Sps1000));
    }

    #[test]
    fn decode_rate_round_trips_every_variant() {
        for r in Rate::ALL {
            assert_eq!(Rate::from_code(r.code()), Some(r));
            assert_eq!(Rate::from_sps(r.sps()), Some(r));
        }
    }

    #[test]
    fn decode_rate_code_4_and_above_not_representable() {
        for code in 4..=u8::MAX {
            assert_eq!(Rate::from_code(code), None, "code {code}");
        }
    }

    #[test]
    fn decode_rate_from_sps_rejects_other_values() {
        for sps in [
            0u32,
            1,
            3,
            4,
            5,
            9,
            11,
            49,
            51,
            100,
            999,
            1001,
            10_000,
            u32::MAX,
        ] {
            assert_eq!(Rate::from_sps(sps), None, "sps {sps}");
        }
    }
}
