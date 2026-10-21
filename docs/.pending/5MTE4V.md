---
fr: 5MTE4V
impact_set: {"symbols":[{"name":"default","file":"apps/desktop/vite.config.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"121f807adc571c2ceef41e60a76abb2fa91ff78fca7e605ec831fa9ee9d8523e"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: explanation
target_file: docs/explanation/architecture.md
generated_at: 2026-10-21T10:29:51.000Z
---

## App icon

The app icon is a USB-C port drawn as the meter's display, with a voltage step and a current trace on it.
Its source is one vector drawing in the desktop app's icons folder, plus a simpler drawing used at 16 and 32 pixels.
Running the icons script in the desktop app regenerates every platform's icon files from those two drawings, so edit the drawings and re-run it rather than editing the generated images.
