---
fr: fr_01M65KVDYC49QF1V7N1Y4PJRVN
impact_set: {"symbols":[{"name":"newerFinalExists","file":"apps/desktop/scripts/release/publish.ts","kind":"function","visibility":"public","change":"added","signatureHash":"23d9c1c68b35fe45cb19e3f3a28f554d18e5bde5b67617b4d5640d0103200f2f"},{"name":"publishRelease","file":"apps/desktop/scripts/release/publish.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"4446b2d6fa26c0d8d1bd042472c2565fda8ec8acf1452c41f55c82c682e82b2f"},{"name":"ReleasePatch","file":"apps/desktop/scripts/release/publish.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"15dae904d6ebcf4f55b569667eb449b798e218aabc4e6c8007c0073977fe9df2"},{"name":"appImageNote","file":"apps/desktop/scripts/release/versions.ts","kind":"function","visibility":"public","change":"added","signatureHash":"6256140c2b7370a41fcd496e678d8114b1d8ffc7b7d694fff44e19a0a6775da8"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/install-a-release.md
generated_at: 2026-10-30T08:59:22Z
---

Release notes on GitHub now end with a short Linux AppImage note, written by
`appImageNote`. The downloaded AppImage loses its executable bit, so the note
gives the two commands to run it, with the release's own version filled in:

```bash
chmod +x open-powerz_<version>_amd64.AppImage
./open-powerz_<version>_amd64.AppImage
```

The **Latest** badge on the releases page always points to the newest final
release. Re-publishing an older tag no longer takes the badge from a newer one
(`newerFinalExists`, checked inside `publishRelease`), so
<https://github.com/nesquikm/open-powerz/releases/latest> stays on the newest
version.
