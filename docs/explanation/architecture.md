# open-powerz architecture

## Context

ChargerLAB's own PC software for the POWER-Z KM003C / KM002C runs only on
Windows. open-powerz is an open-source replacement for macOS, Linux and
Windows. It is written clean-room: from the vendor's public API documents and
our own device experiments, never from vendor code. What we learned is written
down in [docs/protocol.md](../protocol.md), where each fact is tagged
`[device]`, `[static]` or `[inferred]` so readers know how sure it is.

## Modules

```
powerz-protocol   library: framing, decoders, transports, session, crypto
powerz-sim        demo device: a simulated KM003C behind the Transport trait
powerz-record     vendor-compatible CSV writer

powerz-cli        the `powerz` binary         uses protocol, sim, record
open-powerz-desktop  Tauri 2 app backend      uses protocol, sim, record
```

- **powerz-protocol** holds everything that talks to the meter. Framing and
  decoding are pure functions over byte slices; transports sit behind one
  `Transport` trait; `Session` adds the request/reply rules on top.
- **powerz-cli** is the `powerz` binary. Each command is a function over a
  `Session` and an output sink, so tests drive it with a mock transport.
- **powerz-sim** plays scripted scenarios through the same trait, so the CLI,
  the desktop app and their end-to-end tests run without hardware.
- **powerz-record** writes the vendor-compatible CSV, so `powerz stream`, the
  desktop app's recordings and its exports produce the same file.
- **open-powerz-desktop** is the Tauri backend: a worker thread owns the meter
  session and keeps the session history; the Svelte web UI draws it with
  uPlot.

## Design decisions

- **Pure core, pluggable I/O.** Decoders never touch USB, so they are tested
  with byte vectors and property tests, and any new pipe only implements the
  trait.
- **Interfaces are chosen by USB class, not number.** Interface numbers differ
  between models; class codes (vendor bulk, HID, CDC) do not.
- **Vendor bulk first, HID only for snapshots.** On firmware 2.0.6 any HID
  reply longer than 64 bytes hangs and reboots the meter, so the HID transport
  sends only the ADC snapshot request and refuses everything else. CDC skips
  memory read, so it carries `read` and `watch` only.
- **A safety guard in front of every transport.** `GuardedTransport` refuses
  anything outside the safe command set (no firmware, flash or calibration
  writes) before a byte is sent, and `--trace` shows refused frames with `x`.
- **Auth keys ship in the repository.** Authenticated features (queue
  streaming, device info) need the meter's AES keys; shipping them makes those
  features work out of the box.
- **Sections are parsed by their own size.** The main header's object count is
  unreliable on plain replies, so the parser walks the section chain and
  checks every length.

## The desktop app

- **The backend owns the session.** One worker thread per connected meter
  polls the stream, keeps the whole session history with min/max summaries for
  every zoom level, and computes statistics. The web UI receives measurements in
  batches of about 20–60 per second over a Tauri channel, never one message per
  sample.
- **The session outlives the window's page.** When the page reloads (for
  example with a development reload), the new page asks the backend for the
  running session and picks it up instead of starting disconnected: the device,
  the sample rate, any open recording with its elapsed time, and the chart and
  statistics, which refill from the session history. Closing the window still
  ends the session.
- **A poll thread that keeps up at 1000 SPS.** On macOS the worker thread runs
  at the user-interactive quality of service, so a sheet or another app in
  front does not starve it.
- **Steady memory in WebKit.** On macOS the window is a WebKit web view, and
  WebKit does not count the native memory of canvas path objects when it
  decides to collect garbage. The chart therefore strokes every series straight
  onto the canvas, splitting dashed lines into short solid pieces, and refills
  its live and overview frames in place instead of allocating new ones. Memory
  still climbs for a few minutes while the web view warms up, then levels off.
- **One icon source.** The app icon is a USB-C port drawn as the meter's
  display, with a voltage step and a current trace on it. Its source is one
  vector drawing in the desktop app's icons folder, plus a simpler drawing used
  at 16 and 32 pixels; `bun run icons` in `apps/desktop` regenerates every
  platform's icon files from them, so edit the drawings rather than the
  generated images.
