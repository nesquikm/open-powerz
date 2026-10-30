---
fr: fr_01M65KVDYC49QF1V7N1Y4PJRVH
impact_set: {"symbols":[{"name":"Detection","file":"apps/desktop/src/lib/backend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"6d3afcd2ba478cb11f8e62453eaf00ae5b00a9370f142c72bff1785f8c7a1a1e"},{"name":"PdConsole","file":"apps/desktop/src/lib/console.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"f16deb1cc10e6eadaba19a01a790b2908a384df536c10a3f9dd0336eadaed650"},{"name":"lineRefusal","file":"apps/desktop/src/lib/console.svelte.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"95280934ce12240e7c46df07aefb65505cd6e656a399d38d09083bdffef127b5"},{"name":"MockBackend","file":"apps/desktop/src/lib/mockBackend.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"44604c5525a3f4637aa72384096600d6a069393db6d2a739adf324b4f0c88e42"},{"name":"TriggerPanel","file":"apps/desktop/src/lib/trigger.svelte.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"f32989c575481c6553c2921831f05838c456458eecef48357f36aa8df8d2fbc5"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: reference
target_file: docs/reference/desktop-backend.md
generated_at: 2026-10-30T20:00:29Z
---

<!-- warning: output includes pdm::console::listen_for, LISTEN, pdm::detect::hint_from_volts: Rust symbols the impact set does not extract -->

The trigger panel, the console and the CLI's stream change as follows:

- `Detection` gains `hint`: the D+/D- charging-mode text from the newest
  measurement, the same text `powerz detect` prints.
- `pdm::console::listen_for` gives an `entry` line the entry bound instead of
  `LISTEN`, so its `pdo:` block is caught. The desktop console and
  `powerz console` both use it.
- `pdm::detect::hint_from_volts` takes the levels in volts, as records hold
  them.
- A stream error that is not a device loss no longer ends the session while a
  PDM session opens, runs or closes, and no reconnect starts.
- `powerz stream` exits 0 with a note when the meter rejects the stop after
  gaps in the stream.
