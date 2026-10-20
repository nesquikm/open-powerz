//! AC-G0W08Z.2: ADC replies carry a 44-byte payload from the active
//! scenario; four named scenarios define VBUS/IBUS/CC/D± over simulated time
//! with the temperature between 25 and 35 °C.
//! AC-G0W08Z.3: SETTINGS replies are 180 bytes with valid CRCs, the name
//! `POWER-Z` and the meter rate code in bits 2..3 of 0x60.

mod common;

use common::*;
use powerz_protocol::decode::AdcSnapshot;
use powerz_protocol::frame::Attribute;
use powerz_protocol::transport::Transport;

// ---- AC-G0W08Z.2 ----

#[test]
fn four_scenarios_ship_with_their_cli_names() {
    let names: Vec<&str> = Scenario::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["idle", "load-5v-1a", "pd-9v-2a", "noisy-negative"]);
    for s in Scenario::ALL {
        assert_eq!(Scenario::from_name(s.name()), Some(s));
    }
    assert_eq!(Scenario::from_name("no-such-scenario"), None);
}

#[test]
fn adc_reply_is_52_bytes_with_a_44_byte_payload() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Load5v1a, &clock);
    clock.advance(ms(1500));
    let reply = d.transact(&get_data(9, Attribute::ADC), T).unwrap();
    assert_eq!(reply.len(), 52, "{reply:02x?}");
    let secs = sections(&reply);
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0].0, Attribute::ADC.bits());
    assert_eq!(secs[0].1.len(), 44);
}

#[test]
fn adc_snapshot_follows_the_scenario_at_simulated_time() {
    for scenario in Scenario::ALL {
        let (mut s, clock) = connected(scenario);
        for t in [1500u64, 4200, 9999] {
            let now = clock.now_ms() - T0;
            clock.advance(ms(t - now));
            let adc = s.read_adc().unwrap();
            let want = scenario.sample(t);
            let n = scenario.name();
            assert_near(&format!("{n} VBUS @{t}"), adc.vbus_v(), want.vbus_v, 2e-6);
            assert_near(&format!("{n} IBUS @{t}"), adc.ibus_a(), want.ibus_a, 2e-6);
            assert_near(&format!("{n} CC1 @{t}"), adc.cc1_v(), want.cc1_v, 2e-4);
            assert_near(&format!("{n} CC2 @{t}"), adc.cc2_v(), want.cc2_v, 2e-4);
            assert_near(&format!("{n} D+ @{t}"), adc.dp_v(), want.dp_v, 2e-4);
            assert_near(&format!("{n} D- @{t}"), adc.dm_v(), want.dm_v, 2e-4);
            let temp = adc.temp_c().expect("temperature is valid");
            assert_near(&format!("{n} temp @{t}"), temp, want.temp_c, 0.01);
        }
    }
}

#[test]
fn scenarios_are_deterministic_functions_of_time() {
    for scenario in Scenario::ALL {
        for t in [0u64, 1, 777, 123_456] {
            assert_eq!(scenario.sample(t), scenario.sample(t), "{scenario:?} @{t}");
        }
    }
}

fn times() -> impl Iterator<Item = u64> {
    (0..20_000).step_by(7)
}

#[test]
fn idle_is_near_zero() {
    for t in times() {
        let s = Scenario::Idle.sample(t);
        assert!(s.vbus_v.abs() < 0.05, "idle VBUS {} @{t}", s.vbus_v);
        assert!(s.ibus_a.abs() < 0.01, "idle IBUS {} @{t}", s.ibus_a);
    }
}

#[test]
fn load_5v_1a_and_pd_9v_2a_hold_their_operating_points() {
    for t in times() {
        let a = Scenario::Load5v1a.sample(t);
        assert_near(&format!("load-5v-1a VBUS @{t}"), a.vbus_v, 5.0, 0.25);
        assert_near(&format!("load-5v-1a IBUS @{t}"), a.ibus_a, 1.0, 0.05);
        let b = Scenario::Pd9v2a.sample(t);
        assert_near(&format!("pd-9v-2a VBUS @{t}"), b.vbus_v, 9.0, 0.45);
        assert_near(&format!("pd-9v-2a IBUS @{t}"), b.ibus_a, 2.0, 0.1);
    }
}

#[test]
fn pd_scenario_drives_a_cc_line() {
    let s = Scenario::Pd9v2a.sample(1000);
    assert!(
        s.cc1_v > 0.1 || s.cc2_v > 0.1,
        "a PD contract needs a pulled-up CC line: {s:?}"
    );
}

#[test]
fn noisy_negative_is_noisy_and_goes_negative() {
    let currents: Vec<f64> = times()
        .map(|t| Scenario::NoisyNegative.sample(t).ibus_a)
        .collect();
    let min = currents.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(min < 0.0, "current never goes negative (min {min})");
    let mean = currents.iter().sum::<f64>() / currents.len() as f64;
    let var = currents.iter().map(|i| (i - mean).powi(2)).sum::<f64>() / currents.len() as f64;
    assert!(var.sqrt() > 0.01, "current is not noisy (σ {})", var.sqrt());
    let mut distinct = currents.clone();
    distinct.sort_by(f64::total_cmp);
    distinct.dedup();
    assert!(
        distinct.len() > 100,
        "only {} distinct values",
        distinct.len()
    );
}

#[test]
fn temperature_stays_between_25_and_35_c() {
    for scenario in Scenario::ALL {
        for t in times() {
            let temp = scenario.sample(t).temp_c;
            assert!(
                (25.0..=35.0).contains(&temp),
                "{scenario:?} temp {temp} @{t}"
            );
        }
    }
    // And as decoded from the wire.
    let (mut s, clock) = connected(Scenario::Pd9v2a);
    for _ in 0..20 {
        clock.advance(ms(333));
        let temp = s.read_adc().unwrap().temp_c().expect("valid temperature");
        assert!((25.0..=35.0).contains(&temp), "decoded temp {temp}");
    }
}

#[test]
fn adc_payload_decodes_with_the_library_decoder() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Pd9v2a, &clock);
    clock.advance(ms(250));
    let reply = d.transact(&get_data(4, Attribute::ADC), T).unwrap();
    let payload = &sections(&reply)[0].1;
    let adc = AdcSnapshot::decode(payload).unwrap();
    assert_near("VBUS", adc.vbus_v(), 9.0, 0.45);
    assert_near("power", adc.power_w(), 18.0, 1.8);
}

// ---- AC-G0W08Z.3 ----

#[test]
fn settings_are_180_bytes_with_valid_crcs_name_and_rate_code() {
    let (mut s, _clock) = connected(Scenario::Idle);
    let settings = s.read_settings().unwrap();
    assert_eq!(settings.raw().len(), 180);
    assert!(settings.block_a_crc_ok(), "block A CRC-32");
    assert!(settings.block_b_crc_ok(), "block B CRC-32");
    assert_eq!(settings.name(), "POWER-Z");
    assert_eq!(settings.rate_code(), 2, "meter rate 50 SPS = code 2");
    let flags = u32::from_le_bytes(settings.raw()[0x60..0x64].try_into().unwrap());
    assert_eq!((flags >> 2) & 0b11, 2);
}

#[test]
fn settings_reply_is_188_bytes_with_one_section() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let reply = d.transact(&get_data(2, Attribute::SETTINGS), T).unwrap();
    assert_eq!(reply.len(), 188, "{} bytes", reply.len());
    let secs = sections(&reply);
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0].0, Attribute::SETTINGS.bits());
    assert_eq!(secs[0].1.len(), 180);
}
