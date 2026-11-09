//! The rules that turn a recording file's rows into the readings a viewer
//! opens (FR AXHK2S, shared with the web edition by FR AHTZW3): which rows
//! are kept, skipped or left out, the line scale of each kept row, and the
//! file's own last CHARGE and ENERGY. The desktop app and the web edition
//! both open a file through [`FileRows`], so they keep the same rows.

use powerz_protocol::decode::{QueueRecord, Rate};

use crate::reader::{FileRow, RowOutcome};

/// The most rows a file opens with; [`FileRows::left_out`] counts the rest.
pub const MAX_ROWS: u64 = 10_000_000;

/// Rows between two progress reports, which is also how soon Cancel is seen.
pub const REPORT_EVERY: u64 = 8_192;

/// Highest CC or D± value the 0.1 mV scale holds, V.
const FINE_MAX_V: f64 = 6.5535;

/// The refusal of a file with no row kept.
pub const NO_READABLE_ROW: &str = "This file holds no readable row.";

/// Counts a file's lines and turns each kept row into a record.
#[derive(Debug, Clone)]
pub struct FileRows {
    /// The last CHARGE and ENERGY of the rows kept.
    charge_ah: Option<f64>,
    energy_wh: Option<f64>,
    rows: u64,
    skipped: u64,
    left_out: u64,
    /// Blank lines, which only count towards the next report.
    blank: u64,
    max_rows: u64,
    /// A row has needed the coarse line scale: the rest of the file keeps it.
    coarse: bool,
}

impl FileRows {
    /// Rules that keep at most `max_rows` rows.
    pub fn new(max_rows: u64) -> FileRows {
        FileRows {
            charge_ah: None,
            energy_wh: None,
            rows: 0,
            skipped: 0,
            left_out: 0,
            blank: 0,
            max_rows,
            coarse: false,
        }
    }

    /// Take in one line of the file: the record it keeps, if any.
    pub fn take(&mut self, outcome: RowOutcome) -> Option<QueueRecord> {
        match outcome {
            RowOutcome::Row(row) => self.push(&row),
            RowOutcome::Damaged => {
                self.skipped += 1;
                None
            }
            RowOutcome::Blank => {
                self.blank += 1;
                None
            }
        }
    }

    fn push(&mut self, row: &FileRow) -> Option<QueueRecord> {
        // A time past a record's reach is readable, and so are all after it.
        let Some(time_ms) = u32::try_from(row.elapsed_ms)
            .ok()
            .filter(|_| self.rows < self.max_rows)
        else {
            self.left_out += 1;
            return None;
        };
        self.coarse |= [row.cc1_v, row.cc2_v, row.dp_v, row.dm_v]
            .iter()
            .any(|&v| v > FINE_MAX_V);
        self.charge_ah = row.charge_ah.or(self.charge_ah);
        self.energy_wh = row.energy_wh.or(self.energy_wh);
        self.rows += 1;
        Some(record_of(row, time_ms, self.coarse))
    }

    /// Lines taken in so far: each one lands in exactly one count.
    pub fn lines(&self) -> u64 {
        self.rows + self.skipped + self.left_out + self.blank
    }

    /// Whether the line just taken in is one to report after.
    pub fn report_due(&self) -> bool {
        self.lines().is_multiple_of(REPORT_EVERY)
    }

    /// Rows kept.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Damaged rows skipped.
    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Readable rows past the row limit, or past the time a record holds.
    pub fn left_out(&self) -> u64 {
        self.left_out
    }

    /// The last CHARGE of the rows kept, Ah: a recording counts no charge
    /// across a pause or a break, so it wins over a sum of the rows.
    pub fn charge_ah(&self) -> Option<f64> {
        self.charge_ah
    }

    /// The last ENERGY of the rows kept, Wh, as [`charge_ah`](Self::charge_ah).
    pub fn energy_wh(&self) -> Option<f64> {
        self.energy_wh
    }

    /// Refuse a file with no row kept, in words for the user.
    pub fn check_any(&self) -> Result<(), &'static str> {
        if self.rows == 0 {
            Err(NO_READABLE_ROW)
        } else {
            Ok(())
        }
    }
}

/// `row` as a record at `time_ms`.
///
/// A record's rate sets the scale of its CC and D± codes, and a file names no
/// rate. A file starts on the fine scale and takes the coarse one at the first
/// row that needs it, so a 0.1 mV digit from 2 SPS and 7.5 V from 1000 SPS both
/// survive, and the history keeps at most one rate change however rows mix.
fn record_of(row: &FileRow, time_ms: u32, coarse: bool) -> QueueRecord {
    let lines = [row.cc1_v, row.cc2_v, row.dp_v, row.dm_v];
    let (per_volt, rate) = if coarse {
        (1_000.0, Rate::Sps1000)
    } else {
        (10_000.0, Rate::Sps2)
    };
    QueueRecord::from_raw(
        time_ms,
        (row.vbus_v * 1e6).round() as i32,
        (row.ibus_a * 1e6).round() as i32,
        lines.map(|v| (v * per_volt).round() as u16),
        rate,
    )
}
