---
fr: fr_01M65KVDYC49QF1V7N1Y4PJRVJ
impact_set: {"symbols":[{"name":"firmwareCancel","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"b5ae01b051fafb4e7cdf1a3fe41e1fe232b97cb724eaba2a17ab9840877d59fe"},{"name":"firmwareFlashStatus","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"f6e23b3223a7a0bd400987e515da2902f012629caf4b2a0a888916d3acbd7ece"},{"name":"ConnectedInfo","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"8afffa436f21098c84f9fd436347f8d6c63f0a733b1f97f8483de662c0733102"},{"name":"FlashStatus","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"c3e9e1ca6735a0427646f6a4846dd2b60de05a9f556563d2f98cdc7b94dc76ef"},{"name":"FirmwareWarningCode","file":"apps/desktop/src/lib/backend.ts","kind":"type","visibility":"public","change":"modified","signatureHash":"e2b2f182feba54cc316de0cd768929f85c49bbc950e756d5a89c559d3b24c70e"},{"name":"firmwareWarningText","file":"apps/desktop/src/lib/deviceDetails.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"4070824c5125f5e60eb40e66a0a22ff37de4a8d7ca6bb4d72ff8f753e45aa609"},{"name":"notesNumbered","file":"apps/desktop/src/lib/firmwareFile.ts","kind":"function","visibility":"public","change":"added","signatureHash":"c6d3e3a77578b36b6c4aad8dafc3871d931b9e66fa22f2c3c6394cadf03e875d"},{"name":"FirmwareUpdate","file":"apps/desktop/src/lib/firmwareUpdate.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"e006f8b57b25db31f61519afde14646eedf8bee144c8f4bf1ad28706c112321b"},{"name":"CANCEL_PENDING","file":"apps/desktop/src/lib/firmwareUpdate.svelte.ts","kind":"const","visibility":"public","change":"added","signatureHash":"2d3bf7f2df42b39e59134f21d9d739a84ad0e9e1c040f77f535e4952ebbe3291"},{"name":"CANCEL_TOO_LATE","file":"apps/desktop/src/lib/firmwareUpdate.svelte.ts","kind":"const","visibility":"public","change":"added","signatureHash":"a4ed9fbea188b48eb450ef87a35384dbeed17fe5d01e68d302c381544bfcf473"},{"name":"FOLLOWED_UPDATE_ENDED","file":"apps/desktop/src/lib/firmwareUpdate.svelte.ts","kind":"const","visibility":"public","change":"added","signatureHash":"1dc2ac6ec2211a6cc0558053158f4261adacb37de78c9e3584d9f203860ac120"},{"name":"FirmwareUpdateApi","file":"apps/desktop/src/lib/firmwareUpdate.svelte.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"469d01ab455adc37e192300ddb77ba665f72f4dd1db6597602107b4697704186"},{"name":"Meters","file":"apps/desktop/src/lib/meters.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"29de5665661a53ec48fd608c880ad9e5541d93ab62996083b910fd1e9cfcf8d0"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"36d2f1b2a10181c4a8a8be2be0ec6afecdc2e1510b978de44a7a1bd9fa4ba061"},{"name":"MOCK_MODES","file":"apps/desktop/src/lib/mockBackend.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"48ada54e993623b98b0851d90672190ff87476de0983f3d97a143356d255c333"},{"name":"Recording","file":"apps/desktop/src/lib/recording.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"952cf6d4edd4b23b2ff1bb2679492fed997112d8981a4ba929ac31c722150a38"}],"routes":[],"configKeys":[],"stateEvents":[{"kind":"case-branch","name":"update_required","file":"apps/desktop/src/lib/deviceDetails.ts","change":"removed"}]}
target_section: reference
target_file: docs/reference/desktop-backend.md
generated_at: 2026-10-31T00:48:36Z
---

<!-- warning: output includes min_supported_firmware, known_latest_firmware: names the impact set does not extract -->

The firmware update changes as follows:

- `firmwareCancel` stops an update until the erase begins. Once the erase has
  begun it is refused with an error and the update carries on.
- `firmwareFlashStatus` reports a running update as a `FlashStatus`, or null, so
  a reloaded page can follow it to the end instead of offering a second update.
- A recording on the meter being updated is no longer ended without asking. The
  page asks first; another meter's recording keeps going.
- `ConnectedInfo` gains `min_supported_firmware` and `known_latest_firmware`.
- Firmware versions accept a leading `v`. A `-` suffix marks a pre-release, which
  sorts below its release; a `+` suffix is ignored.
- `powerz info` and the desktop app read the bootloader, application, HEAD, OTP
  and UID blocks once each and keep what they read when one block fails. A
  version that could not be read prints as `unknown`.
- The firmware probe takes any SYNC reply. A HEAD flag the app does not know
  shows as "not recognised" under Update check, not as a warning.
