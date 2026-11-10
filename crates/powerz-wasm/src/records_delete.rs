//! Deleting offline records in the browser (FR W8SHXA): the planner, the
//! backup file, the one record-table write (SET_DATA 0x48 att 0x28) and the
//! re-read check, as the native `Session::delete_records` and the desktop
//! worker do them. Sans-IO: the page moves the frames. Errors carry the
//! desktop app's wording.

use powerz_protocol::decode::records::decode_samples;
use powerz_protocol::frame::{self, Reply};
use powerz_protocol::session::{self, record_delete_table, record_table_request};
use powerz_protocol::transport::guard;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::BindError;
use crate::decoders::to_js;
use crate::meter_reads::{RecordBook, not_listed};

/// A record's backup as the desktop worker writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupFile {
    /// The desktop backup file name (`powerz_record::offline_record_file_name`).
    pub name: String,
    /// The CSV text (`powerz_record::offline_record_csv`).
    pub csv: String,
}

const MISMATCH_HINT: &str = "Read the list again; nothing was retried.";

/// A protocol-crate error of a record delete in the desktop app's words.
fn session_error(e: &session::Error) -> BindError {
    use session::Error as S;
    match e {
        S::RecordTableTooLong { .. } => BindError::with_hint(
            "usage",
            "Deleting that would leave more records than one table write can hold.",
            "Delete more records at once, or all of them.",
        ),
        S::RecordTableMismatch { expected, actual } => BindError::with_hint(
            "protocol",
            if expected == actual {
                format!(
                    "The record list does not match after the delete: \
                     {actual} records as expected, but the entries differ."
                )
            } else {
                format!(
                    "The record list does not match after the delete: \
                     the meter lists {actual} records, expected {expected}."
                )
            },
            MISMATCH_HINT,
        ),
        S::Decode(e) => BindError::new("malformed", e.to_string()),
        other => BindError::new("protocol", other.to_string()),
    }
}

impl RecordBook {
    /// 0-based positions of 1-based `indices` over the latest list; an
    /// unlisted one is the desktop app's refusal.
    fn positions(&self, indices: &[u32]) -> Result<Vec<usize>, BindError> {
        indices
            .iter()
            .map(|&index| {
                self.entry(index)?;
                usize::try_from(index - 1).map_err(|_| not_listed())
            })
            .collect()
    }

    /// The 1-based records a delete of `index` (`None` = every record)
    /// removes over the latest list. `confirm` must equal the record's name,
    /// or "all" for every record, else nothing is planned; a table too long
    /// to write is refused here, before any backup. "all" on an empty list
    /// plans nothing.
    pub fn delete_targets(&self, index: Option<u32>, confirm: &str) -> Result<Vec<u32>, BindError> {
        let (targets, expected) = match index {
            Some(index) => (vec![index], self.entry(index)?.name.as_str()),
            None => ((1..=self.entries.len() as u32).collect(), "all"),
        };
        if confirm != expected {
            return Err(BindError::new(
                "usage",
                "The typed confirmation does not match; nothing was deleted.",
            ));
        }
        if !targets.is_empty() {
            let positions = self.positions(&targets)?;
            record_delete_table(&self.entries, &positions).map_err(|e| session_error(&e))?;
        }
        Ok(targets)
    }

    /// Record `index`'s backup from its window's bytes (empty for a record
    /// with no samples), named and written as the desktop backup.
    pub fn backup_file(&self, index: u32, data: &[u8]) -> Result<BackupFile, BindError> {
        let entry = self.entry(index)?;
        let samples = decode_samples(entry, data)?;
        Ok(BackupFile {
            name: powerz_record::offline_record_file_name(index as usize, &entry.name),
            csv: powerz_record::offline_record_csv(entry, &samples),
        })
    }

    /// The SET_DATA 0x48 att 0x28 frame with request id `id` that keeps
    /// every listed record but `indices`, as the native delete builds it and
    /// checked by the native guard; remembers the kept entries.
    pub fn delete_request(&mut self, id: u8, indices: &[u32]) -> Result<Vec<u8>, BindError> {
        self.kept = None;
        let positions = self.positions(indices)?;
        let (kept, table) =
            record_delete_table(&self.entries, &positions).map_err(|e| session_error(&e))?;
        let request = record_table_request(id, &table).ok_or_else(|| {
            session_error(&session::Error::RecordTableTooLong {
                remaining: kept.len(),
                len: table.len(),
            })
        })?;
        guard::check_record_table_write(&request)
            .map_err(|e| BindError::new("forbidden", e.to_string()))?;
        self.kept = Some(kept);
        Ok(request)
    }

    /// Take the table write's reply: ACCEPT or FINISHED counts; a REJECT or
    /// any other reply fails.
    pub fn take_delete_reply(&mut self, reply: &[u8]) -> Result<(), BindError> {
        let reply =
            frame::classify_reply(reply).map_err(|e| BindError::new("protocol", e.to_string()))?;
        match reply {
            Reply::Accept(_) | Reply::Finished(_) => Ok(()),
            Reply::Reject(_) => Err(BindError::with_hint(
                "protocol",
                "The meter refused the request.",
                "Updating the meter's firmware may help.",
            )),
            _ => Err(BindError::new(
                "protocol",
                "The meter sent an unexpected reply to the record delete.",
            )),
        }
    }

    /// After a fresh `decode_list`: the listed entries must equal the kept
    /// ones of the last `delete_request`.
    pub fn check_after_delete(&self) -> Result<(), BindError> {
        let Some(kept) = &self.kept else {
            return Err(BindError::invalid(
                "the re-read check is out of order in a record delete",
            ));
        };
        if &self.entries != kept {
            return Err(session_error(&session::Error::RecordTableMismatch {
                expected: kept.len(),
                actual: self.entries.len(),
            }));
        }
        Ok(())
    }
}

#[wasm_bindgen]
impl RecordBook {
    /// [`delete_targets`](Self::delete_targets) for JS: `index` undefined
    /// for every record.
    #[wasm_bindgen(js_name = deleteTargets)]
    pub fn delete_targets_js(
        &self,
        index: Option<u32>,
        confirm: &str,
    ) -> Result<Vec<u32>, BindError> {
        self.delete_targets(index, confirm)
    }

    /// [`backup_file`](Self::backup_file) for JS: `{ name, csv }`.
    #[wasm_bindgen(js_name = backupFile)]
    pub fn backup_file_js(&self, index: u32, data: &[u8]) -> Result<JsValue, BindError> {
        to_js(&self.backup_file(index, data)?)
    }

    /// [`delete_request`](Self::delete_request) for JS.
    #[wasm_bindgen(js_name = deleteRequest)]
    pub fn delete_request_js(&mut self, id: u8, indices: &[u32]) -> Result<Vec<u8>, BindError> {
        self.delete_request(id, indices)
    }

    /// [`take_delete_reply`](Self::take_delete_reply) for JS.
    #[wasm_bindgen(js_name = takeDeleteReply)]
    pub fn take_delete_reply_js(&mut self, reply: &[u8]) -> Result<(), BindError> {
        self.take_delete_reply(reply)
    }

    /// [`check_after_delete`](Self::check_after_delete) for JS.
    #[wasm_bindgen(js_name = checkAfterDelete)]
    pub fn check_after_delete_js(&self) -> Result<(), BindError> {
        self.check_after_delete()
    }
}
