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

Streaming needs the vendor bulk interface (the default `--transport bulk`);
`cdc` and `hid` carry only `read` and `watch`. To try it without a meter, add
`--transport sim --scenario pd-9v-2a`.

## Record or export from the desktop app

The desktop app writes the same CSV layout.

- **Record** asks for a file name, then writes every sample from that moment
  until you press Stop. The sample rate is locked while it runs.
- **Export** saves everything the session history holds so far.

After a recording stops or an export finishes, a notice names the rows written
and the file. "Reveal in Finder" (on Windows and Linux, "Show in folder") opens
the folder with the file selected, and "Copy path" puts its path on the
clipboard. The stop notice counts only the gaps that happened after the first
written row, so a recording that starts mid-stream never reports a gap it does
not contain.

A sample carries the same Unix time in a recording and in an export of the same
session, also after the chart history was cleared.

### Temperature

The TEMP column carries the meter's latest valid temperature reading at or
before each row; it stays empty until the meter has reported one, and empty
again once that reading is more than 2 s old. Recordings, exports and
`powerz stream` fill it the same way.

At 1000 SPS the readout shows the temperature with the tooltip "Updated once a
second at 1000 SPS", because the meter reports it from a separate reading. A
meter whose firmware reports no temperature at that rate shows "—" instead.

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
