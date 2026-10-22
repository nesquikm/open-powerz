---
fr: TGE83E
impact_set: {"symbols":[{"name":"pdDetails","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"ef5439b59247ccbc42ad4f18d81c58fb64a93cd772cd23406c168320fd44be5c"},{"name":"pdImport","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"6e11dc397b5f5f39444ad2d15249f8f1adb0e88d1817fc623f1a8e6e8b9dfe90"},{"name":"pdOpen","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"99b18cf8d127c0d38be7acc71ae8f4b87f014f8c3e029641d24f2c19bcce36b6"},{"name":"pdSave","file":"apps/desktop/src/lib/backend.ts","kind":"function","visibility":"public","change":"added","signatureHash":"37bfc6180ef8b9052161d16fbc3ddaefddbc3982a2e22512938173f4133df13a"},{"name":"PdOpened","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"29c593bd7e7a2f9c17a25a4e494b396bc07928cdd2123f73138ff2a4912c3b2a"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"84b40eca27c2178831ce282e798aace8fae7cb79fdb6867c16fe7246ec4cf043"},{"name":"PdCapture","file":"apps/desktop/src/lib/pdCapture.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"8c4c59617e63ff0dccb1573c799f69a3b8232464aca934a62b7c631375468c8f"},{"name":"PD_STOP_FIRST_REASON","file":"apps/desktop/src/lib/pdCapture.svelte.ts","kind":"const","visibility":"public","change":"added","signatureHash":"4f567f997a13f2a72b2c1bf955b8fe1e1b83cd9c34ddb0f14011a3f2936b94c2"},{"name":"PdControls","file":"apps/desktop/src/lib/pdCapture.svelte.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"5369a649e84b043a579a8be9fbefe3152e8ee91d0008c107f84cb31c7934048b"},{"name":"rowLabel","file":"apps/desktop/src/lib/pdTable.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"d20df9cf77c3862b83638be9a5e0650ac64afab8afda3bd690c8578d260a0763"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/analyze-pd-traffic.md
generated_at: 2026-10-22T20:01:55Z
---

## Save, open and import captures

- **Save capture** writes the current capture to a `.pdcap` file (an open, documented
  format; see the reference page for the format). The meter's serial number is never
  written. A notice names the file, with buttons to show it in the file manager or copy
  its path.
- **Open capture** loads a `.pdcap` file back into the table, the details and the chart,
  exactly as saved. No meter is needed. If the file is damaged, every line before the
  damage loads and a notice names the line where reading stopped.
- **Import vendor log** opens a PD log database saved by the vendor's Windows app
  (`.sqlite`) read-only and shows its messages. Rows that cannot be read are skipped and
  counted in the notice.
- Stop a running capture before you open or import a file. Clear or Start capture
  returns to the live view.

On the command line, `powerz pd --save capture.pdcap` saves while it prints, and
`powerz pd decode capture.pdcap` prints a saved file without a meter.
