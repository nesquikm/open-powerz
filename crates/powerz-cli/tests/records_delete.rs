//! `powerz records delete <n>|--all` (FR JKWT3V, AC .2, .3 and .4) against
//! scripted sessions that pin every byte sent: `--i-understand` is required
//! (also with `--yes`), the record name (or "all") is typed at a prompt
//! unless `--yes` is given, the affected records are backed up to CSV first
//! (the `records get` download) and nothing is deleted when that fails, one
//! SET_DATA 0x48 / att 0x28 carries the rewritten table, and a re-read that
//! differs is an error naming the backup. All entries and samples are
//! synthetic.

mod common;

use std::path::{Path, PathBuf};

use clap::Parser;
use common::*;
use powerz_cli::args::RecordsAction;
use powerz_cli::commands::{DeleteOptions, RecordSelection, records_delete};
use powerz_cli::{Cli, CliError, Command};
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::frame::SectionHeader;
use powerz_protocol::session::memory_read_request;
use powerz_protocol::transport::mock::Step;

const CSV_HEADER: &str = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";

struct E {
    name: &'static str,
    points: u16,
    offset: u32,
}

const REC_A: E = E {
    name: "REC001",
    points: 2,
    offset: 0x0000_1000,
};
const REC_B: E = E {
    name: "charger test",
    points: 3,
    offset: 0x0001_0000,
};
const REC_C: E = E {
    name: "LAST",
    points: 4,
    offset: 0x0002_0000,
};

fn entry_bytes(e: &E) -> Vec<u8> {
    let mut b = vec![0u8; 48];
    b[..e.name.len()].copy_from_slice(e.name.as_bytes());
    b[0x10] = 0x77;
    b[0x11] = 1;
    b[0x12..0x14].copy_from_slice(&e.points.to_le_bytes());
    b[0x14..0x18].copy_from_slice(&10_000u32.to_le_bytes());
    b[0x18..0x1C].copy_from_slice(&u32::from(e.points - 1).to_le_bytes());
    b[0x24..0x28].copy_from_slice(&e.offset.to_le_bytes());
    b[0x28..0x30].copy_from_slice(&[9, 8, 7, 6, 5, 4, 3, 2]);
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

fn window(e: &E) -> (u32, u32) {
    (0x9810_0000 + e.offset, u32::from(e.points) * 16)
}

/// One memory read (every record here is at most 256 bytes) answered with
/// samples: VBUS 5 V + k mV, IBUS k mA.
fn download_steps(id: u8, e: &E) -> Vec<Step> {
    let (addr, len) = window(e);
    let plain: Vec<u8> = (0..i32::from(e.points))
        .flat_map(|k| {
            [5_000_000 + k * 1_000, k * 1_000, k, 5 * k]
                .into_iter()
                .flat_map(i32::to_le_bytes)
        })
        .collect();
    let mut confirm = vec![0xC4, id, 0x01, 0x01];
    confirm.extend(&powerz_protocol::session::memory_read_plaintext(addr, len)[..16]);
    vec![
        Step::transact(memory_read_request(id, addr, len), confirm),
        Step::read(crypto::encrypt(&KEYS[0], &plain)),
    ]
}

/// The documented SET_DATA 0x48 / att 0x28 frame, built by hand.
fn table_write(id: u8, remaining: &[&E]) -> Vec<u8> {
    let mut data: Vec<u8> = remaining.iter().flat_map(|e| entry_bytes(e)).collect();
    data.extend([0u8; 4]);
    let len = data.len() as u32;
    let mut f = (0x48u32 | (u32::from(id) << 8) | ((len / 4 + 2) << 22))
        .to_le_bytes()
        .to_vec();
    f.extend((0x28u32 | (len << 22)).to_le_bytes());
    f.extend(data);
    f
}

/// UID read (id 1) and auth (id 2, answered by [`Meter`]), the list (id 3)
/// of `entries`, then `rest`.
fn script(entries: &[&E], rest: Vec<Step>) -> Vec<Step> {
    [
        memread_steps(1, UID_ADDR, &SYNTH_UID),
        vec![Step::transact(get_data(3, 0x200), list_reply(3, entries))],
        rest,
    ]
    .concat()
}

fn out_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("powerz-delete-{}-{name}", std::process::id()));
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

fn opts(dir: &Path, i_understand: bool, yes: bool) -> DeleteOptions {
    DeleteOptions {
        i_understand,
        yes,
        out: dir.to_path_buf(),
    }
}

fn set_data_frames(written: &[Vec<u8>]) -> Vec<Vec<u8>> {
    written
        .iter()
        .filter(|f| f[0] & 0x7F == 0x48)
        .cloned()
        .collect()
}

/// The happy path for deleting record 2 of [A, B, C]: backup of B (id 4),
/// the write (id 5), the re-read (id 6).
fn delete_b_script(reread: &[&E]) -> Vec<Step> {
    script(
        &[&REC_A, &REC_B, &REC_C],
        [
            download_steps(4, &REC_B),
            vec![
                Step::transact(table_write(5, &[&REC_A, &REC_C]), accept(5)),
                Step::transact(get_data(6, 0x200), list_reply(6, reread)),
            ],
        ]
        .concat(),
    )
}

// ---- AC-JKWT3V.3: argument parsing ----

#[test]
fn records_delete_parses_an_index_or_all_with_the_flags() {
    let c = Cli::try_parse_from(["powerz", "records", "delete", "2", "--i-understand"]).unwrap();
    assert_eq!(
        c.command,
        Command::Records {
            action: RecordsAction::Delete {
                index: Some(2),
                all: false,
                i_understand: true,
                yes: false,
                out: PathBuf::from("."),
            }
        }
    );
    let c = Cli::try_parse_from([
        "powerz",
        "records",
        "delete",
        "--all",
        "--i-understand",
        "--yes",
        "--out",
        "bk",
    ])
    .unwrap();
    assert_eq!(
        c.command,
        Command::Records {
            action: RecordsAction::Delete {
                index: None,
                all: true,
                i_understand: true,
                yes: true,
                out: PathBuf::from("bk"),
            }
        }
    );
    // Exactly one of <n> and --all.
    assert!(Cli::try_parse_from(["powerz", "records", "delete", "--i-understand"]).is_err());
    assert!(
        Cli::try_parse_from([
            "powerz",
            "records",
            "delete",
            "1",
            "--all",
            "--i-understand"
        ])
        .is_err()
    );
}

// ---- AC-JKWT3V.3: --i-understand and the typed confirmation ----

#[test]
fn delete_without_i_understand_is_refused_before_anything_is_sent() {
    for yes in [false, true] {
        let dir = out_dir(&format!("noflag{yes}"));
        let mut s = session(Meter::new(delete_b_script(&[&REC_A, &REC_C])));
        let mut input: &[u8] = b"charger test\n";
        let mut out = Vec::new();
        let err = records_delete(
            &mut s,
            RecordSelection::One(2),
            &opts(&dir, false, yes),
            &mut input,
            &mut out,
        )
        .expect_err("refused");
        assert!(matches!(err, CliError::Usage(_)), "yes={yes}: {err:?}");
        assert!(err.to_string().contains("--i-understand"), "{err}");
        assert!(
            s.transport().inner.written().is_empty(),
            "yes={yes}: something was sent"
        );
        assert!(files_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn delete_with_the_typed_name_backs_up_writes_the_table_and_verifies_it() {
    let dir = out_dir("one");
    let mut s = session(Meter::new(delete_b_script(&[&REC_A, &REC_C])));
    let mut input: &[u8] = b"charger test\n";
    let mut out = Vec::new();
    records_delete(
        &mut s,
        RecordSelection::One(2),
        &opts(&dir, true, false),
        &mut input,
        &mut out,
    )
    .expect("delete succeeds");

    // AC-JKWT3V.2 / .1 / .4: auth, list, backup read, one write, re-read.
    assert_eq!(
        s.transport().inner.written(),
        &[
            memory_read_request(1, UID_ADDR, 12),
            get_data(3, 0x200),
            memory_read_request(4, 0x9811_0000, 48),
            table_write(5, &[&REC_A, &REC_C]),
            get_data(6, 0x200),
        ]
    );
    assert_eq!(s.transport().inner.remaining(), 0);

    // The backup is the `records get` CSV of record 2.
    assert_eq!(files_in(&dir), vec!["02_charger_test.csv".to_string()]);
    let csv = std::fs::read_to_string(dir.join("02_charger_test.csv")).unwrap();
    assert_eq!(csv.lines().next(), Some(CSV_HEADER));
    assert_eq!(csv.lines().count(), 4, "header + 3 rows");

    let out = text(&out);
    assert!(out.contains("charger test"), "{out}");
    assert!(
        out.contains("02_charger_test.csv"),
        "names the backup: {out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_with_a_wrongly_typed_name_deletes_nothing() {
    for typed in ["charger", "CHARGER TEST", "2", "all", ""] {
        let dir = out_dir("wrong");
        let mut s = session(Meter::new(delete_b_script(&[&REC_A, &REC_C])));
        let line = format!("{typed}\n");
        let mut input: &[u8] = line.as_bytes();
        let mut out = Vec::new();
        let r = records_delete(
            &mut s,
            RecordSelection::One(2),
            &opts(&dir, true, false),
            &mut input,
            &mut out,
        );
        assert!(r.is_err(), "typed {typed:?}");
        assert!(
            set_data_frames(s.transport().inner.written()).is_empty(),
            "typed {typed:?}: SET_DATA sent"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn delete_with_no_answer_at_the_prompt_deletes_nothing() {
    let dir = out_dir("eof");
    let mut s = session(Meter::new(delete_b_script(&[&REC_A, &REC_C])));
    let mut input: &[u8] = b"";
    let mut out = Vec::new();
    assert!(
        records_delete(
            &mut s,
            RecordSelection::One(2),
            &opts(&dir, true, false),
            &mut input,
            &mut out,
        )
        .is_err()
    );
    assert!(set_data_frames(s.transport().inner.written()).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn yes_skips_the_prompt_but_still_backs_up_and_verifies() {
    let dir = out_dir("yes");
    let mut s = session(Meter::new(delete_b_script(&[&REC_A, &REC_C])));
    // Nothing to read: --yes must not prompt.
    let mut input: &[u8] = b"";
    let mut out = Vec::new();
    records_delete(
        &mut s,
        RecordSelection::One(2),
        &opts(&dir, true, true),
        &mut input,
        &mut out,
    )
    .expect("delete succeeds");
    assert_eq!(
        set_data_frames(s.transport().inner.written()),
        vec![table_write(5, &[&REC_A, &REC_C])]
    );
    assert_eq!(files_in(&dir), vec!["02_charger_test.csv".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_all_needs_all_typed_and_backs_up_every_record() {
    let dir = out_dir("all");
    let script = script(
        &[&REC_A, &REC_B],
        [
            download_steps(4, &REC_A),
            download_steps(5, &REC_B),
            vec![
                Step::transact(table_write(6, &[]), accept(6)),
                Step::transact(get_data(7, 0x200), list_reply(7, &[])),
            ],
        ]
        .concat(),
    );
    let mut s = session(Meter::new(script));
    let mut input: &[u8] = b"all\n";
    let mut out = Vec::new();
    records_delete(
        &mut s,
        RecordSelection::All,
        &opts(&dir, true, false),
        &mut input,
        &mut out,
    )
    .expect("delete all succeeds");
    assert_eq!(
        s.transport().inner.written(),
        &[
            memory_read_request(1, UID_ADDR, 12),
            get_data(3, 0x200),
            memory_read_request(4, 0x9810_1000, 32),
            memory_read_request(5, 0x9811_0000, 48),
            table_write(6, &[]),
            get_data(7, 0x200),
        ]
    );
    assert_eq!(
        files_in(&dir),
        vec![
            "01_REC001.csv".to_string(),
            "02_charger_test.csv".to_string()
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_all_with_a_record_name_typed_deletes_nothing() {
    let dir = out_dir("allwrong");
    let mut s = session(Meter::new(script(
        &[&REC_A, &REC_B],
        [download_steps(4, &REC_A), download_steps(5, &REC_B)].concat(),
    )));
    let mut input: &[u8] = b"REC001\n";
    let mut out = Vec::new();
    assert!(
        records_delete(
            &mut s,
            RecordSelection::All,
            &opts(&dir, true, false),
            &mut input,
            &mut out,
        )
        .is_err()
    );
    assert!(set_data_frames(s.transport().inner.written()).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_of_a_record_not_on_the_meter_is_a_usage_error_and_sends_no_write() {
    for n in [0usize, 4, 99] {
        let dir = out_dir(&format!("range{n}"));
        let mut s = session(Meter::new(script(&[&REC_A, &REC_B, &REC_C], vec![])));
        let mut input: &[u8] = b"LAST\n";
        let mut out = Vec::new();
        let err = records_delete(
            &mut s,
            RecordSelection::One(n),
            &opts(&dir, true, true),
            &mut input,
            &mut out,
        )
        .expect_err("no such record");
        assert!(matches!(err, CliError::Usage(_)), "{n}: {err:?}");
        assert!(set_data_frames(s.transport().inner.written()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---- AC-JKWT3V.2: no delete without a backup ----

#[test]
fn a_failed_backup_download_deletes_nothing() {
    let dir = out_dir("backupfail");
    let (addr, len) = window(&REC_B);
    let mut s = session(Meter::new(script(
        &[&REC_A, &REC_B, &REC_C],
        vec![Step::transact(
            memory_read_request(4, addr, len),
            vec![0x06, 4, 0, 0],
        )],
    )));
    let mut input: &[u8] = b"charger test\n";
    let mut out = Vec::new();
    let err = records_delete(
        &mut s,
        RecordSelection::One(2),
        &opts(&dir, true, false),
        &mut input,
        &mut out,
    )
    .expect_err("backup failed");
    assert!(
        err.to_string().to_lowercase().contains("backup"),
        "says the backup failed: {err}"
    );
    assert!(set_data_frames(s.transport().inner.written()).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_backup_that_cannot_be_written_deletes_nothing() {
    let dir = out_dir("nodir");
    let missing = dir.join("does-not-exist");
    let mut s = session(Meter::new(delete_b_script(&[&REC_A, &REC_C])));
    let mut input: &[u8] = b"charger test\n";
    let mut out = Vec::new();
    assert!(
        records_delete(
            &mut s,
            RecordSelection::One(2),
            &opts(&missing, true, false),
            &mut input,
            &mut out,
        )
        .is_err()
    );
    assert!(set_data_frames(s.transport().inner.written()).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- AC-JKWT3V.4: a re-read that differs ----

#[test]
fn a_mismatching_re_read_is_a_device_error_naming_the_backup_and_is_not_retried() {
    let dir = out_dir("mismatch");
    // The meter accepted the write but still lists all three.
    let mut s = session(Meter::new(delete_b_script(&[&REC_A, &REC_B, &REC_C])));
    let mut input: &[u8] = b"charger test\n";
    let mut out = Vec::new();
    let err = records_delete(
        &mut s,
        RecordSelection::One(2),
        &opts(&dir, true, false),
        &mut input,
        &mut out,
    )
    .expect_err("mismatch");
    assert!(matches!(err, CliError::Device(_)), "{err:?}");
    let msg = err.to_string();
    assert!(
        msg.contains(&dir.display().to_string()),
        "names the backup: {msg}"
    );
    let written = s.transport().inner.written();
    assert_eq!(set_data_frames(written).len(), 1, "not retried");
    assert_eq!(written.last(), Some(&get_data(6, 0x200)));
    assert_eq!(s.transport().inner.remaining(), 0);
    assert_eq!(files_in(&dir), vec!["02_charger_test.csv".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- AC-4PJRVM.6: the refusal comes before the backup ----

#[test]
fn delete_leaving_22_entries_is_refused_before_the_backup() {
    let dir = out_dir("too-long");
    let long: Vec<E> = (0..23u32)
        .map(|k| E {
            name: Box::leak(format!("R{k:02}").into_boxed_str()),
            points: 2,
            offset: 0x1000 * (k + 1),
        })
        .collect();
    let entries: Vec<&E> = long.iter().collect();
    let mut s = session(Meter::new(script(&entries, vec![])));
    let mut out = Vec::new();
    let err = records_delete(
        &mut s,
        RecordSelection::One(1),
        &opts(&dir, true, true),
        &mut std::io::empty(),
        &mut out,
    )
    .expect_err("22 entries do not fit one table write");
    let text = err.to_string();
    assert!(text.contains("22 records"), "{text}");
    assert!(files_in(&dir).is_empty(), "no backup was written");
}
