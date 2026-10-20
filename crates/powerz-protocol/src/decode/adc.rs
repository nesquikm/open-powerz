//! ADC snapshot (`ATT_ADC`, 40 or 44 bytes). See `docs/protocol.md`.

use super::{DecodeError, i16_at, i32_at, micro, need, u16_at};

/// Minimum payload length (firmware without the rate byte).
const MIN_LEN: usize = 40;
/// Payload length that carries the rate byte.
const FULL_LEN: usize = 44;
/// Offset of the active queue rate code on 44-byte payloads.
const RATE_OFFSET: usize = 0x26;
/// Raw temperature value meaning "no reading".
const TEMP_NONE: i16 = i16::MIN; // 0x8000

/// One ADC snapshot, kept as raw integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdcSnapshot {
    vbus_uv: i32,
    ibus_ua: i32,
    vbus_avg_uv: i32,
    ibus_avg_ua: i32,
    vbus_avg_uncal_uv: i32,
    ibus_avg_uncal_ua: i32,
    temp_raw: i16,
    cc1: u16,
    cc2: u16,
    dp: u16,
    dm: u16,
    vdd: u16,
    rate_code: Option<u8>,
}

/// Convert a raw 0.1 mV value to volts.
fn ten_thousandths(raw: u16) -> f64 {
    f64::from(raw) / 10_000.0
}

impl AdcSnapshot {
    /// Decode a 40- or 44-byte ADC section payload.
    pub fn decode(payload: &[u8]) -> Result<AdcSnapshot, DecodeError> {
        need("ADC snapshot", payload, MIN_LEN)?;
        let p = payload;
        Ok(AdcSnapshot {
            vbus_uv: i32_at(p, 0x00),
            ibus_ua: i32_at(p, 0x04),
            vbus_avg_uv: i32_at(p, 0x08),
            ibus_avg_ua: i32_at(p, 0x0C),
            vbus_avg_uncal_uv: i32_at(p, 0x10),
            ibus_avg_uncal_ua: i32_at(p, 0x14),
            temp_raw: i16_at(p, 0x18),
            cc1: u16_at(p, 0x1A),
            cc2: u16_at(p, 0x1C),
            dp: u16_at(p, 0x1E),
            dm: u16_at(p, 0x20),
            vdd: u16_at(p, 0x22),
            rate_code: (p.len() >= FULL_LEN).then(|| p[RATE_OFFSET]),
        })
    }

    /// VBUS in volts.
    pub fn vbus_v(&self) -> f64 {
        micro(self.vbus_uv)
    }

    /// IBUS in amperes, signed.
    pub fn ibus_a(&self) -> f64 {
        micro(self.ibus_ua)
    }

    /// Average VBUS in volts.
    pub fn vbus_avg_v(&self) -> f64 {
        micro(self.vbus_avg_uv)
    }

    /// Average IBUS in amperes.
    pub fn ibus_avg_a(&self) -> f64 {
        micro(self.ibus_avg_ua)
    }

    /// Uncalibrated average VBUS in volts.
    pub fn vbus_avg_uncal_v(&self) -> f64 {
        micro(self.vbus_avg_uncal_uv)
    }

    /// Uncalibrated average IBUS in amperes.
    pub fn ibus_avg_uncal_a(&self) -> f64 {
        micro(self.ibus_avg_uncal_ua)
    }

    /// ADC chip temperature in °C, `None` when the raw value is 0x8000.
    pub fn temp_c(&self) -> Option<f64> {
        (self.temp_raw != TEMP_NONE).then(|| f64::from(self.temp_raw) / 128.0)
    }

    /// CC1 in volts.
    pub fn cc1_v(&self) -> f64 {
        ten_thousandths(self.cc1)
    }

    /// CC2 in volts.
    pub fn cc2_v(&self) -> f64 {
        ten_thousandths(self.cc2)
    }

    /// D+ in volts.
    pub fn dp_v(&self) -> f64 {
        ten_thousandths(self.dp)
    }

    /// D− in volts.
    pub fn dm_v(&self) -> f64 {
        ten_thousandths(self.dm)
    }

    /// Internal VDD in volts.
    pub fn vdd_v(&self) -> f64 {
        ten_thousandths(self.vdd)
    }

    /// Active queue rate code (byte 0x26); `None` on a 40-byte payload.
    pub fn rate_code(&self) -> Option<u8> {
        self.rate_code
    }

    /// Power in watts, VBUS × IBUS with sign.
    pub fn power_w(&self) -> f64 {
        self.vbus_v() * self.ibus_a()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::test_util::{approx, section_payload};

    const ADC_52: &str = include_str!("../../tests/vectors/adc_52.hex");

    fn captured() -> Vec<u8> {
        let p = section_payload(ADC_52);
        assert_eq!(p.len(), 44);
        p
    }

    /// Synthetic 44-byte payload with distinct values in every field.
    fn synthetic() -> Vec<u8> {
        let mut p = vec![0u8; 44];
        p[0x00..0x04].copy_from_slice(&5_123_456i32.to_le_bytes());
        p[0x04..0x08].copy_from_slice(&(-1_234_567i32).to_le_bytes());
        p[0x08..0x0C].copy_from_slice(&5_000_001i32.to_le_bytes());
        p[0x0C..0x10].copy_from_slice(&(-1_000_002i32).to_le_bytes());
        p[0x10..0x14].copy_from_slice(&4_999_003i32.to_le_bytes());
        p[0x14..0x18].copy_from_slice(&(-999_004i32).to_le_bytes());
        p[0x18..0x1A].copy_from_slice(&(-640i16).to_le_bytes()); // -5 °C
        p[0x1A..0x1C].copy_from_slice(&1_234u16.to_le_bytes());
        p[0x1C..0x1E].copy_from_slice(&2_345u16.to_le_bytes());
        p[0x1E..0x20].copy_from_slice(&3_456u16.to_le_bytes());
        p[0x20..0x22].copy_from_slice(&4_567u16.to_le_bytes());
        p[0x22..0x24].copy_from_slice(&33_000u16.to_le_bytes());
        p[0x24..0x26].copy_from_slice(&0x8000u16.to_le_bytes());
        p[0x26] = 3;
        p[0x27] = 0xF0;
        p
    }

    // ---- AC-E88WR5.1: layout, units, 40/44 lengths ----

    #[test]
    fn decode_44_byte_payload_all_fields_in_si_units() {
        let s = AdcSnapshot::decode(&synthetic()).unwrap();
        approx(s.vbus_v(), 5.123456);
        approx(s.ibus_a(), -1.234567);
        approx(s.vbus_avg_v(), 5.000001);
        approx(s.ibus_avg_a(), -1.000002);
        approx(s.vbus_avg_uncal_v(), 4.999003);
        approx(s.ibus_avg_uncal_a(), -0.999004);
        approx(s.temp_c().unwrap(), -5.0);
        approx(s.cc1_v(), 0.1234);
        approx(s.cc2_v(), 0.2345);
        approx(s.dp_v(), 0.3456);
        approx(s.dm_v(), 0.4567);
        approx(s.vdd_v(), 3.3);
        assert_eq!(s.rate_code(), Some(3));
    }

    #[test]
    fn decode_40_byte_payload_has_no_rate_code() {
        let p = synthetic();
        let s = AdcSnapshot::decode(&p[..40]).unwrap();
        approx(s.vbus_v(), 5.123456);
        approx(s.ibus_a(), -1.234567);
        approx(s.vdd_v(), 3.3);
        approx(s.dm_v(), 0.4567);
        assert_eq!(s.rate_code(), None);
    }

    #[test]
    fn decode_temperature_0x8000_is_none() {
        let mut p = synthetic();
        p[0x18..0x1A].copy_from_slice(&0x8000u16.to_le_bytes());
        assert_eq!(AdcSnapshot::decode(&p).unwrap().temp_c(), None);
    }

    #[test]
    fn decode_temperature_divides_i16_by_128() {
        let mut p = synthetic();
        p[0x18..0x1A].copy_from_slice(&0x7FFFi16.to_le_bytes());
        approx(
            AdcSnapshot::decode(&p).unwrap().temp_c().unwrap(),
            32767.0 / 128.0,
        );
        p[0x18..0x1A].copy_from_slice(&(-32767i16).to_le_bytes());
        approx(
            AdcSnapshot::decode(&p).unwrap().temp_c().unwrap(),
            -32767.0 / 128.0,
        );
    }

    #[test]
    fn decode_rate_code_reads_byte_0x26() {
        let mut p = synthetic();
        for code in 0..=3u8 {
            p[0x26] = code;
            assert_eq!(AdcSnapshot::decode(&p).unwrap().rate_code(), Some(code));
        }
    }

    #[test]
    fn decode_short_payloads_are_errors() {
        let p = synthetic();
        for len in [0usize, 1, 4, 20, 39] {
            assert!(
                matches!(
                    AdcSnapshot::decode(&p[..len]),
                    Err(DecodeError::Length { got, .. }) if got == len
                ),
                "len {len} should be a length error"
            );
        }
    }

    // ---- AC-E88WR5.2: captured firmware 2.0.6 payload ----

    #[test]
    fn decode_captured_fw_2_0_6_payload() {
        let s = AdcSnapshot::decode(&captured()).unwrap();
        approx(s.vbus_v(), 0.004196);
        approx(s.ibus_a(), -0.000095);
        approx(s.temp_c().unwrap(), 29.453125);
        approx(s.cc1_v(), 0.0805);
        approx(s.cc2_v(), 0.0294);
        approx(s.dp_v(), 0.0724);
        approx(s.dm_v(), 0.0754);
        approx(s.vdd_v(), 3.2048);
        assert_eq!(s.rate_code(), Some(2));
    }

    // ---- AC-E88WR5.3: power keeps the sign ----

    #[test]
    fn decode_power_w_is_signed_product() {
        let s = AdcSnapshot::decode(&synthetic()).unwrap();
        approx(s.power_w(), 5.123456 * -1.234567);
        assert!(s.power_w() < 0.0);

        let mut p = synthetic();
        p[0x04..0x08].copy_from_slice(&2_000_000i32.to_le_bytes());
        let s = AdcSnapshot::decode(&p).unwrap();
        approx(s.power_w(), 5.123456 * 2.0);

        let c = AdcSnapshot::decode(&captured()).unwrap();
        approx(c.power_w(), 0.004196 * -0.000095);
        assert!(c.power_w() < 0.0);
    }
}
