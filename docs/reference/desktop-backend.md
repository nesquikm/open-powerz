<!-- WARNING: This reference was generated without mechanical signature extraction for this stack. Signatures may be imprecise. Review carefully before publishing. -->

# Desktop backend seam

The desktop app's web UI talks to its Rust backend through one TypeScript
module, `apps/desktop/src/lib/backend.ts`. Every function there calls a Tauri
command, or the in-browser `MockBackend` when the UI is built with
`VITE_BACKEND=mock`. Scripts and other front ends can use the same seam. This
page lists the parts added for session gaps, replug and the PD analyzer; the
source has the rest.

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

## Mock backend

`MockBackend` (in `mockBackend.ts`) plays a demo session in the browser, with
no Rust backend. It plays a looping PD capture. Page URL parameters change what
it does:

| parameter | effect |
|---|---|
| `?mock=replug` | One meter is listed next to the demo devices. A session on it is unplugged once and plugged back in; `MOCK_REPLUG` sets the timings. |
| `?pdsynthetic=<n>` | Seeds `n` PD records when the capture first starts, for performance checks. `pdSyntheticFromUrl` reads it into `MockOptions.pdSynthetic`. |
