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
The backups go into a folder the user picks, so each one is really written first.
The user types the record's name, or the word all, to confirm.
The meter's record table is rewritten once and read again to check it.

## Requirement

A WebUSB session deletes offline records with the desktop app's safeguards: a typed confirmation, a backup of every affected record first, one record-table write (SET_DATA 0x48 attribute 0x28) and a re-read that must match.

## Acceptance Criteria

- AC-W8SHXA.1: A delete reads the record list again first; the typed confirmation must equal the record's name (or "all" for every record), else nothing is deleted and nothing is written.
- AC-W8SHXA.2: Before the write, every affected record is downloaded and saved as a CSV file named as the desktop app names its backup files: written into a folder the user picks (Chrome and Edge), or as a browser download where the browser cannot write a folder. Every file is written before the table write; a failed download, a failed file write or a cancelled folder picker deletes nothing.
- AC-W8SHXA.3: The table write is the one frame the protocol core builds from the kept entries, unchanged and in order; a table too long to write is refused before the backup, and "all" on an empty list sends nothing.
- AC-W8SHXA.4: The write counts on ACCEPT or FINISHED; a REJECT or other reply fails. The list is read again and must equal the kept entries, else the delete fails naming the backup; nothing is retried.
- AC-W8SHXA.5: The answer lists the records after the delete and names the backup files; the records view shows them as on the desktop.
- AC-W8SHXA.6: No other SET_DATA attribute can be sent by this path; a HID session or a session without authentication refuses it; the web build switches the record delete feature on.

## Technical Design

Wasm exports the native delete-table planner and the record-table request builder (with the native guard check) plus the CSV writer. The driver runs list, backups, write and re-list inside its lock, like the native delete. Backups are written into the picked folder through the File System Access API, each write finished before the table write; without that API they are downloads. The answer names the folder.

## Testing

Wasm parity tests for the planner and frame; Vitest over the demo meter and fakes for the order (backup before write), confirmation, refusals and mismatch.

## Notes

First run on the real meter needs the user's OK in the HIL tester's tab.

## Hardware evidence (parity HIL 1)

- [x] AC-W8SHXA.1: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P7) typing "A01.d" kept Delete disabled for A02.d; "A02.d" enabled it.
- [x] AC-W8SHXA.2: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P7) wrote the backup `records-1794243513406/02_A02_d.csv` (15 points + header) into the folder the user picked, before the delete (P7-folder-listing.txt).
- [x] AC-W8SHXA.4 and .5: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P7) answered "Deleted. Backup: …" and the list showed only A01.d.
