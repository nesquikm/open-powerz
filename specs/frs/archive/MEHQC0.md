---
title: Desktop startup logo view
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6P7JAKNM3CQCS08GVMEHQC0
created_at: 2026-11-05T16:11:37.285Z
changelog_category: Added
---

# Desktop startup logo view

## Summary

The desktop app gets a Startup logo view.
The user picks a picture, sees the exact pixels and size that will be written, and confirms by typing.
The upload shows progress, cannot be cancelled once started, and ends with a clear status.
The view warns when the startup screen is off and can turn it on.
Remove brings back the built-in logo.

## Requirement

The desktop app offers preview, upload and remove of the startup logo, with typed confirmation, progress, a post-write status and the startup-duration hint, backed by Tauri commands and the mock backend.

## Acceptance Criteria

- AC-MEHQC0.1: A Startup logo view opens an image (PNG, JPEG, BMP or the raw logo file), with an opaque/transparent choice, and shows the preview at its real pixel size with width, height, blob size and chunk count, and a warning when a side is below 48.
- AC-MEHQC0.2: Upload and Remove each open a dialog that needs the word typed as shown before its button enables, the same style as record delete.
- AC-MEHQC0.3: During a write the view shows chunk and read-back progress, offers no cancel, and every control that sends to the meter is disabled; leaving the view or the meter list does not stop the write.
- AC-MEHQC0.4: After a write the view shows verified, or the failed step and chunk index with advice to run Upload or Remove again.
- AC-MEHQC0.5: The view shows the startup-screen duration; when it is off it says the logo will not show and offers 1, 3 or 5 s through the existing settings write.
- AC-MEHQC0.6: Upload and Remove are disabled with a reason for a model other than KM003C or before settings and model were read.
- AC-MEHQC0.7: The view's Read shows what the meter holds now (empty, the logo's size, or unrecognised) and can show the stored logo.
- AC-MEHQC0.8: Batched progress events go over a channel at no more than 20 per second. The mock backend implements every logo command, and Playwright covers preview, both confirmations, progress, a failure at chunk k and the duration hint on Chromium and WebKit, at 375 px width included.

## Technical Design

Backend: `logo.rs` in `src-tauri` with `logo_choose`, `logo_preview`, `logo_read`, `logo_upload`, `logo_remove` commands run on the device worker, progress over `tauri::ipc::Channel`. UI: `StartupLogoView.svelte`, `LogoConfirmDialog.svelte`, `lib/logo.svelte.ts` store; `backend.ts` and `mockBackend.ts` gain the commands.

## Testing

Rust command tests with the simulator; Vitest for the store; Playwright specs on the mock backend in both browsers and with the wide font.

## Notes

Image decoding happens in the backend, so the preview equals the bytes that would be written.
