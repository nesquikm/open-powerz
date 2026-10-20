//! The shared CSV recorder (FR 0P7HSJ): header, row format, integration
//! and the `CsvRecorder` over any `Write`.
//!
//! Every record here is synthetic.

use std::io::{self, Write};

use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_record::{CSV_HEADER, CsvRecorder, Integrator, format_elapsed, sig8};

/// Wall-clock origin used by the tests, ms since the Unix epoch.
const UNIX0: u64 = 1_790_000_000_000;

/// A 20-byte queue record: time, VBUS µV, IBUS µA, CC1, CC2, D+, D− raw.
fn rec(time_ms: u32, vbus_uv: i32, ibus_ua: i32, line: [u16; 4], rate: Rate) -> QueueRecord {
    let mut b = Vec::with_capacity(20);
    b.extend_from_slice(&time_ms.to_le_bytes());
    b.extend_from_slice(&vbus_uv.to_le_bytes());
    b.extend_from_slice(&ibus_ua.to_le_bytes());
    for v in line {
        b.extend_from_slice(&v.to_le_bytes());
    }
    QueueRecord::decode(&b, rate).expect("20-byte record")
}

/// 5 V / 1 A, CC1 1.68 V, D+ 0.6 V at 1000 SPS (1 mV line units).
fn five_one(time_ms: u32) -> QueueRecord {
    rec(
        time_ms,
        5_000_000,
        1_000_000,
        [1680, 0, 600, 0],
        Rate::Sps1000,
    )
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 + 1e-9 * b.abs()
}

fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8(bytes.to_vec())
        .expect("CSV is UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

// ---- AC-0P7HSJ.1: header, elapsed time, plain decimals, integrator ----

#[test]
fn csv_header_matches_vendor_recorder() {
    assert_eq!(
        CSV_HEADER,
        "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR"
    );
}

#[test]
fn format_elapsed_is_h_mm_ss_mmm_with_unpadded_hours() {
    assert_eq!(format_elapsed(0), "0:00:00.000");
    assert_eq!(format_elapsed(7), "0:00:00.007");
    assert_eq!(format_elapsed(61_005), "0:01:01.005");
    assert_eq!(format_elapsed(3_723_456), "1:02:03.456");
    assert_eq!(format_elapsed(100 * 3_600_000 + 59_999), "100:00:59.999");
}

#[test]
fn sig8_keeps_eight_significant_digits_in_plain_decimal_form() {
    assert_eq!(sig8(0.0), "0");
    assert_eq!(sig8(5.0), "5");
    assert_eq!(sig8(12.345678912), "12.345679");
    assert_eq!(sig8(-0.000012085555555555555), "-0.000012085556");
    assert_eq!(sig8(2.5e-12), "0.0000000000025");
    assert_eq!(sig8(123_456_789.4), "123456789");
    for x in [1e-7, -3.3e-9, 4.2e-15, 1.5e9] {
        assert!(!sig8(x).contains('e'), "{x} -> {}", sig8(x));
    }
}

#[test]
fn integrator_is_trapezoidal_signed_and_ignores_backward_steps() {
    let mut i = Integrator::new();
    assert_eq!(i.push(50_000, 5.0, 2.0), (0.0, 0.0));
    let (q, e) = i.push(53_600, 5.0, 2.0); // 3.6 s at 2 A / 10 W
    assert!(close(q, 0.002), "charge {q}");
    assert!(close(e, 0.01), "energy {e}");
    assert_eq!(i.push(100, 5.0, 2.0), (q, e), "a meter reboot adds nothing");

    let mut n = Integrator::new();
    n.push(0, 5.0, -2.0);
    let (q, e) = n.push(100, 5.0, -2.0);
    assert!(close(q, -2.0 * 0.1 / 3600.0), "charge {q}");
    assert!(close(e, -10.0 * 0.1 / 3600.0), "energy {e}");
}

// ---- AC-0P7HSJ.1 / .4: CsvRecorder rows ----

#[test]
fn a_new_recorder_writes_only_the_header() {
    let r = CsvRecorder::new(Vec::new()).expect("header written");
    assert_eq!(r.rows(), 0);
    let out = r.finish().expect("finish");
    assert_eq!(String::from_utf8(out).unwrap(), format!("{CSV_HEADER}\n"));
}

#[test]
fn rows_have_vendor_columns_with_time_from_the_first_record() {
    let mut r = CsvRecorder::new(Vec::new()).unwrap();
    r.push(&five_one(10_000), UNIX0).unwrap();
    // Later wall-clock readings are ignored: Unix = origin + device elapsed.
    r.push(&five_one(10_100), UNIX0 + 9_999).unwrap();
    assert_eq!(r.rows(), 2);
    let out = lines(&r.finish().unwrap());
    assert_eq!(out.len(), 3, "{out:?}");
    assert_eq!(out[0], CSV_HEADER);
    assert_eq!(out[1], "0:00:00.000,1790000000.000,5,1,0.6,0,1.68,0,,0,0,5");
    let charge = 1.0 * 0.1 / 3600.0;
    let energy = 5.0 * 0.1 / 3600.0;
    assert_eq!(
        out[2],
        format!(
            "0:00:00.100,1790000000.100,5,1,0.6,0,1.68,0,,{},{},5",
            sig8(charge),
            sig8(energy)
        )
    );
    assert_eq!(sig8(charge), "0.000027777778");
}

#[test]
fn charge_and_energy_count_from_the_recording_start_not_the_meter_uptime() {
    // A recording started an hour into the session still starts at 0 Ah.
    let mut r = CsvRecorder::new(Vec::new()).unwrap();
    r.push(&five_one(3_600_000), UNIX0).unwrap();
    r.push(&five_one(3_603_600), UNIX0).unwrap(); // +3.6 s at 1 A
    let out = lines(&r.finish().unwrap());
    let first: Vec<&str> = out[1].split(',').collect();
    let second: Vec<&str> = out[2].split(',').collect();
    assert_eq!(first[0], "0:00:00.000");
    assert_eq!((first[9], first[10]), ("0", "0"));
    assert_eq!(second[0], "0:00:03.600");
    assert_eq!((second[9], second[10]), ("0.001", "0.005"));
    assert_eq!(second[1], "1790000003.600");
}

#[test]
fn negative_current_and_two_sps_line_units_are_plain_decimals() {
    let mut r = CsvRecorder::new(Vec::new()).unwrap();
    // 2 SPS: line units are 0.1 mV.
    r.push(&rec(0, 4_021, -59, [820, 290, 450, 440], Rate::Sps2), UNIX0)
        .unwrap();
    let out = lines(&r.finish().unwrap());
    let row: Vec<&str> = out[1].split(',').collect();
    assert_eq!(row.len(), 12, "{row:?}");
    assert_eq!(row[2], "0.004021");
    assert_eq!(row[3], "-0.000059");
    assert_eq!(row[4], "0.045"); // D+
    assert_eq!(row[5], "0.044"); // D−
    assert_eq!(row[6], "0.082"); // CC1
    assert_eq!(row[7], "0.029"); // CC2
    assert_eq!(row[8], "", "TEMP stays empty");
    assert_eq!(row[11], sig8(0.004021 * -0.000059));
    assert!(out[1].chars().all(|c| c != 'e'), "{}", out[1]);
}

/// A writer that counts flushes and fails every write after `ok_bytes`.
struct Flaky {
    buf: Vec<u8>,
    ok_bytes: usize,
    flushes: usize,
}

impl Write for Flaky {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.buf.len() + data.len() > self.ok_bytes {
            return Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
fn flush_reaches_the_underlying_writer() {
    let mut r = CsvRecorder::new(Flaky {
        buf: Vec::new(),
        ok_bytes: usize::MAX,
        flushes: 0,
    })
    .unwrap();
    r.push(&five_one(0), UNIX0).unwrap();
    r.flush().unwrap();
    let w = r.finish().unwrap();
    assert!(w.flushes >= 1, "flush never reached the writer");
    assert_eq!(lines(&w.buf).len(), 2);
}

#[test]
fn a_write_error_is_returned() {
    let ok_bytes = CSV_HEADER.len() + 1 + 60;
    let mut r = CsvRecorder::new(Flaky {
        buf: Vec::new(),
        ok_bytes,
        flushes: 0,
    })
    .unwrap();
    r.push(&five_one(0), UNIX0).expect("first row fits");
    let mut failed = false;
    for k in 1..10 {
        if r.push(&five_one(k), UNIX0).is_err() {
            failed = true;
            break;
        }
    }
    // A buffering recorder may only see the error when it flushes.
    let failed = failed || r.flush().is_err();
    assert!(failed, "the writer's error never surfaced");
}

// ---- AC-0P7HSJ.1: the CLI uses the shared crate ----

fn read(path: &str) -> String {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

#[test]
fn the_cli_depends_on_powerz_record() {
    let manifest = read("../powerz-cli/Cargo.toml");
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|rest| rest.split("\n[").next())
        .expect("powerz-cli has a [dependencies] table");
    assert!(
        deps.lines()
            .any(|l| l.trim_start().starts_with("powerz-record")),
        "powerz-cli does not depend on powerz-record:\n{deps}"
    );
}

#[test]
fn the_cli_has_no_copy_of_the_recorder() {
    let mut sources = Vec::new();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../powerz-cli/src");
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                sources.push((path.clone(), std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    for (path, text) in &sources {
        for needle in [
            "pub fn sig8(",
            "pub struct Integrator",
            "pub fn format_elapsed(",
            "const CSV_HEADER: &str = \"",
        ] {
            assert!(
                !text.contains(needle),
                "{} still defines `{needle}`; it belongs in powerz-record",
                path.display()
            );
        }
    }
}
