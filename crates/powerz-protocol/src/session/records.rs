//! Offline records, read-only: list the meter's records (GET_DATA `0x200`
//! after level-1 authentication) and download one record's samples through
//! consecutive memory reads of at most 256 bytes. See `docs/protocol.md`.

use super::{Clock, Error, Session, unexpected_reply};
use crate::decode::records::{
    FLASH_WINDOW_BASE, RecordEntry, RecordSample, SAMPLE_LEN, decode_record_list, decode_samples,
    encode_record_table,
};
use crate::frame::{self, Attribute, Reply, SectionHeader};
use crate::transport::{self, Transport};

/// Attribute of the SET_DATA section that carries the record table
/// (defined next to the guard rule that admits it). \[static\]
pub use crate::transport::guard::RECORD_TABLE_ATT;

/// Largest table the 10-bit extended `size` field can describe. \[static\]
pub const RECORD_TABLE_MAX_LEN: usize = 0x3FF;

/// The SET_DATA (0x48) frame that rewrites the record table to `table`
/// (built by [`encode_record_table`](crate::decode::records::encode_record_table)):
/// main header `0x48 | id << 8 | ((len / 4) + 2) << 22`, one extended
/// section header (att 0x28, next 0, chunk 0, size `len`), then the table.
/// \[static\] `None` when the table is longer than
/// [`RECORD_TABLE_MAX_LEN`] bytes and so cannot be encoded.
pub fn record_table_request(id: u8, table: &[u8]) -> Option<Vec<u8>> {
    if table.len() > RECORD_TABLE_MAX_LEN {
        return None;
    }
    let len = u32::try_from(table.len()).ok()?;
    let header = 0x48u32 | (u32::from(id) << 8) | ((len / 4 + 2) << 22);
    let section = SectionHeader {
        att: RECORD_TABLE_ATT,
        next: false,
        chunk: 0,
        size: u16::try_from(len).ok()?,
    };
    let mut f = Vec::with_capacity(8 + table.len());
    f.extend_from_slice(&header.to_le_bytes());
    f.extend_from_slice(&section.encode());
    f.extend_from_slice(table);
    Some(f)
}

/// The entries a delete of `positions` keeps and the table that writes them,
/// without touching the meter, so a caller can refuse a delete before it
/// writes a backup. A position outside `list` fails with [`Error::Protocol`]
/// and a table longer than [`RECORD_TABLE_MAX_LEN`] with
/// [`Error::RecordTableTooLong`].
pub fn record_delete_table(
    list: &[RecordEntry],
    positions: &[usize],
) -> Result<(Vec<RecordEntry>, Vec<u8>), Error> {
    if let Some(bad) = positions.iter().find(|&&p| p >= list.len()) {
        return Err(Error::Protocol(format!(
            "record {bad} is not in the list of {} records",
            list.len()
        )));
    }
    let kept: Vec<RecordEntry> = list
        .iter()
        .enumerate()
        .filter(|(i, _)| !positions.contains(i))
        .map(|(_, e)| e.clone())
        .collect();
    let table = encode_record_table(&kept);
    if table.len() > RECORD_TABLE_MAX_LEN {
        return Err(Error::RecordTableTooLong {
            remaining: kept.len(),
            len: table.len(),
        });
    }
    Ok((kept, table))
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Read and decode the record list, authenticating at level 1 first
    /// when the session is below it. An empty list is not an error; a reply
    /// without a RecordList section is.
    pub fn list_records(&mut self) -> Result<Vec<RecordEntry>, Error> {
        // A failed listing must not leave the previous windows readable.
        self.record_windows.clear();
        if self.auth_level < 1 {
            self.authenticate(1)?;
        }
        let payload = self.get_section(Attribute::RECORD_LIST, "RecordList")?;
        let entries = decode_record_list(&payload)?;
        self.record_windows = entries
            .iter()
            .filter_map(RecordEntry::read_window)
            .collect();
        Ok(entries)
    }

    /// Delete the records at `positions` of `list` (the last
    /// [`list_records`](Self::list_records)) by rewriting the record table:
    /// one SET_DATA (attribute 0x28) carrying the other entries, unchanged
    /// and in order, then the list is read again. \[static\]
    ///
    /// The write succeeds on an ACCEPT or a FINISHED with the request id
    /// (the KM003C sends FINISHED \[device\]); a REJECT is
    /// [`Error::Rejected`] and any other reply [`Error::Protocol`].
    ///
    /// A position outside `list` fails with [`Error::Protocol`] and a table
    /// longer than [`RECORD_TABLE_MAX_LEN`] with [`Error::RecordTableTooLong`],
    /// both before anything is sent. A re-read that differs from the
    /// written table is [`Error::RecordTableMismatch`]; nothing is retried.
    /// Returns the list as read again.
    pub fn delete_records(
        &mut self,
        list: &[RecordEntry],
        positions: &[usize],
    ) -> Result<Vec<RecordEntry>, Error> {
        let (kept, table) = record_delete_table(list, positions)?;
        let request =
            record_table_request(self.counter.next(), &table).ok_or(Error::RecordTableTooLong {
                remaining: kept.len(),
                len: table.len(),
            })?;
        let timeout = transport::default_timeout(&request);
        let first = self.transport.transact_record_table(&request, timeout)?;
        let reply = self.reread_until_id(first, request[1], timeout, |_| false)?;
        match frame::classify_reply(&reply)? {
            // The KM003C answers FINISHED with the request id \[device\].
            Reply::Accept(_) | Reply::Finished(_) => {}
            other => return Err(unexpected_reply(&request, "SET_DATA", other)),
        }
        let after = self.list_records()?;
        if after != kept {
            return Err(Error::RecordTableMismatch {
                expected: kept.len(),
                actual: after.len(),
            });
        }
        Ok(after)
    }

    /// Download the samples of a record from the last
    /// [`list_records`](Self::list_records): its
    /// [`RecordEntry::read_window`] as consecutive memory reads of at most
    /// [`MEMORY_READ_MAX_LEN`](crate::decode::records::MEMORY_READ_MAX_LEN)
    /// bytes ([`RecordEntry::read_chunks`]), since
    /// the meter returns no more than that per read `[device]`. A record
    /// with no samples, one outside the record area or one that was not
    /// listed fails with [`Error::Forbidden`] before anything is sent.
    pub fn download_record(&mut self, entry: &RecordEntry) -> Result<Vec<RecordSample>, Error> {
        let (addr, len) = entry
            .read_window()
            .filter(|w| self.record_windows.contains(w))
            .ok_or(Error::Forbidden {
                addr: FLASH_WINDOW_BASE.wrapping_add(entry.offset),
                len: u32::from(entry.points).saturating_mul(SAMPLE_LEN as u32),
            })?;
        let mut data = Vec::with_capacity(len as usize);
        for (at, n) in entry.read_chunks() {
            // Every chunk lies inside the listed window.
            if at < addr || u64::from(at) + u64::from(n) > u64::from(addr) + u64::from(len) {
                return Err(Error::Forbidden { addr: at, len: n });
            }
            data.extend(self.memory_read_unchecked(at, n)?);
        }
        Ok(decode_samples(entry, &data)?)
    }
}
