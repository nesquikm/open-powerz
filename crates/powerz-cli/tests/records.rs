//! `powerz records list` and `powerz records get <n>|--all` (FR 1HHT00,
//! AC .4 and .7): the offline-record table, CSV files in the recorder layout
//! and `--json`, against scripted sessions that pin every byte sent. All
//! entries and samples are synthetic.

mod common;

use std::path::{Path, PathBuf};

use clap::Parser;
use common::*;
use powerz_cli::args::RecordsAction;
use powerz_cli::commands::{RecordSelection, records_get, records_list};
use powerz_cli::{Cli, Command};
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::frame::SectionHeader;
use powerz_protocol::session::memory_read_request;
use powerz_protocol::transport::mock::Step;
use serde_json::Value;

const CSV_HEADER: &str = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";

struct E {
    name: &'static str,
    interval: u8,
    points: u16,
    duration: u32,
    charge: i32,
    energy: i32,
    offset: u32,
}

const REC_A: E = E {
    name: "REC001",
    interval: 2,
    points: 300,
    duration: 598,
    charge: 299,
    energy: 1495,
    offset: 0x0000_1000,
};

const REC_B: E = E {
    name: "charger test",
    interval: 1,
    points: 3,
    duration: 2,
    charge: 2,
    energy: 10,
    offset: 0x0001_0000,
};

fn entry_bytes(e: &E) -> Vec<u8> {
    let mut b = vec![0u8; 48];
    b[..e.name.len()].copy_from_slice(e.name.as_bytes());
    b[0x11] = e.interval;
    b[0x12..0x14].copy_from_slice(&e.points.to_le_bytes());
    b[0x18..0x1C].copy_from_slice(&e.duration.to_le_bytes());
    b[0x1C..0x20].copy_from_slice(&e.charge.to_le_bytes());
    b[0x20..0x24].copy_from_slice(&e.energy.to_le_bytes());
    b[0x24..0x28].copy_from_slice(&e.offset.to_le_bytes());
    b
}

fn list_reply(id: u8, entries: &[&E]) -> Vec<u8> {
    let payload: Vec<u8> = entries.iter().flat_map(|e| entry_bytes(e)).collect();
    let mut f = put_header(id, 1).to_vec();
    f.extend(
        SectionHeader {
            att: 0x200,
            next: false,
            chunk: 0,
            size: (payload.len() & 0x3FF) as u16,
        }
        .encode(),
    );
    f.extend(payload);
    f
}

/// Sample k: VBUS 5 V + k mV, IBUS -k mA, charge k µAh, energy 5k µWh.
fn samples(n: u16) -> Vec<u8> {
    (0..i32::from(n))
        .flat_map(|k| {
            [5_000_000 + k * 1_000, -k * 1_000, k, 5 * k]
                .into_iter()
                .flat_map(i32::to_le_bytes)
        })
        .collect()
}

fn window(e: &E) -> (u32, u32) {
    (0x9810_0000 + e.offset, u32::from(e.points) * 16)
}

fn download_steps(id: u8, e: &E) -> Vec<Step> {
    let (addr, len) = window(e);
    let mut confirm = vec![0xC4, id, 0x01, 0x01];
    confirm.extend(&powerz_protocol::session::memory_read_plaintext(addr, len)[..16]);
    let data = crypto::encrypt(&KEYS[0], &samples(e.points));
    let mut steps = vec![Step::transact(memory_read_request(id, addr, len), confirm)];
    for c in data.chunks(4096) {
        steps.push(Step::read(c.to_vec()));
    }
    steps
}

/// UID read (id 1) and auth (id 2, answered by [`Meter`]), then the list
/// (id 3) of `entries`, then `rest`.
fn script(entries: &[&E], rest: Vec<Step>) -> Vec<Step> {
    [
        memread_steps(1, UID_ADDR, &SYNTH_UID),
        vec![Step::transact(get_data(3, 0x200), list_reply(3, entries))],
        rest,
    ]
    .concat()
}

/// A fresh, empty output directory for this test.
fn out_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("powerz-records-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// Types of every frame written, auth excluded (the [`Meter`] answers 0x4C
/// without the script).
fn assert_read_only(written: &[Vec<u8>]) {
    for f in written {
        let ty = f[0] & 0x7F;
        assert!([0x0C, 0x44].contains(&ty), "unexpected frame {f:02x?}");
        assert_ne!(ty, 0x48, "SET_DATA sent");
        assert_ne!(ty, 0x4A, "flash write sent");
    }
}

fn near(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1e-9, "{what}: got {got}, want {want}");
}

// ---- AC-1HHT00.4: argument parsing ----

#[test]
fn records_list_parses_with_and_without_json() {
    let c = Cli::try_parse_from(["powerz", "records", "list"]).unwrap();
    assert_eq!(
        c.command,
        Command::Records {
            action: RecordsAction::List { json: false }
        }
    );
    let c = Cli::try_parse_from(["powerz", "records", "list", "--json"]).unwrap();
    assert_eq!(
        c.command,
        Command::Records {
            action: RecordsAction::List { json: true }
        }
    );
}

#[test]
fn records_get_parses_an_index_or_all_with_out_and_json() {
    let c = Cli::try_parse_from(["powerz", "records", "get", "2"]).unwrap();
    match c.command {
        Command::Records {
            action:
                RecordsAction::Get {
                    index,
                    all,
                    out,
                    json,
                },
        } => {
            assert_eq!(index, Some(2));
            assert!(!all);
            assert_eq!(out, PathBuf::from("."), "default output dir");
            assert!(!json);
        }
        other => panic!("{other:?}"),
    }
    let c = Cli::try_parse_from(["powerz", "records", "get", "--all", "--out", "dl", "--json"])
        .unwrap();
    match c.command {
        Command::Records {
            action:
                RecordsAction::Get {
                    index,
                    all,
                    out,
                    json,
                },
        } => {
            assert_eq!(index, None);
            assert!(all);
            assert_eq!(out, PathBuf::from("dl"));
            assert!(json);
        }
        other => panic!("{other:?}"),
    }
    // Exactly one of <n> and --all.
    assert!(Cli::try_parse_from(["powerz", "records", "get"]).is_err());
    assert!(Cli::try_parse_from(["powerz", "records", "get", "1", "--all"]).is_err());
}

// ---- AC-1HHT00.4: `records list` ----

#[test]
fn records_list_prints_a_table_row_per_record() {
    let mut s = session(Meter::new(script(&[&REC_A, &REC_B], vec![])));
    let mut out = Vec::new();
    records_list(&mut s, false, &mut out).unwrap();
    let out = text(&out);
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 3, "header + one line per record:\n{out}");
    let header = lines[0].to_lowercase();
    for col in [
        "name", "interval", "points", "duration", "charge", "energy", "offset",
    ] {
        assert!(header.contains(col), "no {col} column:\n{out}");
    }
    let a = lines[1];
    assert!(a.trim_start().starts_with('1'), "1-based index first: {a}");
    for want in ["REC001", "2 s", "300", "0:09:58", "Ah", "Wh", "00001000"] {
        assert!(a.contains(want), "{want:?} missing in {a}");
    }
    let b = lines[2];
    assert!(b.trim_start().starts_with('2'), "{b}");
    for want in ["charger test", "1 s", "3", "0:00:02", "00010000"] {
        assert!(b.contains(want), "{want:?} missing in {b}");
    }
    assert_eq!(
        s.transport().inner.written(),
        &[memory_read_request(1, UID_ADDR, 12), get_data(3, 0x200)]
    );
}

#[test]
fn records_list_with_no_records_says_so() {
    let mut s = session(Meter::new(script(&[], vec![])));
    let mut out = Vec::new();
    records_list(&mut s, false, &mut out).unwrap();
    assert!(
        text(&out).to_lowercase().contains("no offline records"),
        "{}",
        text(&out)
    );
}

#[test]
fn records_list_json_has_every_decoded_field() {
    let mut s = session(Meter::new(script(&[&REC_A, &REC_B], vec![])));
    let mut out = Vec::new();
    records_list(&mut s, true, &mut out).unwrap();
    let v: Value = serde_json::from_str(&text(&out)).expect("valid JSON");
    let recs = v["records"].as_array().expect("records array");
    assert_eq!(recs.len(), 2);
    let a = &recs[0];
    assert_eq!(a["index"], 1);
    assert_eq!(a["name"], "REC001");
    assert_eq!(a["interval_s"], 2);
    assert_eq!(a["points"], 300);
    assert_eq!(a["duration_s"], 598);
    near("charge_ah", a["charge_ah"].as_f64().unwrap(), 299e-6);
    near("energy_wh", a["energy_wh"].as_f64().unwrap(), 1495e-6);
    assert_eq!(a["offset"], 0x1000);
    assert_eq!(recs[1]["index"], 2);
    assert_eq!(recs[1]["name"], "charger test");

    let mut s = session(Meter::new(script(&[], vec![])));
    let mut out = Vec::new();
    records_list(&mut s, true, &mut out).unwrap();
    let v: Value = serde_json::from_str(&text(&out)).unwrap();
    assert_eq!(v["records"].as_array().map(Vec::len), Some(0));
}

// ---- AC-1HHT00.4 / .7: `records get <n>` ----

#[test]
fn records_get_one_writes_a_csv_in_the_recorder_layout() {
    let dir = out_dir("one");
    let mut s = session(Meter::new(script(
        &[&REC_A, &REC_B],
        download_steps(4, &REC_B),
    )));
    let mut out = Vec::new();
    records_get(&mut s, RecordSelection::One(2), &dir, false, &mut out).unwrap();

    // AC-1HHT00.7: auth, the list and exactly one memory read of record 2.
    assert_eq!(
        s.transport().inner.written(),
        &[
            memory_read_request(1, UID_ADDR, 12),
            get_data(3, 0x200),
            memory_read_request(4, 0x9811_0000, 48),
        ]
    );
    assert_read_only(s.transport().inner.written());

    assert_eq!(files_in(&dir), vec!["02_charger_test.csv".to_string()]);
    let csv = std::fs::read_to_string(dir.join("02_charger_test.csv")).unwrap();
    let mut lines = csv.lines();
    assert_eq!(lines.next(), Some(CSV_HEADER));
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    assert_eq!(rows.len(), 3, "{csv}");
    for (k, r) in rows.iter().enumerate() {
        assert_eq!(r.len(), 12, "{r:?}");
        assert_eq!(r[0], format!("0:00:{k:02}.000"), "elapsed = interval × k");
        assert_eq!(r[1], "", "no absolute start time is known");
        let f = |i: usize| r[i].parse::<f64>().unwrap_or_else(|_| panic!("{r:?}"));
        let kf = k as f64;
        near("VBUS", f(2), 5.0 + kf * 0.001);
        near("IBUS", f(3), -kf * 0.001);
        for (i, col) in [(4, "DP"), (5, "DM"), (6, "CC1"), (7, "CC2"), (8, "TEMP")] {
            assert_eq!(r[i], "", "{col} is not in offline records: {r:?}");
        }
        near("CHARGE", f(9), kf * 1e-6);
        near("ENERGY", f(10), 5.0 * kf * 1e-6);
        near("PWR", f(11), (5.0 + kf * 0.001) * (-kf * 0.001));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_get_out_of_range_is_a_usage_error_and_reads_no_samples() {
    for n in [0usize, 3, 99] {
        let dir = out_dir(&format!("range{n}"));
        let mut s = session(Meter::new(script(&[&REC_A, &REC_B], vec![])));
        let mut out = Vec::new();
        let err = records_get(&mut s, RecordSelection::One(n), &dir, false, &mut out)
            .expect_err("no such record");
        assert!(
            matches!(err, powerz_cli::CliError::Usage(_)),
            "{n}: {err:?}"
        );
        assert!(
            !s.transport()
                .inner
                .written()
                .iter()
                .any(|f| f[0] == 0x44 && f != &memory_read_request(1, UID_ADDR, 12)),
            "{n}: a record was read"
        );
        assert!(files_in(&dir).is_empty(), "{n}: a file was written");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn records_get_all_writes_one_csv_per_record_and_json_reports_them() {
    let dir = out_dir("all");
    let mut s = session(Meter::new(script(
        &[&REC_A, &REC_B],
        [download_steps(4, &REC_A), download_steps(5, &REC_B)].concat(),
    )));
    let mut out = Vec::new();
    records_get(&mut s, RecordSelection::All, &dir, true, &mut out).unwrap();
    assert_eq!(
        s.transport().inner.written(),
        &[
            memory_read_request(1, UID_ADDR, 12),
            get_data(3, 0x200),
            memory_read_request(4, 0x9810_1000, 300 * 16),
            memory_read_request(5, 0x9811_0000, 48),
        ]
    );
    assert_eq!(s.transport().inner.remaining(), 0);
    assert_eq!(
        files_in(&dir),
        vec![
            "01_REC001.csv".to_string(),
            "02_charger_test.csv".to_string()
        ]
    );
    let a = std::fs::read_to_string(dir.join("01_REC001.csv")).unwrap();
    assert_eq!(a.lines().count(), 301, "header + 300 rows");
    let last: Vec<&str> = a.lines().last().unwrap().split(',').collect();
    assert_eq!(last[0], "0:09:58.000");

    let v: Value = serde_json::from_str(&text(&out)).expect("valid JSON");
    let written = v["written"].as_array().expect("written array");
    assert_eq!(written.len(), 2);
    assert_eq!(written[0]["index"], 1);
    assert_eq!(written[0]["name"], "REC001");
    assert_eq!(written[0]["rows"], 300);
    let p = PathBuf::from(written[0]["path"].as_str().unwrap());
    assert_eq!(p.file_name().unwrap(), "01_REC001.csv");
    assert_eq!(written[1]["rows"], 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_get_text_names_each_written_file() {
    let dir = out_dir("text");
    let mut s = session(Meter::new(script(&[&REC_B], download_steps(4, &REC_B))));
    let mut out = Vec::new();
    records_get(&mut s, RecordSelection::One(1), &dir, false, &mut out).unwrap();
    let out = text(&out);
    assert!(out.contains("01_charger_test.csv"), "{out}");
    assert!(out.contains('3'), "row count: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_get_sanitizes_file_names_into_the_output_dir() {
    let dir = out_dir("sanitize");
    let evil = E {
        name: "../a/b\\c:d*?",
        ..REC_B
    };
    let mut s = session(Meter::new(script(&[&evil], download_steps(4, &evil))));
    let mut out = Vec::new();
    records_get(&mut s, RecordSelection::One(1), &dir, false, &mut out).unwrap();
    let files = files_in(&dir);
    assert_eq!(files.len(), 1, "{files:?}");
    let f = &files[0];
    assert!(f.starts_with("01_") && f.ends_with(".csv"), "{f}");
    for bad in ['/', '\\', ':', '*', '?'] {
        assert!(!f.contains(bad), "{f}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_get_all_skips_empty_records_without_reading_them() {
    let dir = out_dir("skip");
    let empty = E {
        name: "EMPTY",
        points: 0,
        ..REC_B
    };
    let mut s = session(Meter::new(script(
        &[&empty, &REC_B],
        download_steps(4, &REC_B),
    )));
    let mut out = Vec::new();
    records_get(&mut s, RecordSelection::All, &dir, false, &mut out).unwrap();
    assert_eq!(
        s.transport().inner.written().last().unwrap(),
        &memory_read_request(4, 0x9811_0000, 48)
    );
    assert_eq!(files_in(&dir), vec!["02_charger_test.csv".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}
