<!-- WARNING: This reference was generated without mechanical signature extraction for this stack. Signatures may be imprecise. Review carefully before publishing. -->

# `powerz-protocol` API

The authoritative, always-current API reference is rustdoc:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc -p powerz-protocol --no-deps --locked --open
```

This page is a map of the public modules.

| module | contents |
|---|---|
| crate root | `VID` (`0x5FC9`, the ChargerLAB USB vendor ID); `VERSION` (the library's version); `pd`, a re-export of the [`powerz-pd`](powerz-pd.md) decoder crate (`pub use powerz_pd as pd`) |
| `frame` | Pure framing over byte slices: `MainHeader`, `ControlHeader`, `DataHeader`, `SectionHeader`, `Attribute`, `Section`, `Reply`; `classify_reply`, `parse_put_data`; request builders `get_data`, `connect`, `start_queue`, `stop_queue`, `start_pd_sniffer`, `stop_analyzer`; `Attribute::PD_PACKET`; `SequenceCounter`; `FrameError` |
| `decode` | Payload decoders: `AdcSnapshot`, `QueueRecord` (also built from raw units with `QueueRecord::from_raw`), `Rate`, `Settings`, `DeviceInfoBlock`, `OtpBlock`, `Uid`; `DecodeError`; the `pd_packet` module (see below) |
| `transport` | The `Transport` trait and its pipes: `bulk` (`nusb`), `cdc` (`serialport`), `hid` (`hidapi`, ADC snapshot only); `discovery` and `select` for finding meters; `GuardedTransport` (refuses commands outside the safe set before they are sent); `MockTransport` for tests; `FIRST_CONNECT_TIMEOUT` (wait for the first ACCEPT before `Transport::recover` and a retry); `Error` |
| `session` | `Session` over a transport: CONNECT, memory read (`DeviceInfo`), authentication, and ADC-queue streaming (`Streamer`, `StreamPoll`, `Gap`, and `SnapshotCadence` for the side snapshot that refreshes temperature at 1000 SPS); PD capture (`PdSniffer`, `PdOptions`, `PdPoll`, `CapturedRecord`, and the `Session` methods `start_pd_sniffer`, `stop_pd_sniffer`, `poll_pd_sniffer`; see below); `Clock` / `SystemClock`; `OwnedSection`; `Error` |
| `pdcap` | The [pdcap](../pdcap.md) capture file format: `Header`, `Entry`, `Writer`, `Reader`, `load` / `Loaded`, `Error` / `ErrorKind`, `FORMAT_VERSION`, `MAX_LINE_BYTES` (see below) |
| `crypto` | AES-128-ECB with 0xFF padding (`pad_ff`, `encrypt`, `decrypt`), `crc32`, and the shipped `KEYS` |

Decoders and framing are pure functions (`&[u8] → Result<T, _>`), so they can
be used without a device. The wire format is described in
[docs/protocol.md](../../protocol.md).

## PD capture

The PD capture path has three layers: `decode::pd_packet` parses the meter's
PD_PACKET chunks into records, `session::PdSniffer` starts, polls and stops
the meter's sniffer, and `pdcap` saves and loads captures. The `pd` re-export
decodes the messages inside the records.

### `decode::pd_packet`

Pure functions over the PD_PACKET section payload: a 12-byte status block,
then captured records.

| item | signature or contents |
|---|---|
| `STATUS_LEN` | `pub const STATUS_LEN: usize = 12` |
| `PdStatus` | `time_ms: u32`, `vbus_mv: u16`, `ibus_ma: i16` (signed), `cc1_mv: u16`, `cc2_mv: u16`; `PdStatus::decode(p: &[u8]) -> Result<PdStatus, DecodeError>` |
| `PdChunk` | `status: PdStatus`, `records: Vec<PdRecord>`, `skipped: usize` (records that ran past the chunk); `PdChunk::parse(p: &[u8]) -> Result<PdChunk, DecodeError>` |
| `PdRecord` | `time_ms: u32` (meter time), `from_meter: bool`, `raw: Vec<u8>` (the record's bytes, `b0` included), `kind: PdRecordKind`; `pd_input(&self) -> Option<crate::pd::Input<'_>>` turns a message, attach, detach, hard reset or cable reset into decoder input |
| `PdRecordKind` | `Message(PdMessage)`, `BigMessage(PdMessage)`, `Event(PdEvent)` |
| `PdMessage` | `sop: u8`, `header: u16`, `body: Vec<u8>` (the bytes after the header) |
| `PdEvent` | `Attach(CcLine)`, `Detach(CcLine)`, `BadMessageId`, `HardReset`, `CableReset`, `ReceiveError(ReceiveErrorCause)`, `MonitorType(MonitorType)`, `BigMessage`, `Unknown(u8)` |
| `CcLine`, `ReceiveErrorCause`, `MonitorType` | Event parameters; each keeps unknown values as `Other(u8)` |
| `CaptureClock` | `CaptureClock::new() -> CaptureClock`; `elapsed_ms(&mut self, time_ms: u32) -> u64`: ms since the capture start. A u32 wraparound counts as forward; a step back resyncs without going back. |

### `session` PD sniffer

| item | signature or contents |
|---|---|
| `PdSniffer<'a, T: Transport, C: Clock>` | `PdSniffer::start(session: &'a mut Session<T, C>, options: PdOptions) -> Result<Self, Error>` (authenticates to level 1 first when needed); `poll(&mut self) -> Result<PdPoll, Error>`; `run(&mut self, on_poll: impl FnMut(&PdPoll) -> bool) -> Result<(), Error>` (polls every `POLL_PERIOD` until `on_poll` returns `false` or a poll fails); `stop(self) -> Result<(), Error>`. The stop request is also sent on drop. |
| `PdSniffer::POLL_PERIOD` | `Duration::from_millis(5)` |
| `PdOptions` | `with_adc: bool`: also request the ADC snapshot with every poll |
| `PdPoll` | `status: Option<PdStatus>`, `records: Vec<CapturedRecord>`, `skipped: usize`, `adc: Option<AdcSnapshot>` |
| `CapturedRecord` | `elapsed_ms: u64` (never goes backwards), `record: PdRecord` |
| `Session` methods | `start_pd_sniffer(&mut self) -> Result<(), Error>`, `stop_pd_sniffer(&mut self) -> Result<(), Error>`, `poll_pd_sniffer(&mut self, options: PdOptions, clock: &mut CaptureClock) -> Result<PdPoll, Error>`: the same requests without a borrow, for a caller that keeps the session between polls (the desktop worker). Stopping is then the caller's job. |

A REJECT from the meter on start or stop is `Error::Rejected`.

### `pdcap`

Reads and writes the [pdcap format](../pdcap.md). It works on any `Write` or
`BufRead`; it opens no files.

| item | signature or contents |
|---|---|
| `FORMAT_VERSION` | `pub const FORMAT_VERSION: u32 = 1`: the version written, and the newest read |
| `MAX_LINE_BYTES` | `pub const MAX_LINE_BYTES: usize = 64 * 1024`: a longer line is damage |
| `Header` | `format_version: u32`, `app_version: String`, `model: String`, `firmware_version: String`; `Header::new(app_version: &str, model: String, firmware_version: String) -> Header`. There is no serial number field. |
| `Entry` | `Status { session_ms: u64, status: PdStatus }` or `Record { session_ms: u64, elapsed_ms: u64, record: PdRecord }` |
| `Writer<W: Write>` | `Writer::new(out: W, header: &Header) -> io::Result<Writer<W>>`; `write(&mut self, entry: &Entry) -> io::Result<()>`; `flush(&mut self) -> io::Result<()>`; `finish(self) -> io::Result<W>` |
| `Reader<R: BufRead>` | `Reader::new(input: R) -> Result<Reader<R>, Error>` (reads the header); `header(&self) -> &Header`; an `Iterator<Item = Result<Entry, Error>>` that ends after the first error |
| `load` | `load(input: impl BufRead) -> Loaded`: every valid line up to the first damaged one |
| `Loaded` | `header: Option<Header>`, `entries: Vec<Entry>`, `error: Option<Error>` |
| `Error` | `line(&self) -> usize` (1-based; the header is line 1); `kind(&self) -> &ErrorKind` |
| `ErrorKind` | `Io`, `MissingHeader`, `Json`, `NotUtf8`, `LineTooLong`, `NotPdcap(String)`, `NewerVersion(u32)`, `BadHex`, `BadStatus(usize)`, `BadRecord` |
