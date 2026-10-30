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
powerz-pd         library: USB PD r3.2 message decoder (no dependencies)
powerz-protocol   library: framing, decoders, transports, session, crypto,
                  PD capture, pdcap files, PDM text session,
                  firmware update                             uses pd
powerz-sim        demo device: a simulated KM003C behind the Transport trait
powerz-record     vendor-compatible CSV writer

powerz-cli        the `powerz` binary         uses protocol, sim, record
open-powerz-desktop  Tauri 2 app backend      uses protocol, sim, record
```

- **powerz-pd** turns PD message bytes into typed messages, a one-line
  summary and a field tree. It has no dependencies and does no I/O, so it
  builds for `wasm32-unknown-unknown` and a later web edition can share it.
  `powerz-protocol` re-exports it as `powerz_protocol::pd`.
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
- **Firmware writes take a separate, stricter path.** The firmware update is
  the only code that sends write-capable frames (the jumps, the descriptor
  and the payload). It needs a `FlashAck` marker, which the CLI creates only
  after `--i-understand` and the desktop app only after the typed version,
  and every frame goes through `FlashGuard`, which names nothing below the
  bootloader's 0x4000 boundary and bounds the payload by the declared size.
  Nothing is retried automatically: after a failure the meter stays in its
  bootloader, which can always be flashed again. The engine was checked step
  by step on a real KM003C, each step only after the user approved it.
- **Two meter writes, each on its own narrow path.** Deleting records and
  changing settings are the only other writes. The shared guard still refuses
  every SET_DATA (0x48) and settings write (0x0A); each passes only through
  its own transport method (`transact_record_table`, `transact_settings`),
  whose check accepts exactly one frame shape: the record table on attribute
  0x28, or the settings keys in order with every value in range and the save
  marker last, never calibration or the factory reset. Both write once, read
  the meter back and stop on a mismatch without retrying; a delete backs the
  records up as CSV first and deletes nothing when that fails. The settings
  write goes over the CDC port, since the KM003C ignores it over bulk.
- **Auth keys ship in the repository.** Authenticated features (queue
  streaming, device info) need the meter's AES keys; shipping them makes those
  features work out of the box.
- **Sections are parsed by their own size.** The main header's object count is
  unreliable on plain replies, so the parser walks the section chain and
  checks every length.

## PD capture

The meter can sniff the USB PD traffic on the cable that passes through it.
The capture is passive: the meter keeps passing power and nothing is sent on
the cable.

The capture path has four steps:

1. **Sniffer session.** `PdSniffer` (or the matching `Session` methods) starts
   the meter's sniffer, polls it every 5 ms and stops it on every exit path,
   drop included.
2. **Record parser.** Each poll answers a PD_PACKET chunk: a status block
   (meter time, VBUS, IBUS, CC1, CC2) and the captured records. The parser in
   `decode::pd_packet` splits it into messages, big messages and events, and
   gives each record its time since the capture start.
3. **Decoder.** `powerz-pd` decodes each message. One `Capture` context per
   capture remembers the last offer, so a Request decodes against the offer it
   points at, and it reassembles chunked extended messages.
4. **Rows to the UI.** In the desktop app, the backend decodes the records and
   sends table rows to the UI in batches, at most 60 a second, on the same
   channel as the measurements. The field tree of one record is fetched only
   when the user selects it. The CLI's `pd` command prints the same records
   directly.

PD records and measurement samples share one session time base, so PD markers
sit on the power chart at the moment they happened. In the desktop app the
same worker thread polls the ADC queue and the sniffer, one after the other.

**pdcap files.** A capture saves as a `.pdcap` file: JSON Lines, a header line
and then one line per status sample or record, with the record's raw bytes in
hex. Readers decode everything else from those bytes. The format has no serial number field, so files are
safe to share. A damaged file loads up to the damaged line. See the
[pdcap reference](../reference/pdcap.md).

**Vendor logs.** The desktop app also imports the vendor app's PD log database
(SQLite) read-only. The file is treated as untrusted: it is never written, and
every value read from it is bounded.

## Protocol triggering (the PDM session)

Triggering does not use the binary protocol. The meter takes short text
commands on its virtual serial (CDC) port (`pdm open`, `entry pd`,
`pd req=2`, ...), and the `pdm` module of `powerz-protocol` speaks them:

- `reply` is a pure stream reader. It splits the bytes into text lines,
  binary capability blocks (`pdo:`, `ufcs:`, `scp:`, ...) and stray binary
  frames, which can arrive between lines at any time. `caps` parses the
  blocks.
- `session` (`PdmSession`) opens the session, keeps 60 ms between commands,
  and always ends with `pdm close`, on error and on drop too. It notices when
  a reply belongs to another command. The meter is then out of step, and the
  session refuses everything until it is reopened after a power cycle.
- `risk` is a pure classifier: read-only, 5 V or less, or above 5 V. It fails
  closed, so anything it cannot prove safe counts as above 5 V. The session
  sends an above-5 V command only with a `HighVoltageConfirmation` made for
  that one command. The CLI makes one from `--allow-high-voltage`, the
  desktop app from the user's click on **Confirm**.
- `pd`, `legacy`, `detect`, `message` and `console` build the features on
  top: PD offers and requests, the other protocols, detection, custom PD
  messages and the raw console. Each request path ends with a return to
  5 V.

**Next to the bulk stream.** In the desktop app the measurement stream keeps
running on the bulk pipe while a trigger session is open, so the panel can
show live VBUS. The bulk session must be connected first: a bulk CONNECT
after the PDM session has started silences it. For the same reason, the
stream does not reconnect while the automatic protocol list runs. It only
pauses its polling and resumes when the list ends. The trigger session runs
on its own thread, and every request goes through the backend, which is the
final gate for the confirmation.

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
- **Several meters, one window.** Each connected meter has its own worker,
  session and history in the backend, and its own store in the UI. Commands
  name the meter they act on by device id, so an action never reaches the
  wrong meter; without one they go to the most recently connected meter, which
  keeps the single-meter app unchanged. A reload attaches every running meter.
- **Replug waits for the same meter.** When a streaming meter is unplugged,
  the backend remembers its serial number and the app shows **Waiting for
  meter**. The hotplug watcher matches new listings by serial. When the meter
  comes back, the backend reconnects through the normal connect path at the
  same rate, and the session goes on with the time away marked as a gap. A
  manual pick of another device ends the wait. A recording ends at the unplug
  with a complete file.
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

## Timing budgets in tests

Timing tests keep a strict budget on a developer machine and widen it by a
fixed factor on CI, where shared runners are slower. The 10-million-sample
history test takes both of its limits from `historyPanBudget`: a pan step must
finish within `HISTORY_PAN_BUDGET_MS` (100 ms), and the whole test within
`HISTORY_PAN_TIMEOUT_MS` (5 s). With the `CI` environment variable set, both
are three times larger. A failure message names the budget it used.

## Builds and releases

Every build uses one Rust version, pinned in `rust-toolchain.toml`. rustup
installs it for local builds, and the CI and release workflows install it from
the same file; a test in the gate fails when a workflow names its own Rust
version. A new Rust release on the runners therefore can't turn a green branch
red with new lints: moving the version is a reviewed change to that one file.

The README lists every shipped feature, one line each. A test in the gate reads
every "Added" entry in `CHANGELOG.md` and fails when the README's Features
section has no line for it; `apps/desktop/scripts/readme/readme-features.json`
maps each entry to its area and a phrase from its line, or marks it as not a
user feature. The same test fails when a relative link in the README points at
a file that does not exist. A release that adds a feature therefore also adds
its README line.

Releases come from `.github/workflows/release.yml`. A pushed `vX.Y.Z` tag, or a
manual run that names an existing tag, runs five jobs:

- **check** compares the tag with the workspace, desktop and Tauri versions and
  reads that version's CHANGELOG section, which becomes the release notes;
- **desktop** builds the unsigned installers with tauri-action on macOS
  (universal .dmg), Windows (.msi and setup .exe; the setup .exe alone for a
  pre-release) and Linux (AppImage and .deb);
- **cli** builds `powerz` in release mode and packs it with the LICENSE and
  README;
- **assemble** checks that every expected file is there and writes
  `SHA256SUMS.txt`;
- **publish** runs `apps/desktop/scripts/release/publish.ts`: it creates the
  release as a draft (or finds the one an earlier run left), keeps that
  release's id for every later call, attaches every file and only then
  publishes it.

Only the publish job can write to the repository. A pull request that changes
the release setup runs the same jobs without publish, as a dry run whose files
stay workflow artifacts.
