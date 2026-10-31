import type { ExportResult, PdOpened } from "./backend";

const pad = (n: number): string => String(n).padStart(2, "0");

/** The file name the PD capture save dialog proposes (FR TGE83E):
 * `powerz-pd-YYYYMMDD-HHMMSS.pdcap` in local time. */
export function defaultPdCaptureName(now: Date): string {
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `powerz-pd-${date}-${time}.pdcap`;
}

/** `1 record`, `1,234 records`. */
export function recordsText(records: number): string {
  return `${records.toLocaleString("en-US")} ${records === 1 ? "record" : "records"}`;
}

/** The notice shown once a PD capture was saved: records written and the file. */
export function pdSaveNoticeText(result: ExportResult): string {
  return `Saved ${recordsText(result.rows)} to ${result.path}`;
}

/** What the import and damage notices need of an opened capture: its counts, not its
 * rows and status samples (those live in the store). */
export type PdOpenedNotice = Omit<PdOpened, "rows" | "status"> & { rows: { length: number } };

/** The counts of `opened` the notices need. */
export function pdOpenedNotice(opened: PdOpened): PdOpenedNotice {
  return {
    path: opened.path,
    rows: { length: opened.rows.length },
    dropped: opened.dropped,
    damaged_line: opened.damaged_line,
    oversized: opened.oversized,
    skipped: opened.skipped,
    truncated: opened.truncated,
  };
}

/** The notice shown once a vendor PD log was imported (FR TGE83E, AC .6). */
export function pdImportNoticeText(imported: PdOpenedNotice): string {
  let text = `Imported ${recordsText(imported.rows.length)} from the vendor log ${imported.path}`;
  // What the import left out of a big or odd log.
  if (imported.oversized > 0) {
    const n = imported.oversized;
    text +=
      n === 1
        ? "; 1 row with oversized record bytes was skipped"
        : `; ${n.toLocaleString("en-US")} rows with oversized record bytes were skipped`;
  }
  if (imported.skipped > 0) {
    const n = imported.skipped;
    text +=
      n === 1
        ? "; 1 row without a whole record was skipped"
        : `; ${n.toLocaleString("en-US")} rows without a whole record were skipped`;
  }
  if (imported.truncated) {
    text += "; the log has more rows than the import takes, only the first ones were imported";
  }
  return text;
}

/** The notice shown when an opened PD capture file is damaged (AC-TGE83E.3): the
 * records loaded and the line where reading stopped; `null` for a whole file. */
export function pdOpenDamageText(opened: PdOpenedNotice): string | null {
  if (opened.damaged_line === null) return null;
  return `Loaded ${recordsText(opened.rows.length)}; the file is damaged at line ${opened.damaged_line.toLocaleString("en-US")}`;
}
