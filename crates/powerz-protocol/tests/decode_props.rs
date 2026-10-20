//! Decoder robustness (FR E88WR5, AC .8): random bytes never panic any
//! decoder, and the length contract holds.

use powerz_protocol::decode::{
    AdcSnapshot, DeviceInfoBlock, OtpBlock, QueueRecord, Rate, Settings, Uid,
};
use proptest::collection::vec;
use proptest::prelude::*;

/// Random byte strings, biased toward the exact record lengths.
fn bytes() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        vec(any::<u8>(), 0..256),
        prop::sample::select(vec![
            0usize, 11, 12, 13, 19, 20, 21, 39, 40, 41, 43, 44, 45, 63, 64, 65, 179, 180, 181
        ])
        .prop_flat_map(|n| vec(any::<u8>(), n)),
    ]
}

/// Exercise every accessor so a panic anywhere surfaces.
fn touch_adc(s: &AdcSnapshot) -> f64 {
    s.vbus_v()
        + s.ibus_a()
        + s.vbus_avg_v()
        + s.ibus_avg_a()
        + s.vbus_avg_uncal_v()
        + s.ibus_avg_uncal_a()
        + s.temp_c().unwrap_or(0.0)
        + s.cc1_v()
        + s.cc2_v()
        + s.dp_v()
        + s.dm_v()
        + s.vdd_v()
        + s.power_w()
        + f64::from(s.rate_code().unwrap_or(0))
}

fn touch_queue(r: &QueueRecord) -> f64 {
    f64::from(r.time_ms()) + r.vbus_v() + r.ibus_a() + r.cc1_v() + r.cc2_v() + r.dp_v() + r.dm_v()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn decoders_never_panic_on_random_bytes(data in bytes(), code in any::<u8>(), sps in any::<u32>()) {
        let n = data.len();

        match AdcSnapshot::decode(&data) {
            Ok(s) => {
                prop_assert!(n >= 40);
                prop_assert!(touch_adc(&s).is_finite());
                prop_assert_eq!(s.rate_code().is_some(), n >= 44);
            }
            Err(_) => prop_assert!(n != 40 && n != 44),
        }
        if n == 40 || n == 44 {
            prop_assert!(AdcSnapshot::decode(&data).is_ok());
        }

        for rate in Rate::ALL {
            match QueueRecord::decode(&data, rate) {
                Ok(r) => {
                    prop_assert!(n >= 20);
                    prop_assert!(touch_queue(&r).is_finite());
                }
                Err(_) => prop_assert!(n != 20),
            }
        }

        match Settings::decode(&data) {
            Ok(s) => {
                prop_assert!(n >= 180);
                prop_assert!(s.rate_code() <= 3);
                let _ = (s.block_a_crc_ok(), s.block_b_crc_ok(), s.name());
                prop_assert_eq!(s.raw(), &data[..s.raw().len()]);
            }
            Err(_) => prop_assert!(n != 180),
        }

        match DeviceInfoBlock::decode(&data) {
            Ok(Some(info)) => {
                prop_assert!(n >= 64);
                prop_assert_ne!(&data[0..4], &[0xFF; 4][..]);
                for s in [info.model(), info.version(), info.date(), info.extra()] {
                    prop_assert!(s.chars().count() <= 12);
                    prop_assert!(!s.contains('\0'));
                }
            }
            Ok(None) => prop_assert_eq!(&data[0..4], &[0xFF; 4][..]),
            Err(_) => prop_assert!(n != 64),
        }

        match OtpBlock::decode(&data) {
            Ok(otp) => {
                prop_assert!(n >= 64);
                let serial = otp.serial();
                prop_assert!(!serial.contains(' '));
                prop_assert!(serial.chars().count() <= 64);
            }
            Err(_) => prop_assert!(n != 64),
        }

        match Uid::decode(&data) {
            Ok(uid) => {
                prop_assert_eq!(n, 12);
                let hex = uid.to_string();
                prop_assert_eq!(hex.len(), 24);
                prop_assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
            }
            Err(_) => prop_assert!(n != 12),
        }

        let from_code = Rate::from_code(code);
        prop_assert_eq!(from_code.is_some(), code <= 3);
        if let Some(r) = from_code {
            prop_assert_eq!(r.code(), code);
        }
        let from_sps = Rate::from_sps(sps);
        prop_assert_eq!(from_sps.is_some(), matches!(sps, 2 | 10 | 50 | 1000));
        if let Some(r) = from_sps {
            prop_assert_eq!(r.sps(), sps);
        }
    }
}
