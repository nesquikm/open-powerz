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
powerz-protocol      library: framing, decoders, transports, session, crypto
  ↑     ↑      ↑
  │     │      open-powerz-desktop  (Tauri 2 app; its features arrive in the next milestone)
  │     powerz-sim                  (demo device: a simulated KM003C behind the Transport trait)
  │     ↑
  powerz-cli                        (uses the library, and the simulator for --transport sim)
```

- **powerz-protocol** holds everything that talks to the meter. Framing and
  decoding are pure functions over byte slices; transports sit behind one
  `Transport` trait; `Session` adds the request/reply rules on top.
- **powerz-cli** is the `powerz` binary. Each command is a function over a
  `Session` and an output sink, so tests drive it with a mock transport.
- **powerz-sim** plays scripted scenarios through the same trait, so the CLI
  and its end-to-end tests run without hardware.

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
