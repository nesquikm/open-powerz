<!-- WARNING: This reference was generated without mechanical signature extraction for this stack. Signatures may be imprecise. Review carefully before publishing. -->

# Desktop backend seam

The desktop app's web UI talks to its Rust backend through one TypeScript
module, `apps/desktop/src/lib/backend.ts`. Every function there calls a Tauri
command, or the in-browser `MockBackend` when the UI is built with
`VITE_BACKEND=mock`. Scripts and other front ends can use the same seam. This
page lists the parts added for session gaps, replug, the PD analyzer, the
trigger panel and the firmware update; the source has the rest.

## Session history

| function | answers |
|---|---|
| `historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]>` | The breaks between measuring runs that overlap `t0Ms..=t1Ms`, oldest first. A `HistoryGap` is `{ from_ms, to_ms }`. |

## Replug

| item | meaning |
|---|---|
| `Status` `{ state: "waiting"; serial: string }` | The meter went away on its own; the backend waits for this serial to come back. `ConnectionState` (in `format.ts`) has the matching `waiting` state, shown as "Waiting for meter". |
| `cancelWait(): Promise<void>` | Stops waiting for an unplugged meter, after the user picked another device. |

`Session` (in `session.svelte.ts`) keeps the awaited meter picked while the
backend waits.

## PD capture

Commands:

| function | what it does |
|---|---|
| `pdStart(): Promise<void>` | Starts the PD capture on the open session. Rows and the capture state arrive on the session channel. |
| `pdStop(): Promise<void>` | Stops the capture. The captured records stay. |
| `pdClear(): Promise<number>` | Drops every captured record and answers the index of the next record. Rows numbered below it were sent before the clear and are stale. |
| `pdDetails(index: number, opened = false): Promise<PdDetails>` | The raw bytes and field tree of record `index`: of the capture opened from a file when `opened`, of the live capture otherwise. |
| `pdSave(suggestedName: string): Promise<ExportResult \| null>` | Opens the save dialog and saves the capture so far as a [pdcap file](pdcap.md). Answers `{ path, rows }`, or `null` when the dialog was cancelled. |
| `pdOpen(): Promise<PdOpened \| null>` | Opens the open dialog and loads a pdcap file. Needs no meter. `null` when cancelled. |
| `pdImport(): Promise<PdOpened \| null>` | Opens the open dialog and imports a vendor PD log database (`.sqlite`) read-only. Needs no meter. `null` when cancelled. |

Session channel events (`BackendEvent`):

| `type` | `data` |
|---|---|
| `pd_state` | `{ capturing: boolean }` |
| `pd_rows` | `{ rows: PdRow[] }`, sent in batches (at most 60 a second) |
| `pd_status` | `{ samples: PdStatusSample[] }`, the status blocks for the chart |

Types:

| type | fields |
|---|---|
| `PdRow` | `index` (from 0, in capture order), `elapsed_ms` (since the capture start), `session_ms?` (on the samples' `t_ms` base, for the chart), `kind` (`"message"` or `"event"`), `sop`, `name`, `message_id`, `direction`, `objects`, `spec_revision`, `vbus_v`, `ibus_a`, `summary`. The message fields are `null` on event rows. |
| `PdDirection` | `"source_to_sink"`, `"sink_to_source"` or `"cable"` |
| `PdDetails` | `index`, `raw` (the record's bytes), `fields` (a `PdField` tree) |
| `PdField` | `name`, `meaning`, `raw`, `bytes` (`[start, end)` in the record's raw bytes), `children` |
| `PdStatusSample` | `session_ms`, `vbus_v`, `ibus_a` (signed), `cc1_v`, `cc2_v` |
| `PdOpened` | `path`, `rows`, `status`, `damaged_line` (1-based line where reading stopped, `null` for a whole file), `oversized` (vendor log rows skipped, 0 for pdcap), `truncated` (the vendor log had more rows than the import takes) |

On the UI side, `PdCapture` (in `pdCapture.svelte.ts`) holds the rows and the
status samples for the table and the chart. `THEME_COLORS` and
`themeVariables` (in `theme.ts`) carry the marker colours, one per
`PdMarkerKind`: `negotiation`, `reset`, `error`, `attach_detach`, `other`.

## Trigger panel

The trigger panel drives a PDM text session on the meter's virtual serial
port (`src-tauri/src/trigger.rs`). It runs on a thread of its own next to the
measurement stream, which stays connected on the bulk pipe. Every function
below calls the Tauri command with the same name in snake case (for example
`triggerOpen` calls `trigger_open`).

Commands:

| function | what it does |
|---|---|
| `triggerOpen(): Promise<void>` | Opens the trigger session on the connected meter (a simulated one for a demo device). Fails with `usage` when no meter is connected or a session is already open. |
| `triggerDetect(): Promise<Detection>` | BC1.2, Apple and the automatic list. Nothing above 5 V is asked. The stream's polling pauses while it runs (see below). |
| `triggerList(protocol: TriggerProtocol): Promise<ProtocolSection>` | Enters `protocol` and reports its section. A protocol the charger does not answer is `supported: false`, not an error. |
| `triggerRisk(request: TriggerRequest): Promise<RiskClass>` | The library's risk class of `request`, given what the charger offered. `above_5v` needs the user's confirmation. |
| `triggerRequest(request: TriggerRequest, confirmed: boolean): Promise<TriggerReply>` | Sends `request`. The backend is the final gate: above 5 V it is refused unless `confirmed`. A request for another protocol than the one entered enters that protocol again first. |
| `triggerMessageRisk(message: PdMessageRequest): Promise<RiskClass>` | The risk class of a built PD message. GoodCRC and malformed messages fail with `usage` and the reason. |
| `triggerMessageSend(message: PdMessageRequest, confirmed: boolean): Promise<PdSendReply>` | Sends a built PD message; above 5 V only when `confirmed`. |
| `triggerConsoleRisk(line: string): Promise<RiskClass>` | The risk class of a raw console line. |
| `triggerConsoleSend(line: string, confirmed: boolean): Promise<ConsoleLine[]>` | Sends a raw console line and answers the transcript lines. Above 5 V (any unparsable line too) only when `confirmed`; GoodCRC is always refused. |
| `triggerBackTo5v(): Promise<void>` | Back to 5 V: PD sends `pd req=1` (a Hard Reset as the fallback), the other protocols `reset`. The session stays open. |
| `triggerClose(): Promise<void>` | Back to 5 V where the protocol allows it, then `pdm close`. Does nothing when no session is open. |

Types (they mirror `src-tauri/src/trigger.rs`):

| type | fields |
|---|---|
| `TriggerProtocol` | `"pd"`, `"qc"`, `"fcp"`, `"afc"`, `"sfcp"`, `"scp"`, `"vfcp"`, `"tfcp"`, `"ufcs"`. `TRIGGER_PROTOCOLS` lists them, PD first. |
| `Detection` | `bc12`, `apple` (each a `SupportName`: `"supported"`, `"not-supported"` or `"unknown"`), `entries` (`DetectEntry[]`: `name`, `support`, `vbus_mv`, `ibus_ma`), `complete` (false when the list stalled or was cut off) |
| `ProtocolSection` | `protocol`, `supported`, `offers` (`TriggerOffer[]`, PD only), `limits` (`TriggerLimits[]`), `settings` (`PdEmulation`: `pd_type`, `emarker`, `sink`; PD only, else `null`) |
| `TriggerOffer` | `position`, `kind` (`"fixed"`, `"battery"`, `"variable"`, `"pps"`, `"spr-avs"`, `"epr-avs"`), `min_voltage_mv`, `max_voltage_mv`, `max_current_ma`, `max_power_mw`, `epr` |
| `TriggerLimits` | `index` (the UFCS capability number, else `null`), `min_voltage_mv`, `max_voltage_mv`, `min_current_ma`, `max_current_ma`, `max_power_mw` |
| `TriggerRequest` | One of `{ protocol: "pd"; position; voltage_mv; current_ma }` (the last two `null` for a fixed offer), `{ protocol: "qc" \| "fcp" \| "afc" \| "sfcp"; volts }`, `{ protocol: "qc3"; voltage_mv }`, `{ protocol: "scp" \| "vfcp" \| "tfcp"; voltage_mv; current_ma }`, `{ protocol: "ufcs"; index; voltage_mv; current_ma }` |
| `RiskClass` | `"read_only"`, `"at_most_5v"` or `"above_5v"` |
| `TriggerReply` | `command`, `requested_mv`, `requested_ma`, `delivered_mv`, `delivered_ma`, `reply` |
| `PdMessageRequest` | `sop` (`"sop"`, `"sop_prime"` or `"sop_double_prime"`), `msg_type` (the 5-bit Message Type), `data_objects` (32-bit numbers; none means a control message) |
| `ConsoleLine` | `at_ms` (clock time, ms since the Unix epoch), `direction` (`"tx"` or `"rx"`), `text`, `block` (a parsed binary block in words, or `null`) |
| `PdSendReply` | `command`, `lines` (`ConsoleLine[]`), `cable` (`CableView` or `null`) |
| `CableView` | `vid`, `pid`, `active`, `max_voltage_v`, `max_current_ma`, `epr_capable` |

Behaviour:

- **Polling pauses during the automatic list.** While `triggerDetect` runs
  the list, the measurement stream stops polling on its open bulk session and
  resumes however the list ends, on error too. It does not reconnect: a bulk
  CONNECT silences the PDM session. A queue reply that arrives shorter than
  it declares after the pause keeps its whole records, and the lost samples
  show as a gap.
- **Other limits while a session is open.** The sample rate is held at
  50 SPS or less and restored when the session closes. Opening a session
  stops the PD capture, and `pdStart` fails with `usage` until the session is
  closed.
- **Back to 5 V on every way out.** Disconnecting the meter, closing the
  window or `triggerClose` returns the charger to 5 V where the protocol
  allows it and sends `pdm close`. The UI calls `triggerClose` when the user
  leaves the Trigger tab.
- **`out_of_sync` errors.** `ErrorKind` has an `out_of_sync` kind. The
  backend uses it when the meter answers a command with a reply that belongs
  to another command. The message says no more requests are sent; the hint
  says to power-cycle the meter (unplug it and plug it back in) and open the
  session again. From then on the session refuses every command and writes
  nothing. The panel disables requests; **Detect** closes the session and
  opens a new one.

On the UI side, `TriggerPanel` (in `trigger.svelte.ts`) holds the view's
state: detection, the protocol sections, the pending confirmation, the last
reply and the live VBUS check. `PdConsole` (in `console.svelte.ts`) holds the
message builder and the console transcript.

## Firmware

Device mode and firmware version:

| item | meaning |
|---|---|
| `DeviceMode` | `"application"`, `"bootloader"` or `"unknown"`, from the USB string of the meter's vendor interface (`APP` in application mode). |
| `DeviceListing.mode` | A meter in bootloader mode is listed; `deviceOptionLabel` ends its picker label in "(bootloader mode)". `Session` will not connect to it, and the backend refuses that connect too. |
| `ConnectedInfo` | Adds `mode`, `bootloader_version`, `warnings`, `update_required` (`boolean \| null`: `null` when the HEAD reply could not be read) and `firmware_problems` (one line per part of the firmware status that could not be read, shown as "Firmware status" rows). |
| `FirmwareWarningCode` | `"too_old"` (below 1.7.0), `"outdated"` (below 2.0.7, the newest version known) and `"update_required"` (the HEAD reply says so). |

Update commands:

| function | what it does |
|---|---|
| `firmwareInspect(path: string): Promise<FirmwareFileInfo>` | Reads a local firmware container and answers its metadata; needs no meter. The file is never decrypted; files over 8 MiB are refused before they are read. A broken file rejects with `usage`, prefixed with the file name. |
| `firmwareChoose(): Promise<string \| null>` | Opens the open dialog filtered to `.mencrypt` files; `null` when it was cancelled. |
| `firmwareFlash(deviceId: string, path: string, confirmation: string, onStage: (event: FlashEvent) => void): Promise<FlashDone>` | Updates the meter with the file at `path`. Before anything is sent the backend checks that `confirmation` is the file's version and reads the file again. Each event goes to `onStage` over a channel, about one write message per percent. Rejects with the error and its recovery hint. |

Types:

| type | fields |
|---|---|
| `FirmwareFileInfo` | `vendor`, `container_version`, `model`, `version`, `date`, `crc` (8 uppercase hex digits), `size` (the declared image size in bytes), `release_notes_en` |
| `FlashStage` | `{ stage: "switching" }`, `"erasing"`, `{ stage: "writing"; done; total }` (bytes), `"verifying"`, `"finished"`, `"returning"` |
| `FlashReplug` | `{ stage: "replug"; to: "bootloader" \| "application"; reply }`: a jump was sent; `reply` is the meter's answer to it. The screen asks for a replug if the meter does not restart by itself. It does not change the stage. |
| `FlashEvent` | `FlashStage \| FlashReplug` |
| `FlashDone` | `{ outcome: "verified"; version }` (read back from the meter) or `{ outcome: "power_cycle_needed" }` (the image was accepted but the meter stays in its bootloader until it is replugged) |

Behaviour:

- **The meter is held for the whole update.** Connecting, PD capture,
  recording, the rate, the trigger panel and hotplug reconnects are refused
  until it ends, and closing the window or quitting is refused while it runs.
  `Session.flashing` is set meanwhile, which disables the device picker and
  Connect.
- **The Firmware tab is opt-in.** It appears only when **Advanced firmware**
  is switched on in About. The switch is stored in the browser, off by
  default, and cannot be turned off while an update runs.

## Mock backend

`MockBackend` (in `mockBackend.ts`) plays a demo session in the browser, with
no Rust backend. It plays a looping PD capture. Page URL parameters change what
it does:

| parameter | effect |
|---|---|
| `?mock=replug` | One meter is listed next to the demo devices. A session on it is unplugged once and plugged back in; `MOCK_REPLUG` sets the timings. |
| `?mock=trigger-sag` | A trigger request delivers 12 % less than asked, so the panel flags the live VBUS. |
| `?mock=trigger-all` | The demo charger speaks every protocol, with decoded limits for SCP, VFCP, TFCP and UFCS. |
| `?mock=trigger-out-of-sync` | Every trigger request fails with an `out_of_sync` error. |
| `?mock=trigger-req-error` | The charger refuses every request above 5 V (`req error 252`); the session stays usable. |
| `?mock=trigger-detect-disagree` | BC1.2 says not supported while the automatic list says supported. |
| `?mock=trigger-list-lag` | Detection fails with an `out_of_sync` error after the list, as when the meter answers one command late. |
| `?mock=bootloader` | One meter in bootloader mode is listed before the demo devices. |
| `?mock=firmware-fail-<stage>` | A firmware update fails at `switching`, `erasing`, `writing`, `verifying` or `returning`, with the recovery steps. |
| `?mock=firmware-other-model` | The chosen firmware file is for a KM002C. |
| `?mock=firmware-bad-file` | The chosen firmware file fails inspection. |
| `?pdsynthetic=<n>` | Seeds `n` PD records when the capture first starts, for performance checks. `pdSyntheticFromUrl` reads it into `MockOptions.pdSynthetic`. |
