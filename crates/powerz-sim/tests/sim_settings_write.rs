//! AC-JKWT3W.6: on its CDC persona the demo meter takes a settings write
//! (type 0x0A) and serves the new values in its SETTINGS block afterwards,
//! with valid CRCs and everything else unchanged. A write that carries a key
//! outside the documented table (such as the factory reset 0x08) is
//! REJECTed and changes nothing. The `ignore-settings-write` fault ACCEPTs
//! the write but keeps the old block, so the re-read does not match. On the
//! bulk persona a settings write gets no reply and changes nothing, as on
//! the KM003C (2026-10-29) `[device]`.

mod common;

use common::*;
use powerz_protocol::decode::Rate;
use powerz_protocol::decode::Settings;
use powerz_protocol::decode::settings::{
    DimTimeout, Language, MeterSettings, Rotation, Theme, WritableField,
};
use powerz_protocol::frame::Attribute;
use powerz_protocol::transport::Transport;

/// Session on the CDC persona, where the settings write is taken (no
/// CONNECT: 0x02 is ADCNEW on CDC).
fn cdc_with(faults: FaultPlan) -> SimSession {
    let clock = clock();
    let dev = device(Persona::Cdc, Scenario::Idle, &clock).with_faults(faults);
    session(dev, &clock)
}

/// A raw type-0x0A frame with `entries`, `next` set on all but the last.
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

fn raw_settings(d: &mut SimDevice, id: u8) -> Vec<u8> {
    let reply = d
        .transact(&get_data(id, Attribute::SETTINGS), T)
        .expect("SETTINGS reply");
    sections(&reply)
        .into_iter()
        .find(|(att, _)| *att == Attribute::SETTINGS.bits())
        .map(|(_, p)| p)
        .expect("a SETTINGS section")
}

#[test]
fn a_write_changes_the_fields_the_meter_serves_afterwards() {
    let mut s = cdc_with(FaultPlan::new());
    let before = s.read_meter_settings().expect("read");
    assert_eq!(before.brightness.value, 50);
    let after = s
        .write_settings(&[
            (WritableField::Brightness, 80),
            (WritableField::Theme, 2),
            (WritableField::Rotation, 2),
            (WritableField::Language, 0),
            (WritableField::DimTimeout, 2),
        ])
        .expect("write");
    assert_eq!(after.brightness.value, 80);
    assert_eq!(after.theme.value, Theme::Pink);
    assert_eq!(after.rotation.value, Rotation::Deg180);
    assert_eq!(after.language.value, Language::Chinese);
    assert_eq!(after.dim_timeout.value, DimTimeout::Secs30);
    // Everything not written is kept.
    assert_eq!(after.startup_logo, before.startup_logo);
    assert_eq!(after.auto_rotate, before.auto_rotate);
    assert_eq!(after.sample_rate, before.sample_rate);
    assert_eq!(after.voltage_cal, before.voltage_cal);
    assert_eq!(after.current_cal, before.current_cal);
    assert_eq!(after.offset_cal, before.offset_cal);
    // And it stays changed for later reads.
    assert_eq!(s.read_meter_settings().unwrap(), after);
}

#[test]
fn a_rate_write_changes_the_rate_bits() {
    let mut s = cdc_with(FaultPlan::new());
    let after = s
        .write_settings(&[(WritableField::SampleRate, 1)])
        .expect("write");
    assert_eq!(after.sample_rate.value, Rate::Sps10);
    let block = s.read_settings().unwrap();
    assert_eq!(block.rate_code(), 1);
}

#[test]
fn the_written_block_keeps_valid_crcs_and_its_name() {
    let clock = clock();
    let mut d = device(Persona::Cdc, Scenario::Idle, &clock);
    let entries = [
        (0x01u16, 1u32),
        (0x03, 33),
        (0x04, 1),
        (0x06, 0),
        (0x07, 6),
        (0x14, 3),
        (0x15, 2),
        (0x7FFF, 0x1234),
    ];
    let reply = d
        .transact_settings(&settings_frame(5, &entries), T)
        .expect("reply");
    assert_eq!(&reply[..2], &[0x07, 0x05], "FINISHED: {reply:02x?}");
    let p = raw_settings(&mut d, 6);
    let block = Settings::decode(&p).unwrap();
    assert!(block.block_a_crc_ok() && block.block_b_crc_ok());
    assert_eq!(block.name(), "POWER-Z");
    let m = MeterSettings::decode(&p).unwrap();
    assert_eq!(m.brightness.value, 33);
    assert_eq!(m.dim_timeout.value, DimTimeout::Secs15);
    assert_eq!(m.theme.value, Theme::Cyan);
    assert_eq!(m.rotation.value, Rotation::Deg270);
}

#[test]
fn a_write_with_the_factory_reset_key_is_rejected_and_changes_nothing() {
    let clock = clock();
    let mut d = device(Persona::Cdc, Scenario::Idle, &clock);
    let before = raw_settings(&mut d, 2);
    let frame = settings_frame(
        3,
        &[
            (0x01, 1),
            (0x03, 10),
            (0x04, 0),
            (0x06, 0),
            (0x07, 0),
            (0x08, 1),
            (0x14, 2),
            (0x15, 1),
            (0x7FFF, 0x1234),
        ],
    );
    // The sim checks the frame itself, even when no guard is in front.
    let reply = d.transact(&frame, T).expect("reply");
    assert_eq!(ty(&reply), 0x06, "REJECT: {reply:02x?}");
    assert_eq!(raw_settings(&mut d, 4), before);
}

#[test]
fn keys_0x14_and_0x15_land_where_the_meter_put_them() {
    // On the KM003C (2026-10-29) 0x14 = 2 and 0x15 = 1 read back as
    // rotation 180° (bits 0..1 of 0x60) and 10 SPS (bits 2..3) `[device]`.
    let clock = clock();
    let mut d = device(Persona::Cdc, Scenario::Idle, &clock);
    let entries = [
        (0x01u16, 1u32),
        (0x03, 50),
        (0x04, 0),
        (0x06, 0),
        (0x07, 0),
        (0x14, 2),
        (0x15, 1),
        (0x7FFF, 0x1234),
    ];
    let reply = d
        .transact_settings(&settings_frame(3, &entries), T)
        .expect("reply");
    assert_eq!(&reply[..2], &[0x07, 0x03], "FINISHED: {reply:02x?}");
    let m = MeterSettings::decode(&raw_settings(&mut d, 4)).unwrap();
    assert_eq!(m.rotation.value, Rotation::Deg180);
    assert_eq!(m.sample_rate.value, Rate::Sps10);
}

#[test]
fn on_bulk_a_settings_write_gets_no_reply_and_changes_nothing() {
    let clock = clock();
    let mut d = raw_connected(Scenario::Idle, &clock);
    let before = raw_settings(&mut d, 2);
    let entries = [
        (0x01u16, 1u32),
        (0x03, 20),
        (0x04, 0),
        (0x06, 0),
        (0x07, 1),
        (0x14, 2),
        (0x15, 1),
        (0x7FFF, 0x1234),
    ];
    assert_eq!(
        d.transact_settings(&settings_frame(3, &entries), T),
        Err(powerz_protocol::transport::Error::Timeout)
    );
    assert_eq!(raw_settings(&mut d, 4), before);
}

#[test]
fn ignore_settings_write_accepts_but_keeps_the_old_block() {
    let mut s = cdc_with(FaultPlan::new().ignore_settings_write());
    let before = s.read_meter_settings().unwrap();
    let err = s
        .write_settings(&[(WritableField::Brightness, 80)])
        .expect_err("the re-read does not match");
    assert!(err.to_string().contains("brightness"), "{err}");
    assert_eq!(s.read_meter_settings().unwrap(), before);
}

#[test]
fn the_fault_parses_from_its_cli_name() {
    assert_eq!(
        FaultPlan::parse("ignore-settings-write"),
        Ok(FaultPlan::new().ignore_settings_write())
    );
}
