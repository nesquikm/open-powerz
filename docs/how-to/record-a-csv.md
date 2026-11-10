# Record a high-rate CSV

Use `powerz stream` to record the meter's ADC sample stream to a CSV file that
opens in a spreadsheet and matches the vendor recorder's column layout.

1. Pick a rate: `2`, `10`, `50` or `1000` samples per second.
2. Record, either for a fixed time or until Ctrl-C:

   ```bash
   powerz stream --rate 1000 --csv run.csv --duration 60
   powerz stream --rate 50 --csv run.csv          # stop with Ctrl-C
   ```

3. Read the summary on stderr: the sample count, then one line per gap if
   any samples were missed.

The file starts with the header
`ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR`. Time is
counted from the first sample. Numbers are plain decimals with at most 8
significant digits; CHARGE (Ah) and ENERGY (Wh) are integrated from the signed
current, so charging in the reverse direction counts down.

Streaming the ADC queue needs the vendor bulk interface (the default
`--transport bulk`). To try it without a meter, add
`--transport sim --scenario pd-9v-2a`.

## Record the serial-port stream instead

The meter can also push its samples on its virtual serial port (the ADCNEW
stream). It needs no sign-in, at 50 or 1000 samples per second:

```bash
powerz stream --source cdc --rate 1000 --csv run.csv --duration 60
```

The CSV has the same columns, with `TEMP` empty (this stream carries no
temperature). The summary line reads
`<n> records, <g> gaps, <d> dropped frames, <c> CRC errors`. Right after a bulk
session the meter may echo the start for a few seconds; the CLI says so and
starts again after 12 s. In the desktop app, pick **Serial stream** in the
Measurement source picker before connecting.

## Record or export from the desktop app

The desktop app writes the same CSV layout.

- **Record** starts at once, on a new file in your Downloads folder under
  `open-powerz-recordings` (the app's data folder if there is no Downloads
  folder), and writes every sample until you press Stop. Stop then asks where
  to keep the file; cancel and it stays where it is. The sample rate is locked
  while it runs.
- **Export** saves everything the session history holds so far.

After a recording stops or an export finishes, a notice names the rows written
and the file. "Reveal in Finder" (on Windows and Linux, "Show in folder") opens
the folder with the file selected, and "Copy path" puts its path on the
clipboard. The stop notice counts only the gaps that happened after the first
written row, so a recording that starts mid-stream never reports a gap it does
not contain.

A sample carries the same Unix time in a recording and in an export of the same
session, also after the chart history was cleared.

### Pause a recording

While a recording runs, **Pause** stops writing rows. The meter and the chart go
on, and the indicator reads PAUSED. **Resume** continues in the same file. The
time in between counts as one gap in the stop notice, and charge and energy in
the file do not count it. Each meter tile has its own Pause button.

### Recorder options

**Options**, next to Record, opens the recorder options. They are saved, hold
for the next recording, and apply to the main meter and to every tile. They lock
while any meter records.

- **File name prefix** replaces `powerz` in the file name, so `bench` gives
  `bench-20261102-143000.csv`. It cannot hold a path separator or any of
  `: * ? " < > |`.
- **Folder** sets where new files go. With a folder set, Stop asks nothing and
  the file stays there. A folder that is missing or cannot be written to is
  refused when you press Record, and no recording starts. With no folder, files
  go where **Record** says above and Stop asks.
- **Auto-stop** ends a recording when the current stays below a set value for a
  set time (the defaults are 0.05 A for 60 s). Both must be above 0. It keeps
  the file, and the notice opens with "Recording ended on low current". Paused
  time does not count toward it, and a slow window cannot delay it.
- **Row limit** ends a recording at that many rows (default 10,000,000). It
  keeps the file, and the notice opens with "Recording ended at the row limit".

The dialog also shows how long the chart's memory lasts at the current rate:
about 10 minutes at 1000 SPS, longer at lower rates. This limits the chart, not
a recording.

`powerz stream` has none of these options: it has no pause, auto-stop or row
limit, and it writes to the file you name.

In the web edition a recording, an export and a saved file go to the
browser's downloads: there is no Folder option and no reveal in the file
manager. The file is the same, TEMP included.

### Temperature

The TEMP column carries the meter's latest valid temperature reading at or
before each row; it stays empty until the meter has reported one, and empty
again once that reading is more than 2 s old. Recordings, exports and
`powerz stream` fill it the same way.

At 1000 SPS the readout shows the temperature with the tooltip "Updated once a
second at 1000 SPS", because the meter reports it from a separate reading. A
meter whose firmware reports no temperature at that rate shows "—" instead.

### Long sessions

The chart history keeps the newest 10 million samples at full resolution, about
2 h 45 min at 1000 samples/s. Older data folds into blocks that keep each
series' minimum, maximum and average and the block's charge and energy, so the
statistics, the ruler, segment marks and E.D. stay exact at block edges. A faint
"coarser before here" tick marks where the folded part ends on the chart.

An export of a folded session holds only the full-resolution part, and its
notice says how much of the oldest part was left out and the time the file
starts at. To keep every sample of a long session, use **Record**.

### Reset the statistics or clear the chart history

Two buttons in the Stats panel erase different things:

- **Reset stats** zeroes the statistics (min, max, average, charge, energy).
  The chart history stays.
- **Clear chart history…** erases the chart history and what Export saves,
  after you confirm. The chart, "Whole session" and the overview strip then
  start again from the moment of the clear.

The meter stays connected and keeps measuring through either one, and a
recording in progress keeps every row.

### If the meter stops answering

If the app was closed while streaming, the meter can be left mid-transfer. The
app clears the stuck USB pipe and retries the connection by itself; you only
need to replug when it asks you to.

## Open a recording

**Open recording**, in the tab bar of the desktop app and the web edition, reads a
CSV or a vendor `.db` file back into the app. It needs no meter. The file opens in a **File** tab,
marked as a file and read-only. The tab exists only while a file is open, and
**Close file** ends it. One file opens at a time; opening another replaces it.

Opening a file sends nothing to any meter. A running stream or recording keeps
going behind the tab.

A CSV the app recorded opens with the same values and statistics. A CSV from
`powerz records get` opens too, with D+, D−, CC1 and CC2 at 0, because that file
leaves them empty.

- **Damaged rows** are skipped, and the view says how many. A row is damaged
  when it starts `invalid,,`, is short or is not text, or has a required field
  empty or unreadable. A time that goes backwards or a value out of range counts
  too. TEMP, CHARGE, ENERGY and PWR may be empty.
- **Refused files** do not open, and the app says why. That means a wrong
  header, a binary file, an empty file, a file with no readable row, or a
  database of another shape.
- **Large files** open with progress, and **Cancel** stops them. The row limit
  is 10,000,000: a longer file opens with its first 10,000,000 rows, and the
  view says how many were left out.

Min, max and average come from the rows. The view takes charge and energy from
the CHARGE and ENERGY columns, at their last values, because a recording with a
pause does not count that time and the rows alone cannot show it. A file without
those columns has them integrated from its rows.

Drag across the chart to zoom in, and press **Whole file** to go back.

### Export a vendor database

**Export .db** saves everything the session history holds so far to a `.db` file
in the vendor's table layout. It holds no serial number. The app writes to a
temporary name and renames at the end, so a failed export leaves an older file
alone. Open the file here and it reads back with equal values.

Both directions follow the vendor's table layout as far as we know it. This is
unproven against the vendor app: no real vendor `.db` file was tested, neither
one opened here nor one of ours opened there.
