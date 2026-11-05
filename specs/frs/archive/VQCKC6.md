---
title: The firmware Writing stage explains the bytes the file stores past its image
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M6N685F4SQPGGSXWRCVQCKC6
created_at: 2026-11-05T06:27:28.356Z
changelog_category: Fixed
kind: bug
---

# The firmware Writing stage explains the bytes the file stores past its image

## Summary

In the r6 hardware run the Writing stage counted 456,912 bytes, but the file panel said 456,896.
The real 2.0.7 file stores 16 bytes past the image size it declares.
The update sends every stored byte, so the Writing count is right.
Nothing on screen said why the two figures differ.
The file panel and the progress section now say so.
What goes to the meter does not change.

## Requirement

### Steps to reproduce
1. Open the Firmware view and choose the 2.0.7 firmware file.
2. Start the update and read the Writing stage.

### Expected
Any two byte counts on the screen agree, or the screen says why they differ.

### Actual
The file panel reads "Size 456,896 bytes"; the Writing stage reads "Writing 456,912 / 456,912 bytes", with no explanation (r6 part F).

## Acceptance Criteria

- AC-VQCKC6.1: The file details the backend gives the page carry the stored payload length beside the declared image size. A Rust test on a container that stores 16 bytes more than it declares reads both figures.
- AC-VQCKC6.2: When the two differ, the file panel shows a "Sent to the meter" row after Size, for example "456,912 bytes (16 more than the image; the update sends all the file stores)". When they are equal, there is no such row and the panel is unchanged.
- AC-VQCKC6.3: When the write total differs from the chosen file's image size, the Update progress section shows one line under the stages: "The file stores 16 bytes past its 456,896-byte image, and the update sends all 456,912." The Writing stage keeps counting the bytes actually sent. When the two are equal, no line shows.
- AC-VQCKC6.4: Nothing sent to the meter changes: the flash streams the same stored bytes as before, and no USB command is added.
- AC-VQCKC6.5: Regression: Vitest covers the panel row and the progress line for differing and equal sizes. The mock's file stores 16 bytes past its image like the real file, its update counts the stored bytes, and a WebKit test reads the Sent to the meter row and the progress line during a mock update.

## Technical Design

### Root cause
`download` in `crates/powerz-protocol/src/firmware/flash.rs` streams `container.payload()`, so its Writing total is the stored length. `FirmwareFileInfo` carries only the declared `size`, so the page cannot explain the difference. The mock counted `info.size`, so no test saw it.

### Fix
`FirmwareFileInfo` gains `stored_size` (`c.payload().len()`), display only. `firmwareFileRows` adds the row when the two differ. A pure helper builds the progress line from the write total and the chosen file's size, and `FirmwareView` renders it. The mock file stores 456,912 bytes, and the mock update counts `stored_size`.

## Testing

Rust unit test in `firmware_file.rs`; Vitest on `firmwareFile` rows and the progress-line helper; one WebKit test on the mock update in the firmware specs.

## Notes

Source: M7b r6 hardware run, part F. Why the file stores 16 more bytes is not known [inferred: one cipher block of padding]; the CLI's first line already names the declared size. Related: V9SK53 (file panel), V9SK55 (progress).
