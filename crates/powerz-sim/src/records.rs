//! Synthetic offline records ("files") of the demo meter: the RecordList
//! entries served for GET_DATA `0x200` and the 16-byte samples behind each
//! entry's `0x98100000 + offset` memory-read window.
//!
//! Every record is made up and deterministic. Layouts follow
//! `docs/protocol.md` (Offline records).

use powerz_protocol::decode::records::{ENTRY_LEN, FLASH_WINDOW_BASE, SAMPLE_LEN};

/// Largest bulk transfer the meter sends memory-read data in. \[static\]
pub(crate) const MAX_TRANSFER: usize = 4096;

/// One synthetic record: name, interval and a load profile.
struct Spec {
    name: &'static str,
    interval_s: u8,
    points: u16,
    /// Flash offset of the samples inside the record area.
    offset: u32,
    /// Nominal VBUS (V) and IBUS (A).
    vbus_v: f64,
    ibus_a: f64,
}

/// The demo meter's records. The first one is over 4096 bytes so a
/// download spans several transfers; windows do not overlap.
const SPECS: [Spec; 3] = [
    Spec {
        name: "PHONE_5V",
        interval_s: 1,
        points: 300,
        offset: 0x0000_0000,
        vbus_v: 5.1,
        ibus_a: 1.8,
    },
    Spec {
        name: "PD_9V_TEST",
        interval_s: 2,
        points: 120,
        offset: 0x0000_2000,
        vbus_v: 9.0,
        ibus_a: 2.0,
    },
    Spec {
        name: "LAPTOP_20V",
        interval_s: 5,
        points: 60,
        offset: 0x0000_3000,
        vbus_v: 20.0,
        ibus_a: 3.0,
    },
];

/// `(vbus µV, ibus µA, charge µAh, energy µWh)` of every sample of `spec`.
fn sample_values(spec: &Spec) -> Vec<[i32; 4]> {
    let dt_h = f64::from(spec.interval_s) / 3600.0;
    let (mut charge, mut energy) = (0.0f64, 0.0f64);
    (0..spec.points)
        .map(|i| {
            let t = f64::from(i) * f64::from(spec.interval_s);
            let v = spec.vbus_v + 0.02 * (t * 0.11).sin();
            let a = spec.ibus_a * (1.0 - 0.3 * (t / 600.0).min(1.0)) + 0.01 * (t * 0.37).sin();
            if i > 0 {
                charge += a * 1e6 * dt_h;
                energy += v * a * 1e6 * dt_h;
            }
            [
                (v * 1e6).round() as i32,
                (a * 1e6).round() as i32,
                charge.round() as i32,
                energy.round() as i32,
            ]
        })
        .collect()
}

/// The 48-byte RecordList entry of `spec`.
fn entry(spec: &Spec) -> [u8; ENTRY_LEN] {
    let last = sample_values(spec).last().copied().unwrap_or([0; 4]);
    let mut e = [0u8; ENTRY_LEN];
    e[..spec.name.len()].copy_from_slice(spec.name.as_bytes());
    e[0x11] = spec.interval_s;
    e[0x12..0x14].copy_from_slice(&spec.points.to_le_bytes());
    let duration = u32::from(spec.interval_s) * u32::from(spec.points.saturating_sub(1));
    e[0x18..0x1C].copy_from_slice(&duration.to_le_bytes());
    e[0x1C..0x20].copy_from_slice(&last[2].to_le_bytes());
    e[0x20..0x24].copy_from_slice(&last[3].to_le_bytes());
    e[0x24..0x28].copy_from_slice(&spec.offset.to_le_bytes());
    e
}

/// The RecordList payload: every entry back to back.
pub(crate) fn list_payload() -> Vec<u8> {
    SPECS.iter().flat_map(entry).collect()
}

/// Plain samples of the record whose window is exactly `(addr, len)`, or
/// `None` for any other window.
pub(crate) fn samples(addr: u32, len: u32) -> Option<Vec<u8>> {
    let spec = SPECS.iter().find(|s| {
        addr == FLASH_WINDOW_BASE + s.offset && len == u32::from(s.points) * SAMPLE_LEN as u32
    })?;
    Some(
        sample_values(spec)
            .into_iter()
            .flat_map(|w| w.into_iter().flat_map(i32::to_le_bytes))
            .collect(),
    )
}
