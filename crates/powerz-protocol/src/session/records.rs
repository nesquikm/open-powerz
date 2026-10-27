//! Offline records, read-only: list the meter's records (GET_DATA `0x200`
//! after level-1 authentication) and download one record's samples through
//! consecutive memory reads of at most 256 bytes. See `docs/protocol.md`.

use super::{Clock, Error, Session};
use crate::decode::records::{
    FLASH_WINDOW_BASE, RecordEntry, RecordSample, SAMPLE_LEN, decode_record_list, decode_samples,
};
use crate::frame::Attribute;
use crate::transport::Transport;

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
