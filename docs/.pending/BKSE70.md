---
fr: BKSE70
impact_set: {"symbols":[{"name":"LEGACY_BOOTLOADER_HELP","file":"apps/desktop/src/lib/deviceDetails.ts","kind":"const","visibility":"public","change":"added","signatureHash":"db4e52d1dd71e00e44dd71eb92b81142d75087c1859bf63e302441f08b777456"},{"name":"FirmwareUpdate","file":"apps/desktop/src/lib/firmwareUpdate.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"89b58a6e9f523efef3fd5ae0c61f498d9c42bfc5c115b08558b2ef246862972e"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"3dc55df722c84bf628cd8529ba45c615ea4dd3cd7983c82b8744e2bb2532788c"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/update-firmware.md
generated_at: 2026-11-20T10:49:46Z
---

## Update stops at once on an old bootloader

When a meter on the old bootloader (2.1) reaches its bootloader during an update, the update now
stops at the first look instead of waiting two minutes. Nothing is sent to that bootloader. The
message says this bootloader is not supported yet, that unplugging and replugging the meter (no
buttons) returns it to the application firmware, and links the new "Old bootloader" section of
this how-to.

In the desktop app the firmware page will not start an update on a meter listed in the old
bootloader and shows that message instead (`LEGACY_BOOTLOADER_HELP`).
The mock backend's old-bootloader mode refuses the update the same way
(`MockBackend`).
