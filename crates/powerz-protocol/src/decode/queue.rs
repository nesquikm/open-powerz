//! ADC-queue records (`ATT_ADC_QUEUE`, 20 bytes each). See `docs/protocol.md`.

use super::{DecodeError, Rate, i32_at, micro, need, u16_at, u32_at};

/// Length of one queue record.
const RECORD_LEN: usize = 20;

/// One 20-byte queue record, kept as raw integers plus its rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueRecord {
    time_ms: u32,
    vbus_uv: i32,
    ibus_ua: i32,
    cc1: u16,
    cc2: u16,
    dp: u16,
    dm: u16,
    rate: Rate,
}

impl QueueRecord {
    /// Decode one 20-byte record captured at `rate`.
    pub fn decode(record: &[u8], rate: Rate) -> Result<QueueRecord, DecodeError> {
        need("ADC queue record", record, RECORD_LEN)?;
        let p = record;
        Ok(QueueRecord {
            time_ms: u32_at(p, 0x00),
            vbus_uv: i32_at(p, 0x04),
            ibus_ua: i32_at(p, 0x08),
            cc1: u16_at(p, 0x0C),
            cc2: u16_at(p, 0x0E),
            dp: u16_at(p, 0x10),
            dm: u16_at(p, 0x12),
            rate,
        })
    }

    /// Volts per raw CC / D± unit: 0.1 mV at code 0, 1 mV at codes 1..3.
    fn line_v(&self, raw: u16) -> f64 {
        match self.rate {
            Rate::Sps2 => f64::from(raw) / 10_000.0,
            Rate::Sps10 | Rate::Sps50 | Rate::Sps1000 => f64::from(raw) / 1_000.0,
        }
    }

    /// Device time (uptime) in milliseconds.
    pub fn time_ms(&self) -> u32 {
        self.time_ms
    }

    /// VBUS in volts.
    pub fn vbus_v(&self) -> f64 {
        micro(self.vbus_uv)
    }

    /// IBUS in amperes, signed.
    pub fn ibus_a(&self) -> f64 {
        micro(self.ibus_ua)
    }

    /// CC1 in volts.
    pub fn cc1_v(&self) -> f64 {
        self.line_v(self.cc1)
    }

    /// CC2 in volts.
    pub fn cc2_v(&self) -> f64 {
        self.line_v(self.cc2)
    }

    /// D+ in volts.
    pub fn dp_v(&self) -> f64 {
        self.line_v(self.dp)
    }

    /// D− in volts.
    pub fn dm_v(&self) -> f64 {
        self.line_v(self.dm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::test_util::{approx, hex_bytes, section_payload};

    const QUEUE_1268: &str = include_str!("../../tests/vectors/queue_1268.hex");

    // ---- AC-E88WR5.4: 20-byte records, rate-dependent CC/D± scale ----

    #[test]
    fn decode_captured_code_2_record() {
        let rec = hex_bytes("1f4f0a00 b50f0000 c5ffffff 5200 1d00 4500 4400");
        assert_eq!(rec.len(), 20);
        let r = QueueRecord::decode(&rec, Rate::Sps50).unwrap();
        assert_eq!(r.time_ms(), 675_615);
        approx(r.vbus_v(), 0.004021);
        approx(r.ibus_a(), -0.000059);
        approx(r.cc1_v(), 0.082);
        approx(r.cc2_v(), 0.029);
        approx(r.dp_v(), 0.069);
        approx(r.dm_v(), 0.068);
    }

    #[test]
    fn decode_code_0_uses_tenth_millivolt_units() {
        let rec = hex_bytes("1f4f0a00 b50f0000 c5ffffff 5200 1d00 4500 4400");
        let r = QueueRecord::decode(&rec, Rate::Sps2).unwrap();
        assert_eq!(r.time_ms(), 675_615);
        approx(r.vbus_v(), 0.004021);
        approx(r.ibus_a(), -0.000059);
        approx(r.cc1_v(), 0.0082);
        approx(r.cc2_v(), 0.0029);
        approx(r.dp_v(), 0.0069);
        approx(r.dm_v(), 0.0068);
    }

    #[test]
    fn decode_codes_1_to_3_use_millivolt_units() {
        let rec = hex_bytes("1f4f0a00 b50f0000 c5ffffff 5200 1d00 4500 4400");
        for rate in [Rate::Sps10, Rate::Sps50, Rate::Sps1000] {
            let r = QueueRecord::decode(&rec, rate).unwrap();
            approx(r.cc1_v(), 0.082);
            approx(r.dm_v(), 0.068);
            // VBUS / IBUS do not depend on the rate.
            approx(r.vbus_v(), 0.004021);
            approx(r.ibus_a(), -0.000059);
        }
    }

    #[test]
    fn decode_synthetic_record_full_range() {
        let mut rec = vec![0u8; 20];
        rec[0x00..0x04].copy_from_slice(&u32::MAX.to_le_bytes());
        rec[0x04..0x08].copy_from_slice(&20_000_000i32.to_le_bytes());
        rec[0x08..0x0C].copy_from_slice(&(-5_000_000i32).to_le_bytes());
        rec[0x0C..0x0E].copy_from_slice(&u16::MAX.to_le_bytes());
        rec[0x0E..0x10].copy_from_slice(&1u16.to_le_bytes());
        rec[0x10..0x12].copy_from_slice(&3_300u16.to_le_bytes());
        rec[0x12..0x14].copy_from_slice(&600u16.to_le_bytes());
        let r = QueueRecord::decode(&rec, Rate::Sps1000).unwrap();
        assert_eq!(r.time_ms(), u32::MAX);
        approx(r.vbus_v(), 20.0);
        approx(r.ibus_a(), -5.0);
        approx(r.cc1_v(), 65.535);
        approx(r.cc2_v(), 0.001);
        approx(r.dp_v(), 3.3);
        approx(r.dm_v(), 0.6);
    }

    #[test]
    fn decode_captured_queue_section_records() {
        let payload = section_payload(QUEUE_1268);
        assert_eq!(payload.len(), 63 * 20);
        let records: Vec<QueueRecord> = payload
            .as_chunks::<20>()
            .0
            .iter()
            .map(|c| QueueRecord::decode(c, Rate::Sps50).unwrap())
            .collect();
        assert_eq!(records.len(), 63);
        // First record: 6b160b00 b3100000 cdffffff 4f00 1c00 4b00 4c00.
        let r = &records[0];
        assert_eq!(r.time_ms(), 0x000B_166B);
        approx(r.vbus_v(), 0.004275);
        approx(r.ibus_a(), -0.000051);
        approx(r.cc1_v(), 0.079);
        approx(r.cc2_v(), 0.028);
        approx(r.dp_v(), 0.075);
        approx(r.dm_v(), 0.076);
        // Code 2 = 50 SPS: device time steps by 20 ms.
        assert_eq!(records[1].time_ms() - records[0].time_ms(), 20);
    }

    #[test]
    fn decode_short_record_is_error() {
        let rec = [0u8; 20];
        for len in [0usize, 1, 12, 19] {
            assert!(
                matches!(
                    QueueRecord::decode(&rec[..len], Rate::Sps2),
                    Err(DecodeError::Length { got, .. }) if got == len
                ),
                "len {len} should be a length error"
            );
        }
    }
}
