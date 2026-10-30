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
| `decode` | Payload decoders: `AdcSnapshot`, `QueueRecord` (also built from raw units with `QueueRecord::from_raw`), `Rate`, `Settings`, `DeviceInfoBlock`, `OtpBlock`, `Uid`; `DecodeError`; the `pd_packet` module (see below); the `head` module (`HeadReply`, the old-firmware check); the `records` and `settings` modules (offline records and the settings block, see below) |
| `adcnew` | The ADCNEW stream the meter pushes on its CDC serial port: `Request` (start / stop / keep-alive bytes), `Content`, `Frame` and `decode_frame`, `crc8`, `CdcDemux` (splits ADCNEW frames from PDM text on one port, `CdcItem`), `AdcnewCounters`, `AdcnewError`; `KEEP_ALIVE_PERIOD`, `START_ECHO_QUIET` (12 s), `MAX_RAW_FRAMES` (see below) |
| `transport` | The `Transport` trait and its pipes: `bulk` (`nusb`), `cdc` (`serialport`; `cdc::without_tty_twins` drops the macOS `/dev/tty.*` twin of each `/dev/cu.*` port), `hid` (`hidapi`, ADC snapshot only); `discovery` and `select` for finding meters; `GuardedTransport` (refuses commands outside the safe set before they are sent; the record-table write and the settings write pass it only through `Transport::transact_record_table` and `Transport::transact_settings`, checked by `guard::check_record_table_write` and `guard::check_settings_write`); `Transport::settings_pipe` (the CDC twin a bulk pipe opens for the settings write); `MockTransport` for tests; `FIRST_CONNECT_TIMEOUT` (wait for the first ACCEPT before `Transport::recover` and a retry); `Error` |
| `session` | `Session` over a transport: CONNECT, memory read (`DeviceInfo`), authentication, and ADC-queue streaming (`Streamer`, `StreamPoll`, `Gap`, and `SnapshotCadence` for the side snapshot that refreshes temperature at 1000 SPS); PD capture (`PdSniffer`, `PdOptions`, `PdPoll`, `CapturedRecord`, and the `Session` methods `start_pd_sniffer`, `stop_pd_sniffer`, `poll_pd_sniffer`; see below); `Clock` / `SystemClock`; `OwnedSection`; `Error` |
| `pdcap` | The [pdcap](../pdcap.md) capture file format: `Header`, `Entry`, `Writer`, `Reader`, `load` / `Loaded`, `Error` / `ErrorKind`, `FORMAT_VERSION`, `MAX_LINE_BYTES` (see below) |
| `pdm` | The PDM text session on the virtual serial port, for protocol triggering: `PdmSession`, `HighVoltageConfirmation`, `Command`, `Protocol`, `Risk` and `classify`, `ReplyReader`, `Capabilities`, `find_port` / `open_port`, `Error`; submodules `pd`, `legacy`, `detect`, `message`, `console` (see below) |
| `firmware` | Mode detection, firmware versions, firmware container inspection and the flash engine: `DeviceMode`; submodules `version`, `container` and `flash` (see below) |
| `crypto` | AES-128-ECB with 0xFF padding (`pad_ff`, `encrypt`, `decrypt`), `crc32`, and the shipped `KEYS` |

Decoders and framing are pure functions (`&[u8] → Result<T, _>`), so they can
be used without a device. The wire format is described in
[docs/protocol.md](../../protocol.md).

## Offline records, settings and the ADCNEW stream

### `decode::records`

| item | signature or contents |
|---|---|
| `RecordEntry` | One 48-byte record-list entry: `name: String`, `interval_s: u8`, `points: u16`, `capacity_points: u32`, `duration_s: u32`, `charge_uah: i32` and `energy_uwh: i32` (signed, as stored), `offset: u32`, `raw: [u8; 48]`; `RecordEntry::decode(buf: &[u8]) -> Result<RecordEntry, DecodeError>`; `charge_ah`, `energy_wh` (signed) and `charge_magnitude_ah`, `energy_magnitude_wh` (as the screen shows them); `read_window(&self) -> Option<(u32, u32)>` (the record's memory window, bounded by the record area); `read_chunks(&self) -> Vec<(u32, u32)>` (that window as reads of at most `MEMORY_READ_MAX_LEN`); `sample_time_s(&self, index: usize) -> u64` |
| `decode_record_list` | `decode_record_list(payload: &[u8]) -> Result<Vec<RecordEntry>, DecodeError>`; an empty list is `Ok(vec![])` |
| `encode_record_table` | `encode_record_table(entries: &[RecordEntry]) -> Vec<u8>`: the table a delete writes back, the kept entries' raw bytes in order, then four zero bytes |
| `RecordSample` | One 16-byte sample: `vbus_uv`, `ibus_ua`, `charge_uah`, `energy_uwh` (all `i32`), `raw: [u8; 16]`; `RecordSample::decode`; `vbus_v`, `ibus_a`, `charge_ah`, `energy_wh` |
| `SAMPLE_FIELDS` | Each sample field's key, offset and confidence `Tag` |
| constants | `ENTRY_LEN` (48), `SAMPLE_LEN` (16), `FLASH_WINDOW_BASE` (`0x9810_0000`), `MEMORY_READ_MAX_LEN` (256), `RECORD_AREA_LEN` (`0x20_0000`) |

### `decode::settings`

| item | signature or contents |
|---|---|
| `Settings` | The raw settings block: `Settings::decode(payload: &[u8]) -> Result<Settings, DecodeError>`; `block_a_crc_ok`, `block_b_crc_ok`, `rate_code`, `name`, `raw`; `meter_settings(&self) -> Result<MeterSettings, DecodeError>` (refuses a block whose CRC fails) |
| `MeterSettings` | Each field as `Tagged<T>` (`value`, `tag`): `language`, `brightness`, `dim_timeout`, `startup_logo`, `theme`, `auto_rotate`, `rotation`, `sample_rate`, `voltage_cal`, `current_cal`, `offset_cal`; `rows(&self) -> Vec<Row>` (`key`, `label`, `value`, `tag`) for display |
| `Tag` | `Confirmed` or `Inferred`; `as_str` |
| field enums | `Language`, `DimTimeout`, `StartupLogo`, `Theme`, `Rotation` |
| `WritableField` | The fields a settings write may change: `Language`, `Brightness`, `DimTimeout`, `Theme`, `Rotation`, `SampleRate` (`ALL`); `name`, `from_name`, `key`, `from_key`, `range` (allowed wire codes), `code` (the value held now), `code_after(current, changes)` |
| `settings_write_request` | `settings_write_request(id: u8, current: &MeterSettings, changes: &[(WritableField, i32)]) -> Result<Vec<u8>, SettingsWriteError>`: the type-0x0A frame, every key at its new or read value and the save marker last |
| `SettingsWriteError` | `OutOfRange` (a change outside its field's range) or `MeterValueOutOfRange` (a field left unchanged that the meter holds out of range); nothing is sent |
| helpers | `apply_settings_write` (applies a write to a 180-byte block as the meter does; used by the simulator), `settings_key_range`, `settings_entry_word` |

### `session` records, settings writes and ADCNEW

`Session` methods; only `delete_records` and `write_settings` write to the meter:

| method | contents |
|---|---|
| `list_records(&mut self) -> Result<Vec<RecordEntry>, Error>` | Authenticates to level 1 when needed, then one GET_DATA of the record list |
| `download_record(&mut self, entry: &RecordEntry) -> Result<Vec<RecordSample>, Error>` | Reads the entry's window in chunks of at most 256 bytes; any address outside it is refused |
| `delete_records(&mut self, list: &[RecordEntry], positions: &[usize]) -> Result<Vec<RecordEntry>, Error>` | Rewrites the record table without the entries at `positions` (one SET_DATA, attribute 0x28; the meter answers FINISHED), then reads the list again and returns it. A position outside `list` or a table over `RECORD_TABLE_MAX_LEN` bytes (`Error::RecordTableTooLong`) is refused before anything is sent; a re-read that differs is `Error::RecordTableMismatch`. Nothing is retried |
| `write_settings(&mut self, changes: &[(WritableField, i32)]) -> Result<MeterSettings, Error>` | Reads the settings, sends one settings write (over the CDC pipe; bulk opens it through `settings_pipe`), reads them back and returns them. A refused change is `Error::SettingsWrite`; a read-back where a changed field lacks its value or another field moved is `Error::SettingsMismatch` naming it. Nothing is retried |
| `start_adcnew(&mut self, content: Content, rate: Rate)`, `stop_adcnew(&mut self)` | Start and stop the ADCNEW stream on a CDC session (50 or 1000 SPS) |
| `poll_adcnew(&mut self) -> Result<StreamPoll, Error>` | The records received since the last poll, with gaps; sends the keep-alive when due and restarts the stream after a start echo |
| `adcnew_restart_pending(&self) -> bool` | A start echo was seen and the start is waiting for `START_ECHO_QUIET` of quiet |
| `adcnew_counters(&self) -> AdcnewCounters` | `crc_errors`, `dropped_frames`, `start_echoes` |
| `take_adcnew_raw(&mut self) -> Vec<Frame>` | The raw frames kept for a trace (at most `MAX_RAW_FRAMES`) |

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

## PDM text session

`pdm` drives the meter's text commands on its virtual serial port. The
session and the risk check are in the root of the module; each feature has a
submodule. All functions that talk to the meter take a
`&mut PdmSession<P: Read + Write, C: Clock>`, so tests run them on a fake
port.

| item | signature or contents |
|---|---|
| `PdmSession<P, C = SystemClock>` | `open(port: P) -> Result<Self, Error>`, `open_with_clock(port: P, clock: C)`; `send(&mut self, command: &Command)`, `send_confirmed(..)` (above 5 V, with a `HighVoltageConfirmation`), `wait_for(&mut self, prefixes: &[&str], timeout: Duration) -> Result<String, Error>`, `risk(&self, command: &Command) -> Risk`, `check_in_step(&self)`, `capabilities(&self) -> &Capabilities`, `entered(&self) -> Option<Protocol>`, `reopen(&mut self, port: P)`, `close(self)`. `pdm close` is also sent on drop. |
| session constants | `MIN_COMMAND_GAP` (60 ms), `REPLY_TIMEOUT` (1 s) |
| `HighVoltageConfirmation` | `HighVoltageConfirmation::for_command(command: &Command)`: a confirmation that covers that one command |
| `Command` | Every command the session sends; `render(&self) -> Vec<u8>` (the exact ASCII), `Command::parse(text: &str) -> Command` (unknown text is `Command::Raw`) |
| `Risk`, `classify` | `Risk::{ReadOnly, AtMost5V, Above5V}`; `classify(command: &Command, caps: &Capabilities) -> Risk`, pure and fail-closed; `risk::SAFE_LIMIT_MV` (5000) |
| `ReplyReader`, `ReplyItem` | The pure stream reader: `push(&mut self, bytes: &[u8])`, `next_item(&mut self) -> Option<Result<ReplyItem, Error>>`; `reply::MAX_LINE` (4096) |
| `caps` | `parse(buf: &[u8]) -> Result<Option<(CapBlock, usize)>, Error>`; `CapBlock`, `PdoBlock`, `UfcsCaps` / `UfcsCap`, `ScpCaps`, `VfcpCaps`, `TfcpCaps`; `Capabilities` (the last block of each kind) |
| `port` | `find_port(ports, serial: Option<&str>)`, `open_port(serial: Option<&str>) -> Result<Box<dyn serialport::SerialPort>, Error>` |
| `Error` | `Timeout`, `Malformed`, `HighVoltageNotConfirmed`, `Refused` (GoodCRC), `Forbidden`, `UnsupportedOnCdc`, `LineTooLong`, `UnexpectedReply`, `OutOfSync` (the meter needs a power cycle), `NotFound`, `Ambiguous`, `Disconnected`, `Io` |
| `pd` | USB PD: `enter`, `ensure_entered`, `offers` / `Offer` / `OfferKind`, `validate`, `request` / `Request` / `RequestOutcome`, `hard_reset`, `return_to_5v` / `BackTo5V`, `read_settings` / `write_settings` / `PdSettings` (`PdType`, `Emarker`, `SinkCaps`), `Error`; `FIRST_EPR_POSITION` (8) |
| `legacy` | QC 2.0, QC 3.0, FCP, AFC, SFCP, SCP, VFCP, TFCP and UFCS: `LegacyProtocol`, `enter`, `ensure_entered`, `limits` / `Limits`, `validate`, `request` / `Request` / `Outcome`, `return_to_5v`, `Error`; `QC3_MIN_MV`, `QC3_MAX_MV`, `QC3_STEP_MV` |
| `detect` | `bc12`, `apple` (each answers a `Support`), `list` / `list_until` with `ListOptions` → `ListResult` of `ListEntry`; `disagreements` / `Disagreement`; `hint(dp_mv: u32, dm_mv: u32) -> Hint`, `hint_from_volts(dp_v: f64, dm_v: f64) -> Hint` and `ChargeMode`; `DEFAULT_STALL_LIMIT` (45 s) |
| `message` | Custom PD messages: `PdMessage` (`control`, `data`, `from_hex`, `to_command`), `SopTarget`, `Preset` (the cable Discover Identity), `await_cable_identity` / `CableIdentity`, `cable_identity_from_message`, `refusal` (GoodCRC), `MessageError` |
| `console` | The raw console: `send_line` (returns `SentLine`), `command_of`, `console_refusal`, `effect_of` / `Effect` (what has to be undone before `pdm close`), `ConsoleEntry` / `Direction`, `describe_block`; `LISTEN` (300 ms), `listen_for(command: &Command) -> Duration` (the entry bound for an `entry` line) |

The text protocol itself is described in
[docs/protocol.md](../../protocol.md#pdm-text-session-protocol-triggering).

## Firmware update

`firmware` reads versions and firmware files and runs the update. Only
`flash` can send write-capable frames, and only with a `FlashAck`.

| item | signature or contents |
|---|---|
| `DeviceMode` | `Application`, `Bootloader`, `Unknown`; `DeviceMode::from_interface_string(interface: Option<&str>) -> DeviceMode` (`APP` or `BOOT`), `name(self) -> &'static str` |
| `version` | `Version` (`parse(s: &str) -> Option<Version>`), `MIN_SUPPORTED` (1.7.0), `KNOWN_LATEST` (2.0.7), `FirmwareWarning` (`code(self) -> &'static str`), `warnings(version: &str, update_required: bool) -> Vec<FirmwareWarning>` |
| `container` | `FirmwareContainer::parse(bytes: &[u8]) -> Result<FirmwareContainer, ContainerError>` with `vendor`, `container_version`, `model`, `firmware_version`, `date`, `crc` / `crc_hex`, `size` (the declared image size), `release_notes_en` / `release_notes_cn`, `payload`, `check_model(meter: &str)`; `read_container_file(path: &Path)`; `MAX_CONTAINER_BYTES` (8 MiB). The image is never decrypted. |
| `flash::flash` | `flash(ack: FlashAck, container: &FirmwareContainer, opener: &mut impl MeterOpener, clock: impl Clock, stop: &AtomicBool, progress: &mut dyn FnMut(FlashProgress)) -> Result<FlashOutcome, FlashError>` |
| `flash::jump` | `jump(ack: FlashAck, target: JumpTarget, opener: &mut impl MeterOpener, clock: impl Clock, stop: &AtomicBool, sent: &mut dyn FnMut(&JumpSent)) -> Result<JumpOutcome, FlashError>` |
| `FlashAck` | `FlashAck::acknowledge()`: the marker that the user explicitly asked for a flash or a jump |
| `MeterOpener`, `Meter`, `FlashTransport` | Finds and opens the meter again after each re-enumeration (`open(&mut self) -> Result<Option<Meter<Self::Pipe>>, transport::Error>`); `FlashTransport::write_raw` writes without awaiting a reply. `transport::bulk::BulkFlashOpener` is the real one. |
| `FlashGuard` | The strict guard of the flash path: CONNECT, HEAD, the info and UID reads, a level-1 authentication, the two jumps, a descriptor starting at 0x4000 and the payload writes after it; everything else is refused before it is sent |
| `FlashProgress` | `Switching`, `Erasing`, `Writing { done, total }`, `Verifying`, `Finished`, `Returning`, `Replug { to, reply }` |
| `FlashOutcome` | `Verified { version }` or `PowerCycleNeeded` |
| `FlashError` | One variant per step (`Product`, `Read`, `Write`, `Erase`, `Download`, `Verify`, `Device`, `Interrupted`, `NotInBootloader`, `WorkMode`, `AlreadyInMode`, `UnknownMode`); `code(&self) -> i32` gives the vendor error number |
| `JumpTarget`, `JumpReply`, `JumpSent`, `JumpOutcome` | The mode asked for, the meter's reply to the jump (`Accept`, `Reject`, `Other`, `NoReply`, `Disconnected`), what was sent, and the mode the meter came back in |
| constants | `APP_START` (0x4000), `BLOCK_LEN` (4080), `MAX_APP_END`, `SWITCH_TIMEOUT` and `RETURN_TIMEOUT` (120 s), `JUMP_REPLY_WAIT` (500 ms), `POLL_INTERVAL`, `BACK_BUTTON_HINT` |

Related items elsewhere:

- `Session::firmware_status`, `read_firmware_status` and
  `read_firmware_status_or_unknown` read both versions and the HEAD reply;
  `Session::probe` / `probe_with_head_reply` send only CONNECT and HEAD
  (`ProbeReport`, `ConnectOutcome`, `HeadOutcome`). A SYNC reply is
  `session::Error::BootloaderMidDownload`.
- `transport::discovery::find_flashed_meter` and `follow_flashed_meter` pick
  the meter by its USB serial, which is the same in both modes.

The firmware download sequence is described in
[docs/protocol.md](../../protocol.md#firmware-download).
