---
title: Delete offline records in the browser
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHXA
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Added
---

# Delete offline records in the browser

## Summary

The records view in the browser can now delete one record or all of them, as the desktop app does.
Before anything is deleted, each affected record is downloaded and saved as a CSV backup file.
The user types the record's name, or the word all, to confirm.
The meter's record table is rewritten once and read again to check it.

## Requirement

A WebUSB session deletes offline records with the desktop app's safeguards: a typed confirmation, a backup of every affected record first, one record-table write (SET_DATA 0x48 attribute 0x28) and a re-read that must match.

## Acceptance Criteria

- AC-W8SHXA.1: A delete reads the record list again first; the typed confirmation must equal the record's name (or "all" for every record), else nothing is deleted and nothing is written.
- AC-W8SHXA.2: Before the write, every affected record is downloaded and saved as a CSV download named as the desktop app names its backup files; a failed download deletes nothing.
- AC-W8SHXA.3: The table write is the one frame the protocol core builds from the kept entries, unchanged and in order; a table too long to write is refused before the backup, and "all" on an empty list sends nothing.
- AC-W8SHXA.4: The write counts on ACCEPT or FINISHED; a REJECT or other reply fails. The list is read again and must equal the kept entries, else the delete fails naming the backup; nothing is retried.
- AC-W8SHXA.5: The answer lists the records after the delete and names the backup files; the records view shows them as on the desktop.
- AC-W8SHXA.6: No other SET_DATA attribute can be sent by this path; a HID session or a session without authentication refuses it; the web build switches the record delete feature on.

## Technical Design

Wasm exports the native delete-table planner and the record-table request builder (with the native guard check) plus the CSV writer. The driver runs list, backups, write and re-list inside its lock, like the native delete. Backup files go through the page's download function; the answer's backup folder is the browser's downloads.

## Testing

Wasm parity tests for the planner and frame; Vitest over the demo meter and fakes for the order (backup before write), confirmation, refusals and mismatch.

## Notes

First run on the real meter needs the user's OK in the HIL tester's tab.
