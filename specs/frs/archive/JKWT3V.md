---
title: Delete offline recordings
milestone: M_CD7M3M
status: archived
archived_at: 2026-10-29T22:23:37Z
id: fr_01M61PKCQNB3VEMC0G4YJKWT3V
created_at: 2026-10-28T16:48:24.821Z
changelog_category: Added
---

# Delete offline recordings

## Summary

The meter keeps up to 40 recordings and fills up over time.
This feature deletes one recording or all of them from the command line tool and the desktop app.
A backup is downloaded first and the user types a confirmation, because a delete cannot be undone.
It runs on the real meter only after the user approves the first try.

## Requirement

The library, CLI and desktop app delete one or all offline recordings by rewriting the meter's record table, only after a backup download and a typed confirmation, and verify the result by reading the list again.

## Acceptance Criteria

- AC-JKWT3V.1: The delete sends one SET_DATA (0x48) with attribute 0x28 carrying the rewritten table: the remaining 48-byte entries unchanged and in order, then four zero bytes, as documented in the protocol notes. A table builder with tests produces it from the decoded list.
- AC-JKWT3V.2: Before any delete the CLI and the app download the affected recordings to CSV (the existing download), and refuse to delete when the backup fails.
- AC-JKWT3V.3: `powerz records delete <n>|--all` requires `--i-understand` plus typing the record name (or "all") at a prompt; `--yes` skips only the prompt and still needs the flag. The desktop Records tab asks for the typed name in a dialog.
- AC-JKWT3V.4: After the write the list is read again and must equal the expected table; any difference is reported as an error with the backup path, and nothing is retried on its own.
- AC-JKWT3V.5: The guard allows SET_DATA 0x48 only with attribute 0x28 and only from this delete path; every other 0x48 attribute (calibration and the rest) stays refused, with tests.
- AC-JKWT3V.6: The simulator keeps a record table that delete changes, so CLI, desktop and e2e tests cover delete one, delete all, a failed backup and a mismatching re-read.

## Technical Design

Table builder and delete in the protocol crate beside the record list decoder; the flash guard gains one narrow allow rule for 0x48/0x28. The CLI and the desktop reuse the download for the backup.

## Testing

Unit tests for the table builder (byte layout, empty table, last entry), guard tests, simulator and CLI tests, a Playwright test for the dialog. The real-meter run is in the hardware FR of this milestone.

## Notes

User decision 2026-10-28: build delete in M6b; the first real delete runs only with the user's OK.

Size limit: the 10-bit `size` field cannot encode a table over 1023 bytes (22+ remaining entries). The vendor wraps it; the meter's handling is unknown (encrypted firmware, no probing writes), so we refuse such a delete with a clear message and a test.

Real meter, 2026-10-29: the KM003C answers the record-table write with FINISHED (`07 <id> 00 00`), not ACCEPT; the first run exited 5 on a delete that had worked, fixed in `cd63907` (ACCEPT or FINISHED is success). Three deletes then checked out byte for byte: the last row, a middle row (the entry behind it kept byte-identical, offset unchanged) and the remaining throwaway. Delete-all is covered by simulator tests only.

## Implementation notes

- the meter answers the record-table write with FINISHED, not ACCEPT [device]; the session takes either, and the read-back decides (fixed in `cd63907`)
- Phase 3 review: after a failed delete the desktop panel lists again so its rows match the meter, keeping the delete's error and backup path (`46a3c5a`); a delete refused before touching the meter (wrong typed name) costs one extra harmless read
- the 1023-byte table limit stays an open question: no real meter with 22+ remaining records was available
