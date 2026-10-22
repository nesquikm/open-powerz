<!-- WARNING: This reference was generated without mechanical signature extraction for this stack. Signatures may be imprecise. Review carefully before publishing. -->

# `powerz-protocol` API

The authoritative, always-current API reference is rustdoc:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc -p powerz-protocol --no-deps --locked --open
```

This page is a map of the public modules.

| module | contents |
|---|---|
| crate root | `VID` (`0x5FC9`, the ChargerLAB USB vendor ID); `VERSION` (the library's version) |
| `frame` | Pure framing over byte slices: `MainHeader`, `ControlHeader`, `DataHeader`, `SectionHeader`, `Attribute`, `Section`, `Reply`; `classify_reply`, `parse_put_data`; request builders `get_data`, `connect`, `start_queue`, `stop_queue`; `SequenceCounter`; `FrameError` |
| `decode` | Payload decoders: `AdcSnapshot`, `QueueRecord` (also built from raw units with `QueueRecord::from_raw`), `Rate`, `Settings`, `DeviceInfoBlock`, `OtpBlock`, `Uid`; `DecodeError` |
| `transport` | The `Transport` trait and its pipes: `bulk` (`nusb`), `cdc` (`serialport`), `hid` (`hidapi`, ADC snapshot only); `discovery` and `select` for finding meters; `GuardedTransport` (refuses commands outside the safe set before they are sent); `MockTransport` for tests; `FIRST_CONNECT_TIMEOUT` (wait for the first ACCEPT before `Transport::recover` and a retry); `Error` |
| `session` | `Session` over a transport: CONNECT, memory read (`DeviceInfo`), authentication, and ADC-queue streaming (`Streamer`, `StreamPoll`, `Gap`, and `SnapshotCadence` for the side snapshot that refreshes temperature at 1000 SPS); `Clock` / `SystemClock`; `OwnedSection`; `Error` |
| `crypto` | AES-128-ECB with 0xFF padding (`pad_ff`, `encrypt`, `decrypt`), `crc32`, and the shipped `KEYS` |

Decoders and framing are pure functions (`&[u8] → Result<T, _>`), so they can
be used without a device. The wire format is described in
[docs/protocol.md](../../protocol.md).
