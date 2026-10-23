# POWER-Z KM003C / KM002C protocol reference

A clean-room description of the binary protocol the KM003C / KM002C speaks
over USB, written from our own research notes and the public vendor API
description. No vendor code, vendor text, key bytes or device identifiers are
reproduced here. The AES keys the protocol needs live in the library module
`crypto::keys` (`crates/powerz-protocol/src/crypto/keys.rs`), not in this
document.

All multi-byte values are little-endian unless stated otherwise. Bit fields
are counted from the least significant bit of a u32.

## Legend

Every fact carries one or more confidence tags:

| tag | meaning |
|---|---|
| `[device]` | observed on our own KM003C in on-device session 1 (2026-10-20, application firmware 2.0.6); raw frames are kept in the research workspace |
| `[static]` | read from the shipped vendor PC software (V2.5.2) by static analysis with high confidence, or worked out by hand from vendor sample bytes |
| `[doc]` | stated in a public vendor document (USB API description, serial trigger instructions, ADCNEW note, user manual, changelog) |
| `[inferred]` | plausible but not proven: a reasoned guess, or a fact reported by third-party projects (tagged `[inferred] (prior art)`) from their own hardware, mostly firmware 1.9.9 |

Where the device contradicted an earlier static or documentary reading, the
device result wins; such rows say "device overrides" and give the old reading.
KM002C facts stay `[inferred]` until a KM002C is tested.

## USB identity and interfaces

| item | value | tag |
|---|---|---|
| vendor ID | 0x5FC9 for every POWER-Z meter | [doc] [static] [device] |
| product ID, KM003C | 0x0063 | [device] (firmware 2.0.6) |
| product ID, KM002C | 0x0061 | [inferred] (prior art) |
| PID matching | the vendor app accepts any PID 0x00xx and never branches on it; the model comes from the device-info block | [static] |
| device class | 0xEF / 0x02 / 0x01 (composite with an interface association), USB 2.10, full speed, EP0 32 bytes | [device] (firmware 2.0.6) |
| string 4 (iProduct) | `POWER-Z KM003C` | [device] (firmware 2.0.6) |
| string 2 | `APP` in application mode (the interface string of IF0 and IF1, not the product string); anything else means boot loader | [static] [device] (firmware 2.0.6) |
| string 3 (iSerial) | the device serial number (not reproduced here) | [device] |
| WinUSB binding | MS OS 2.0 descriptor set (vendor request code 0x20) gives interface 0 the `WINUSB` compatible ID | [device] (firmware 2.0.6) |

Interfaces on the KM003C (firmware 2.0.6). Endpoints are all 64 bytes unless
noted.

| IF | class | endpoints | use | tag |
|---|---|---|---|---|
| 0 | 0xFF vendor | bulk OUT 0x01, bulk IN 0x81 | main binary protocol; everything tested works here | [device] (firmware 2.0.6) [static] |
| 1 | 0x02 CDC comm | interrupt IN 0x83, 8 bytes, interval 10 | CDC notifications only | [device] (firmware 2.0.6) |
| 2 | 0x0A CDC data | bulk OUT 0x02, bulk IN 0x82 | virtual serial port: text trigger commands, binary GET_DATA and the ADCNEW stream | [device] (firmware 2.0.6) [doc] |
| 3 | 0x03 HID | interrupt IN 0x85, OUT 0x05, interval 1 | vendor usage page 0xFF00, one 64-byte input and one 64-byte output report, no report IDs; see HID limits | [device] (firmware 2.0.6) |

- Interface numbers differ between reports (the KM002C is said to put HID at
  IF1 and CDC at IF2/IF3), so pick interfaces by class, never by number.
  [inferred] (prior art) [device]
- The vendor API description says the same binary commands work over bulk,
  CDC and HID (roughly 200 KB/s, 200 KB/s and 60 KB/s). [doc] On the device
  that is only partly true; see HID limits and CDC. [device]
- macOS: libusb can claim IF0 without detaching a driver; HID is held by the
  OS HID stack and is reachable only through hidapi. [device] Linux: the
  mainline `powerz` hwmon driver binds IF0 and must be detached first.
  [inferred] (prior art)
- The vendor app keeps one request in flight, takes the next IN transfer as
  the reply, and sends a zero-length packet after any OUT write whose length
  is a non-zero multiple of 64. [static]

## Header bit layouts

Every packet starts with a 4-byte little-endian header. The same 32 bits are
read in one of three views.

### Main control view

Used by requests and by control replies: `type` in bits 0..6, `extend` in
bit 7, transaction `id` in bits 8..15, bit 16, and the attribute `att` in
bits 17..31. [static] [doc]

| bits | field | meaning | tag |
|---|---|---|---|
| 0..6 | type | message type, below 0x40 for control messages | [doc] [static] |
| 7 | extend | always 0 in requests | [static] |
| 8..15 | id | transaction id; the vendor app pre-increments one global u8 per frame | [static] |
| 16 | enc | 0 in every control request | [static] |
| 17..31 | att | attribute or argument; `att = 1` therefore shows up as byte 2 = 0x02 | [doc] [static] |

Worked example: `0C 00 02 00` is GET_DATA, id 0, att 1 (the ADC snapshot).
[doc] [static]

### Main data view

Used by data replies such as PUT_DATA: `type` in bits 0..6, `extend` in
bit 7, `id` in bits 8..15, bit 16 marks an encrypted payload, and `obj` in
bits 22..31. [static] [doc]

| bits | field | meaning | tag |
|---|---|---|---|
| 0..6 | type | 0x40 and up | [doc] [static] |
| 7 | extend | set on the memory-read confirmation (`C4 ..`), clear everywhere else seen | [device] (firmware 2.0.6) |
| 8..15 | id | echoed from the request in every reply except the 0x4C auth reply, which always has id 0; any request id is accepted, including 0 and repeats | [device] (firmware 2.0.6) |
| 16 | encode | 1 = payload is AES-128-ECB encrypted; 0 on PUT_DATA, 1 on the memory-read confirmation and the auth reply | [doc] [static] [device] |
| 17..18 | permi | the session auth level (0, 1 or 2); documented without a meaning, device overrides | [device] (firmware 2.0.6) |
| 19..21 | reserved | always 0 seen | [doc] [device] |
| 22..31 | obj | the docs give it as the payload size counted in 32-bit words; on PUT_DATA it only roughly tracks the length, so never frame by it | [doc] [static] [device] |

- Device overrides the docs on `obj`: for PUT_DATA it behaves like
  `floor((total_len - 12) / 4)` in 10-bit two's complement, with at least one
  unexplained exception; on 0x44 and 0x4C replies it is the payload length in
  words. Parse plain replies by walking the section headers. [device]
  (firmware 2.0.6)
- [device] On firmware 2.0.6 every reply header after authentication carries
  the auth level in bits 17..18: the ADC reply header reads `41 id 80 02` at
  level 0, `41 id 82 02` at level 1 and `41 id 84 02` at level 2.

### Extended section header

Each block inside a data payload starts with its own 4-byte header: `att` in
bits 0..14, `next` in bit 15, `chunk` in bits 16..21 and `size` (payload
bytes) in bits 22..31. [static] [doc]

| bits | field | meaning | tag |
|---|---|---|---|
| 0..14 | att | the attribute of this section: one bit of the GET_DATA mask | [doc] [static] |
| 15 | next | 1 = another section follows | [doc] [static] [device] |
| 16..21 | chunk | record count for ADC_QUEUE (so at most 63 records per reply) | [static] [device] |
| 22..31 | size | section length in bytes; for ADC_QUEUE the record size | [doc] [static] [device] |

- Section length = `chunk * size` for ADC_QUEUE, `size` otherwise. Always
  skip that many bytes, including for unknown attributes. [static]
- Sections arrive in ascending attribute-bit order; an attribute that is
  gated or has nothing to report is simply left out. [device]
  (firmware 2.0.6)
- Exception: PD_STATUS declares size 0 but trailing bytes follow; treat it as
  "rest of the frame" (see GET_DATA attributes). [device] (firmware 2.0.6)

## Message catalogue

Control types (main header type below 0x40):

| type | name | argument (att) | reply | tag |
|---|---|---|---|---|
| 0x01 | SYNC | none | used as a per-chunk acknowledgement during firmware download; on CDC this byte starts ADCNEW instead | [doc] [static] [device] |
| 0x02 | CONNECT | 0 | ACCEPT over bulk; required before memory read (and so before auth) on bulk; no reply over HID; on CDC it starts an ADCNEW stream | [doc] [static] [device] (firmware 2.0.6) |
| 0x03 | DISCONNECT | none | can arrive instead of PUT_DATA; a stale one often waits on EP 0x81 at open, so drain IN first | [doc] [static] [device] |
| 0x04 | RESET | none | not used by the vendor app; never sent by us | [doc] [inferred] |
| 0x05 | ACCEPT | reply only | | [doc] [static] [device] |
| 0x06 | REJECT | reply only | | [doc] [static] [device] |
| 0x07 | FINISHED | reply only | acknowledges flash-write chunks | [doc] [static] |
| 0x08 | JUMP_APROM | 0 | none; leaves DFU mode; never sent by us | [doc] [static] |
| 0x09 | JUMP_DFU | 0 | none; enters firmware-update mode; never sent by us | [doc] [static] |
| 0x0A | GET_STATUS | | no handler in the vendor app | [doc] |
| 0x0B | ERROR | | no handler in the vendor app | [doc] |
| 0x0C | GET_DATA | attribute bit mask | PUT_DATA (0x41) | [doc] [static] [device] |
| 0x0D | GET_FILE | | unused by the vendor app | [doc] [static] |
| 0x0E | start ADC queue | rate code | ACCEPT at auth level 1 or more; REJECT at level 0 and while a stream is already started | [static] [device] (firmware 2.0.6) |
| 0x0F | stop ADC queue | 0 | ACCEPT at level 1 or more, REJECT at level 0 | [static] [device] (firmware 2.0.6) |
| 0x10 | start PD / UFCS sniffer | mode: 1 = PD, 2..5 = UFCS | ACCEPT at level 1, REJECT at level 0 | [static] [device] (firmware 2.0.6) |
| 0x11 | stop sniffer | 0 | ACCEPT at level 1, REJECT at level 0 | [static] [device] (firmware 2.0.6) |

Data types (main header type 0x40 and up):

| type | request header | payload | purpose | tag |
|---|---|---|---|---|
| 0x40 HEAD | `40 id 00 01` | 16 zero bytes | firmware-download pre-check: the device must report it is ready for an update | [static]; meaning [inferred] |
| 0x41 PUT_DATA | reply only | section chain | GET_DATA reply | [doc] [static] [device] |
| 0x42 | `42 id 00 04` | 64-byte firmware descriptor | firmware download; never sent by us | [static] |
| 0x44 | `44 id 01 01` | 32 encrypted bytes | memory read (see Memory read and device info) | [static] [device] |
| 0x48 SET_DATA | varies | section header + plain data | writes a setting or calibration value; never sent by us | [static] |
| 0x4A | varies | encrypted block header + data | memory / SPI-flash write; never sent by us | [static] |
| 0x4C | `4C id 00 02` | 32 encrypted bytes | session authentication (see Authentication) | [static] [device] |

Other opcodes reported by third parties (0x4B, 0x4D, 0x27) are not seen in
the V2.5.2 software. [inferred] (prior art)

## GET_DATA attributes

The request is `0C id b2 b3` with `(b3 << 8 | b2) = mask << 1`. Several bits
can be combined into one request; the reply then holds one section per
attribute, chained by `next`. [doc] [static] [device]

| mask | name | request bytes | reply section | auth level needed | tag |
|---|---|---|---|---|---|
| 0x001 | ADC | `0C id 02 00` | one 44-byte snapshot | 0 | [doc] [static] [device] (firmware 2.0.6) |
| 0x002 | ADC_QUEUE | `0C id 04 00` | `chunk` x 20-byte records | 0 for polling (device overrides the "needs auth" reading) | [doc] [static] [device] (firmware 2.0.6) |
| 0x004 | ADC_QUEUE_10K | `0C id 08 00` | always empty, also at level 2 and queue code 4 | n/a | [doc] [device] (firmware 2.0.6) |
| 0x008 | SETTINGS | `0C id 10 00` | 180 bytes in two CRC-32 blocks | 0 | [doc] [static] [device] (firmware 2.0.6) |
| 0x010 | PD_PACKET | `0C id 20 00` | 12-byte status + PD / event records | 0 for polling | [doc] [static] [device] (firmware 2.0.6) |
| 0x020 | PD_STATUS | `0C id 40 00` | size 0, then 2 or 12 trailing bytes | 0 | [doc] [device] (firmware 2.0.6) |
| 0x040 | QC_PACKET | `0C id 80 00` | UFCS sniffer stream; idle: 12-byte status + 8 unknown constant bytes | 0 for polling | [doc] [static] [device] (firmware 2.0.6) |
| 0x080 | TICK | `0C id 00 01` | 8 bytes: u32 device uptime in ms, u32 0 | 1 (empty at 0) | [doc] [device] (firmware 2.0.6) |
| 0x100 | TICK2 | `0C id 00 02` | always empty | n/a | [doc] [device] |
| 0x200 | RecordList | `0C id 00 04` | N x 48-byte offline-record entries | 1 (empty at 0) | [static] [device] (firmware 2.0.6) |
| 0x400 | PDList | `0C id 00 08` | always empty | n/a | [static] [device] |

- [device] On firmware 2.0.6, PD_STATUS declares size 0 in its section
  header, yet trailing bytes follow it (12 bytes on the first read, then 2).
  It always came last. A parser must treat it as "the rest of the frame".
- A DISCONNECT reply means "no data"; a PUT_DATA shorter than 5 bytes means
  "nothing new". [static] An empty PUT_DATA (`41 id 00 00`, no section) is
  what gated attributes return on their own. [device]
- Prior art says level 0 masks requests to ADC, SETTINGS and PD_PACKET; on
  firmware 2.0.6 the queue, PD_STATUS and QC_PACKET are also served at level
  0, while TICK and RecordList are not. Device overrides. [inferred] (prior art)
  [device]

## ADC snapshot

[device] On firmware 2.0.6 the ADC snapshot payload is 44 bytes: the section
header reads att 1, size 44 (`01 00 00 0B`), and the whole reply is 52 bytes
(4-byte main header, 4-byte section header, 44-byte payload). The vendor API
description shows a 40-byte layout; device overrides. Parse by `size` and
accept 40 for older firmware. [doc] [inferred]

| off | type | field | scale | tag |
|---|---|---|---|---|
| 0x00 | i32 | VBUS | µV, / 1e6 for V | [doc] [static] [device] |
| 0x04 | i32 | IBUS, signed (negative = current from receptacle to plug) | µA, / 1e6 for A | [doc] [static] [device] |
| 0x08 | i32 | VBUS average | µV | [doc] [device] |
| 0x0C | i32 | IBUS average | µA | [doc] [device] |
| 0x10 | i32 | VBUS average, uncalibrated | µV | [doc] [static] |
| 0x14 | i32 | IBUS average, uncalibrated | µA | [doc] [static] |
| 0x18 | i16 | T1, ADC chip temperature | / 128 for °C; 0x8000 = invalid | [static] [doc]; 29-30 °C idle and 0x8000 while the queue runs at 1000 SPS [device] (firmware 2.0.6) |
| 0x1A | u16 | CC1 | 0.1 mV, / 10000 for V | [doc] [static] [device] |
| 0x1C | u16 | CC2 | 0.1 mV | [doc] [static] [device] |
| 0x1E | u16 | D+ | 0.1 mV | [doc] [static] [device] |
| 0x20 | u16 | D- | 0.1 mV | [doc] [static] [device] |
| 0x22 | u16 | internal VDD | 0.1 mV (3.205 V seen) | [doc]; unit [inferred]; [device] |
| 0x24 | i16 | second temperature (T2) per the PC app, a rate byte per the API description | raw; 0x8000 = invalid | [static] [doc]; always 0x8000 on our unit [device] (firmware 2.0.6) |
| 0x26 | u8 | rate code of the running ADC queue (0..3) | code | [static] [device] (firmware 2.0.6) |
| 0x27 | u8 | unknown | | 0xF0 constant [device] |
| 0x28 | u16 | battery level x 10 for the Apple battery feature | / 10 for % | [static]; 0 with nothing attached [device] |
| 0x2A | u16 | unknown | | 0x00FF constant [device] |

- Worked example (idle, nothing attached, firmware 2.0.6): VBUS 4196 µV,
  IBUS -95 µA, T1 raw 3770 = 29.45 °C, CC1 805 = 80.5 mV, VDD 32048 =
  3.205 V. [device]
- Power is `V * I` (signed) on the host. [static]
- The snapshot works over bulk, CDC and HID, needs no CONNECT and no auth.
  [device] (firmware 2.0.6)

## ADC queue streaming

The queue is a buffer of 20-byte samples that the meter fills at a chosen
rate. [static]

- [device] On firmware 2.0.6 the queue is always running at the meter's own
  rate (bits 2..3 of the SETTINGS u32 at 0x60), and queue polling works
  without auth: `0C id 04 00` returns records at level 0. Earlier notes said
  polling needed auth; device overrides.
- [device] On firmware 2.0.6 all four gated control commands, 0x0E / 0x0F
  (queue start / stop) and 0x10 / 0x11 (sniffer start / stop), get REJECT
  below auth level 1 and ACCEPT at level 1 or more. So choosing the rate
  needs auth, reading the queue does not.
- Start: `0E id (code << 1) 00`, expect ACCEPT. Stop: `0F id 00 00`. Then
  poll `0C id 04 00` (or `0C id 06 00` together with the snapshot). [static]
- [device] On firmware 2.0.6, 0x0E while a stream is already started gets
  REJECT; send 0x0F first. After 0x0F the queue keeps filling at the meter's
  default rate.
- [device] On firmware 2.0.6 one reply holds at most 63 records (6-bit
  `chunk`); the backlog is kept between polls. At 1000 SPS poll at least every
  60 ms (the vendor app uses 32 ms) or samples are lost.

Rate codes, as measured on firmware 2.0.6 (10 s per code at auth level 1):

| code | rate | sample interval | CC / D± unit | tag |
|---|---|---|---|---|
| 0 | 2 SPS | 500 ms | 0.1 mV | [device] (firmware 2.0.6); the PC app labels it 1 SPS and the ADCNEW note 4/s, device overrides |
| 1 | 10 SPS | 100 ms | 1 mV | [device] (firmware 2.0.6) |
| 2 | 50 SPS | 20 ms | 1 mV | [device] (firmware 2.0.6) |
| 3 | 1000 SPS | 1 ms | 1 mV | [device] (firmware 2.0.6) |
| 4 | ACCEPTed, but behaves like code 0 (2 SPS, 0.1 mV); no 10 kSPS | 500 ms | 0.1 mV | [device] (firmware 2.0.6) |

Record layout (section att 2, `chunk` = record count, `size` = 20):

| off | type | field | scale | tag |
|---|---|---|---|---|
| 0x00 | u32 | device time (uptime) | ms | [static] [device] (firmware 2.0.6); prior art's "u16 sequence + marker" reading is wrong, device overrides |
| 0x04 | i32 | VBUS | µV | [doc] [static] [device] |
| 0x08 | i32 | IBUS, signed | µA | [doc] [static] [device] |
| 0x0C | u16 | CC1 | 0.1 mV at code 0 and 4, 1 mV otherwise | [doc] [static] [device] (firmware 2.0.6) |
| 0x0E | u16 | CC2 | as CC1 | [doc] [static] [device] |
| 0x10 | u16 | D+ | as CC1 | [doc] [static]; not yet checked with a QC charger [inferred] |
| 0x12 | u16 | D- | as CC1 | [doc] [static]; not yet checked with a QC charger [inferred] |

Vendor poll periods per rate code (scheduler): 1000, 200, 100, 32 ms for codes
0..3, 10 ms above. [static] Whether a 0x0E stream stops on its own after an
hour, as ADCNEW does, is unknown. [inferred]

## SETTINGS

[device] On firmware 2.0.6 the SETTINGS section (GET_DATA mask 0x008) is 180
bytes: main header `0x0B000141`, section header att 8, size 180, total reply
188 bytes, the same over bulk and CDC. It holds two blocks, each protected by
its own CRC-32 (standard IEEE / zlib). The CDC trigger tool reads only the
first 96 bytes; device overrides its "96 bytes" reading.

| off | type | field | tag |
|---|---|---|---|
| 0x00..0x5F | bytes | block A (96 bytes) | [static] [device] (firmware 2.0.6) |
| 0x10 | u32 | run-time overlay: 0xFFFFFFFF at first, later a Unix time near the last reboot; the stored block-A CRC was computed with 0xFFFFFFFF here, so decoders check the CRC with these 4 bytes restored | [device]; meaning [inferred] |
| 0x1C | 5 x i32 | an array (5 equal values seen), not a single 1e6-scaled gain as static notes said; device overrides | [device] [inferred] |
| 0x30 | 10 x i32 | an array of values near 1 000 000 (likely gains, 1e6 = 1.0) | [static] [device] [inferred] |
| 0x5C | u32 | CRC-32 of bytes 0x00..0x5B | [static] [device] (firmware 2.0.6) |
| 0x60..0xB3 | bytes | block B (84 bytes) | [static] [device] (firmware 2.0.6) |
| 0x60 | u32 | flags; bits 2..3 = the meter's queue rate code | [static] [device] |
| 0x70 | ASCII | `POWER-Z`, probably the name shown on screen | [device] [inferred] |
| 0xB0 | u32 | CRC-32 of bytes 0x60..0xAF | [device] (firmware 2.0.6) [inferred] (prior art) |

- Writing settings (SET_DATA 0x48) is out of scope and never sent. [static]
- Meter UI settings (brightness, rotation, language) are set some other way
  that is still unknown. [inferred]

## Memory read and device info

[device] On firmware 2.0.6, memory read (type 0x44) works without auth: all
four known addresses were read at auth level 0. [device] It does need CONNECT
first on firmware 2.0.6: without a prior CONNECT on the bulk pipe the meter
answers the memory read with REJECT (`06 id 00 00`); after CONNECT the same
request succeeds (checked 2026-10-20 with the `powerz` CLI). Auth reads the UID
or OTP by memory read, so it needs CONNECT too. [device] Over CDC the meter gives no
reply at all to a memory read (FW 2.0.6, 2026-10-20), and CONNECT cannot be sent there
(0x02 starts ADCNEW), so device info, auth and choosing the queue rate need the bulk pipe.

Request: header `44 id 01 01`, then 32 bytes, the AES-128-ECB encryption
(key slot 0, see `crypto::keys`) of this plaintext: [static] [device]

| off | type | field | tag |
|---|---|---|---|
| 0x00 | u32 | address | [static] [device] |
| 0x04 | u32 | length in bytes | [static] [device] |
| 0x08 | u32 | 0xFFFFFFFF | [static] [device] |
| 0x0C | u32 | CRC-32 of bytes 0x00..0x0B | [static] [device] |
| 0x10 | 16 bytes | 0xFF padding | [static] [device] |

Reply, as seen on firmware 2.0.6: [device]

| step | bytes | content | tag |
|---|---|---|---|
| 1 | 20 | confirmation: header `C4 id 01 01` (type 0x44 with extend and encode set) + the plaintext first 16 request bytes | [device] (firmware 2.0.6) |
| 2 | roundup16(length) | the data, AES-128-ECB with key slot 0, in a separate transfer with no header | [static] [device] (firmware 2.0.6) |

Known addresses (the only ones we ever read):

| address | length | content | tag |
|---|---|---|---|
| `0x420` | 64 | info block: four strings at 0x10 / 0x1C / 0x28 / 0x34 = model (`KM003C`), version (`2.5`), date, extra; likely the boot loader / hardware block | [static] [device]; meaning [inferred] |
| `0x4420` | 64 | application firmware info: u32 0x4000 (app start), size, CRC, then model, version (`2.0.6`), date; first u32 0xFFFFFFFF would mean "none" | [static] [device] (firmware 2.0.6) |
| `0x03000C00` | 64 | OTP block: ASCII serial number, a 128-bit hex id, a production Unix time, a short tag; the first 16 bytes are the level-2 auth credential | [static] [device] |
| `0x40010450` | 12 | MCU unique ID; the level-1 auth credential | [static] [device]; meaning [inferred] |

- Keep reads at least 50 ms apart (100 ms in session 1). Third parties report
  that other peripheral addresses crash the meter. [inferred] (prior art)
- The SPI-flash read window at `0x98000000 + offset` holds offline
  recordings; it is not on our allowed list yet. [static]

## Authentication

Authentication (type 0x4C) raises the session auth level. [static] [device]

- Request: header `4C id 00 02` (bit 16 clear), then 32 bytes, the
  AES-128-ECB encryption (key slot 3, see `crypto::keys`) of:
  `u64 host_time_ms`, the credential, `u64 random`, padded with 0xFF to 32
  bytes. [static] [device]

| level | credential | reply header | tag |
|---|---|---|---|
| 1 | the 12-byte MCU UID from `0x40010450` | `4C 00 03 02` (word `0x0203004C`) | [device] (firmware 2.0.6) |
| 2 | the first 16 bytes of the OTP block at `0x03000C00` | `4C 00 05 02` (word `0x0205004C`) | [device] (firmware 2.0.6) |
| refused | | bits 17..18 = 0 (`0x0201` reported) | [inferred] (prior art) |

- [device] On firmware 2.0.6 the reply has id 0 (not the echoed request id),
  bit 16 set, and the auth level in header bits 17..18.
- [device] On firmware 2.0.6 the decrypted reply (key slot 3, a variant for
  the reply) is exactly our plaintext, so the random value sits at offset
  `8 + credential length`: 20 for level 1, 24 for level 2. The vendor app
  checks bytes 24..31, which only fits level 2; compare the whole block.
- [device] On firmware 2.0.6 the auth level is device-wide (a bulk auth also
  shows in HID reply headers) and is lost when the host closes and reopens
  the device: after a reopen the level is back to 0 and 0x0E is REJECTed.
  Authenticate after every open.
- Auth over HID gets no reply. [device]
- Keep auth requests at least 1 s apart. [inferred] (prior art)
- On the KM002C the UID is reportedly refused and the OTP credential gives
  level 1. [inferred] (prior art)
- The KM003C accepted UID auth; level 2 unlocked nothing visible beyond
  level 1. [device]

## Session flow and errors

Recommended order for our library (bulk transport): [device] [static]

| step | action | tag |
|---|---|---|
| 1 | open, claim IF0, drain any stale DISCONNECT waiting on EP 0x81 | [device] |
| 2 | CONNECT (`02 id 00 00`, expect ACCEPT); memory read and auth are REJECTed without it | [static] [device] (firmware 2.0.6) |
| 3 | memory read of the UID and the OTP block | [static] [device] |
| 4 | authenticate (0x4C), level 1 | [static] [device] |
| 5 | memory read of the info blocks `0x420` and `0x4420` | [static] [device] |
| 6 | GET_DATA SETTINGS (and RecordList) | [static] |
| 7 | stop the queue (0x0F), then start it at the wanted rate (0x0E) | [static] [device] |
| 8 | poll the ADC snapshot every ~200 ms and the queue at the rate's period | [static] |

- One request in flight, at least 60 ms between requests (our own rule from
  session 1). [device] The vendor scheduler ORs due attributes into one
  GET_DATA per 10 ms tick. [static]
- The vendor app never checks reply ids; we may, since the device echoes
  them (except the 0x4C reply). [static] [device]
- Connect recovery: if CONNECT gets no ACCEPT within 500 ms, we clear the
  halt on both bulk endpoints (0x01 OUT, 0x81 IN), drain IN, and repeat
  CONNECT once; we never send RESET (0x04). [inferred] The vendor app does
  much the same when its HEAD request after CONNECT fails: it clears the halt
  on the bulk OUT endpoint, sends its own CLEAR_FEATURE ENDPOINT_HALT control
  transfer, and repeats CONNECT once. [static]

Error handling:

| condition | meaning | tag |
|---|---|---|
| REJECT (0x06) to 0x0E / 0x0F / 0x10 / 0x11 | auth level too low, or a stream already started | [device] (firmware 2.0.6) |
| DISCONNECT (0x03) instead of PUT_DATA | no data; also seen stale at open | [static] [device] |
| PUT_DATA shorter than 5 bytes | nothing new | [static] |
| empty PUT_DATA (`41 id 00 00`) | attribute gated or empty | [device] |
| no reply within ~1 s | timeout; the vendor app uses 1000 ms (2000 ms for CONNECT) | [static] |
| reply type not 0x41 for GET_DATA | protocol error | [static] |

## HID limits

HID writes are report ID 0 plus 64 bytes; replies are one 64-byte input report
with no report-ID byte, zero-padded. [device] Tested on macOS only; Linux and
Windows HID are untested. [inferred]

- [device] The ADC snapshot (GET_DATA mask 0x001, `0C id 02 00`) is the only
  request that is safe over HID: its 52-byte reply fits one report.
- PD_PACKET and QC_PACKET replies (20 and 28 bytes) fit in 64 bytes while
  nothing is attached (idle status). [device] With a charger attached they
  should carry PD / UFCS records and grow past 64 bytes, so they are not
  safe; not yet tested with a charger, reasoned from the 68-byte reply that
  rebooted the meter. [inferred]
- [device] Control commands get no reply over HID: CONNECT, 0x0E, 0x0F, 0x44
  and 0x4C all timed out, and a 0x0E sent over HID did not change the queue
  rate (not executed).
- [device] Any request whose reply exceeds 64 bytes (queue poll, SETTINGS,
  ADC + PD at 68 bytes) hangs the firmware: nothing arrives, later reads fail,
  and the meter reboots by watchdog 1.5-3.5 s later. It happened twice in
  session 1; stored settings were unchanged.
- Bulk is the transport; HID is at most an ADC-only fallback. [device]

| request over HID | result | tag |
|---|---|---|
| ADC snapshot (mask 0x001) | works, 52 bytes + padding | [device] (firmware 2.0.6) |
| PD_PACKET / QC_PACKET, nothing attached | works, 20 / 28 bytes | [device] (firmware 2.0.6) |
| PD_PACKET / QC_PACKET with a charger | expected to exceed 64 bytes | [inferred] |
| control commands (0x02, 0x0E, 0x0F, 0x44, 0x4C) | no reply, no effect | [device] (firmware 2.0.6) |
| any reply over 64 bytes | firmware hang and reboot | [device] (firmware 2.0.6) |

## CDC and the ADCNEW stream

The CDC data interface carries both the text trigger commands and the binary
protocol on one byte stream. [static] [doc] Port settings: the vendor tools
use 2 000 000 baud 8N1, but CDC ignores the baud. [static] [doc]

- Binary GET_DATA works on CDC with the same framing as bulk: ADC (52 bytes),
  SETTINGS (188 bytes), queue poll. [device] (firmware 2.0.6)
- [device] On firmware 2.0.6, types 0x01 / 0x02 / 0x03 on CDC are ADCNEW, not
  SYNC / CONNECT / DISCONNECT: a CONNECT sent on CDC started a 1000/s stream.
  Never send those control types on CDC except as ADCNEW.

ADCNEW is a separate push stream described in a vendor note; the V2.5.2 PC
software does not use it. [doc] [static]

| type | name | effect | tag |
|---|---|---|---|
| 0x01 | keep-alive | no reply seen when sent alone | [doc] [device] |
| 0x02 | start auto-upload | `02` alone streams at the meter's rate; `02 <att>` picks content and rate | [doc] [device] (firmware 2.0.6) |
| 0x03 | stop | stops the stream, no reply | [doc] [device] (firmware 2.0.6) |

Frame header (4 bytes):

| byte | field | meaning | tag |
|---|---|---|---|
| 0 | type | bits 0..6 type, bit 7 zero | [doc] [device] |
| 1 | att | request: high nibble content (0 ADC, 1 +PD, 2 +UFCS, 3 +PD+UFCS), low nibble rate; reply record frames: 0x10 OR rate | [doc] [device] (firmware 2.0.6) |
| 2 | obj | body length in 4-byte words; framing by it is exact | [device] (firmware 2.0.6) |
| 3 | crc8 | CRC-8, poly 0x29, init 0, no reflection, xorout 0, over the body only; the device does not check request CRCs | [device] (firmware 2.0.6) |

- Record frames carry the same 20-byte record as the ADC queue (u32 ms time).
  At 50/s each frame holds 1 record, at 1000/s 11 records. [device]
  (firmware 2.0.6)
- The first frame after a bare `02` has att 0x03 and holds section-chained
  ADC + queue data. [device]
- A `0F` sent during the stream got a 4-byte reply of unknown meaning and
  stopped the stream. [device]
- The note says auto-upload stops after one hour or on unplug unless
  refreshed. [doc]

## PDM text session (protocol triggering)

The CDC port also carries the "PDM" text commands that make the meter act
as a sink and ask a charger for a voltage. [doc] [static] [device]
(firmware 2.0.6, session 2)

- Commands are lowercase ASCII, one per write. The vendor tools send no
  terminator; a trailing `\r\n` or extra bytes are tolerated because the
  firmware matches by prefix. Matching is case-sensitive and unknown text
  gets no reply. Keep at least 60 ms between commands. [static] [device]
- Replies are text lines ending in `\n` (no `\r`). Some replies embed a
  binary capability block after a tag (`pdo:`, `ufcs:`, `scp:`, `vfcp:`,
  `tfcp:`), so the stream must be read as bytes. Binary PUT_DATA replies and
  ADCNEW frames can arrive between text lines at any time. [device]
- Session: `pdm open` → `pdm mode entry` (and a version line) and the meter
  presents Rd, so VBUS comes up at 5 V. `reset` → `ok`, VBUS stays at 5 V.
  `pdm close` → `ok`, Rd is removed. A `busy` reply to `pdm open` is
  answered with `reset`. [device]; `busy` [static] [doc]
- The text session is tied to the open port: after the port is closed and
  reopened, nothing is answered until `pdm open` (and `entry <protocol>`)
  is sent again. [device]
- `entry pd` → `ok`, `cc1 attach`, then a `pdo:` block, `ready:<mV>mV,<mA>mA`,
  and, when the meter enters EPR, a second `pdo:` block and `ready:`. The
  blocks arrive unprompted. [device]
- `pdo:` block: tag, one ASCII byte (`'4'` SPR, `'9'` EPR: probably the
  highest position in use; do not depend on it), a slot count (11), that
  many u32 LE PDOs (0 = unused slot), `\n`. `pd req=<slot>` uses the slot
  position. [device]
- `pd req=<slot>` → `req:<mV>mV,<mA>mA` then `ready:<mV>mV,<mA>mA` with VBUS
  and IBUS at PS_RDY. `pd req=<slot>,volt=<mV>,cur=<mA>` requests a PPS or
  AVS level. `pd hrst` sends a Hard Reset and the charger renegotiates from
  5 V. `pdm parm` → `type=<n>,em=<n>,sink=<n>`; the default `em=2` (EPR cable
  emulation) makes the meter enter EPR by itself. [device]
- A protocol the charger lacks answers `ok` then `timeout` (UFCS: three
  `timeout` lines). [device]
- Detection: `entry bc` and `entry apple` answer `true` or `false` [doc];
  on the device `entry bc` answered only `ok` (read as unknown) and
  `entry apple` answered `fail` on a non-Apple charger (not supported).
  `entry list` (and `entry list+`, which adds VOOC and MTK PE and needs a
  load [doc]) prints `starting`, then one `<protocol>:<true|null>
  [<mV>mV,<mA>mA]` line per protocol, and then went silent for 45 s; `reset`
  ends it with `ok`. How a full list ends is not known yet. [device]
- The meter's own probing during `entry list` may raise VBUS on some
  chargers, so nothing that cannot take it belongs on the output. [inferred]
- The `ufcs:`, `scp:`, `vfcp:` and `tfcp:` block layouts come from the vendor
  app only. [static]

How our library sends them safely:

- Every text write passes the same frame-type guard as binary requests (the
  first byte is read as a frame type), so for example a line starting with
  `H` (0x48, SET_DATA) is refused, and so are the CDC ADCNEW types.
- Every command is classified before it is sent: read-only, 5 V or less, or
  above 5 V, using the charger's last known offers. Unknown or unparsable
  text, a request for an offer we have not seen, and a range request below
  the offer's minimum all count as above 5 V. A command above 5 V is sent
  only with an explicit confirmation made for that one command. Offers are
  forgotten when the port is reopened.
- `pdm close` is always sent before the port is released.

## PD capture

- Start: `10 id 02 00` (mode 1 = PD) or modes 2..5 for UFCS; poll
  `0C id 20 00` (PD) or `0C id 80 00` (UFCS) every 5-50 ms; stop with
  `11 id 00 00`. [static]
- [device] On firmware 2.0.6, 0x10 and 0x11 are REJECTed at auth level 0 and
  ACCEPTed at level 1. Polling PD_PACKET works at level 0 and returns the
  12-byte status. Prior art said capture works without auth; device overrides.
- [device] Records arrive without 0x10 too: polling PD_PACKET at level 0
  returned the PD traffic the meter itself took part in (session 2, over
  CDC). A section carries at most about 320 bytes; older records stay
  queued and come with the next polls, so poll fast (every 5 ms) or until a
  poll returns only the status block.
- Capturing a negotiation that passes through the meter (charger on the
  input, sink on the output) with 0x10 started is not yet checked. [inferred]
- An open serial `pdm open` session reportedly holds the PD engine, so bulk
  PD queries get REJECT until `pdm close`. [inferred] (prior art)

Status header (12 bytes, always first in the PD_PACKET and QC_PACKET sections):

| off | type | field | scale | tag |
|---|---|---|---|---|
| 0 | u32 | timestamp | ms | [static] [device] |
| 4 | u16 | VBUS | mV | [static] [device]; unit [inferred] |
| 6 | i16 | IBUS, signed | mA | [static]; unit [inferred] |
| 8 | u16 | CC1 (UFCS: D+) | mV | [static] [device]; unit [inferred] |
| 10 | u16 | CC2 (UFCS: D-) | mV | [static] [device]; unit [inferred] |

Records follow, each starting with a flag / length byte `b0`: [static]

| b0 | record length (incl. b0) | kind | tag |
|---|---|---|---|
| bit 7 set | `(b0 AND 0x3F) + 1` | PD message | [static] [device] |
| bit 7 clear, bit 5 set | `(b0 AND 0x1F) + 1 + u16 at record offset 6` | long (extended) PD message | [static] |
| bit 7 clear, bit 5 clear | `(b0 AND 0x1F) + 1` | event | [static] [device] |

- Bit 6 of `b0` marks a record the meter sent itself: its own requests,
  GoodCRCs and resets, as opposed to traffic from the line. [device]
- Bit 7 wins over bit 5: `0xA3` is a 36-byte PD message, not a long one.
  [device]
- A record whose length runs past the end of the section is skipped and
  counted, and parsing of that section stops there; a record too short for
  its kind is skipped and parsing goes on. No long record (bit 5) has been
  seen on the device; EPR_Source_Capabilities arrived chunked. [device]

Record body:

| off | type | field | tag |
|---|---|---|---|
| +0 | u8 | `b0` | [static] |
| +1 | u32 | timestamp, ms; same clock as the status block. Our library reports it as ms since the capture start, never going backwards when the meter clock restarts | [static] [device] |
| +5 | u8 | PD: SOP type (low 3 bits: 0 SOP, 1 SOP', 2 SOP'', 3 SOP'_Debug, 4 SOP''_Debug); event: code in the low nibble, parameter in the high nibble | [static] |
| +6 | u16 | PD: message header (long messages: extra length here, header at +8) | [static] |
| +8 | u32 x n | PD data objects, decoded per USB PD r3.2 | [static] |

- A normal PD message on the wire is `(b0 AND 0x3F) - 5` bytes; records carry
  no CRC. [static] [inferred] (prior art)
- Event codes: 0 none, 1 attach (parameter 1 = CC1, 2 = CC2), 2 detach,
  3 bad message id, 4 hard reset, 5 cable reset, 6 receive error, 7 monitor
  type, 8 big message. [static]
- Idle QC_PACKET adds 8 constant bytes of unknown meaning after the status.
  [device]

## Never-send commands

Our library refuses to send these requests, so a bug or a CLI typo cannot
brick, decalibrate or reboot a device. Three checks run before any byte
reaches the meter: the transport-layer guard refuses the six frame types, the
session refuses memory reads outside the four known blocks, and the HID
transport sends only the ADC snapshot. [device] [static]

| request | why | tag |
|---|---|---|
| type 0x04 RESET | resets the device; unneeded | [doc] |
| type 0x08 JUMP_APROM | leaves firmware-update mode | [doc] [static] |
| type 0x09 JUMP_DFU | enters firmware-update mode | [doc] [static] |
| type 0x42 | firmware download descriptor (starts an erase) | [static] |
| type 0x48 SET_DATA | writes settings and calibration | [static] |
| type 0x4A | memory / SPI-flash write | [static] |
| memory read (0x44) at any address other than `0x420`, `0x4420`, `0x03000C00`, `0x40010450` | other addresses reportedly crash the meter | [inferred] (prior art) [device] |
| any HID request other than the ADC snapshot (GET_DATA mask 0x001) | replies over 64 bytes hang and reboot the meter; control commands are ignored | [device] (firmware 2.0.6) |

PDM text commands go through the same guard, and a request that could raise
VBUS above 5 V needs a per-command confirmation (see PDM text session).

Separate from the guard, one rule applies on CDC: there SYNC / CONNECT /
DISCONNECT (0x01..0x03) start or stop the ADCNEW stream, so the CDC transport
refuses to send them (ADCNEW is not supported yet). [device] (firmware 2.0.6)

Outside the guard, one USB-level operation is safe and allowed: clearing an
endpoint halt. It is the standard USB control request CLEAR_FEATURE with
selector ENDPOINT_HALT, handled by the USB stack rather than the meter's
command set, and it resets the endpoint's data toggle. [static] We use it on
both bulk endpoints (0x01 OUT, 0x81 IN) only during connect recovery (see
Session flow and errors), when a stale DISCONNECT (`03 xx 00 00`, carrying a
device-side id unrelated to our previous session) may sit in bulk IN at open
[device] and CONNECT gets no ACCEPT within 500 ms. That it clears the stall
without side effects on the meter is [inferred] until confirmed on the device.
