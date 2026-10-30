# Read the meter's recordings and settings

The meter can record on its own, without a computer, and keeps its settings
in a small block of memory. open-powerz reads both, and can delete recordings
and change the display and sample-rate settings. Reading never writes
anything to the meter; the two writes each ask for a confirmation first.

## Download recordings from the CLI

1. Make a recording on the meter (Settings ▸ Storage), then plug the meter
   into the computer's data port.
2. List what it holds:

   ```bash
   powerz records list
   ```

   One row per record: number, name, sample interval, points, duration,
   charge (Ah), energy (Wh) and offset. Charge and energy are shown as
   magnitudes, as the meter's screen shows them.

3. Download one record, or every record with samples, as CSV:

   ```bash
   powerz records get 2 --out ~/recordings
   powerz records get --all --out ~/recordings
   ```

   Each record becomes `NN_<name>.csv` in the `stream` CSV layout, so it opens
   in the same spreadsheets as a live recording. `ElapsedTime` steps by the
   record's interval.

Add `--json` to either command for machine-readable output. To try it without
a meter, add `--transport sim`: the simulator holds three sample records.

## Download recordings in the desktop app

1. Connect the meter and open the **Records** tab. The list loads on
   connect; **Refresh** reads it again.
2. Press **Download** on one row, or **Download all**. Progress shows as
   "Downloading 2 of 3…".
3. Press **Open** on a downloaded row to see it in the history chart with
   its statistics (min, max and average per channel). **Back to records**
   returns to the list.

## Delete recordings

A delete cannot be undone, so open-powerz backs the record up first, as the
same CSV a download writes, and deletes nothing when the backup fails.

From the CLI:

```bash
powerz records delete 2 --i-understand            # asks you to type the record's name
powerz records delete --all --i-understand --out backups   # asks you to type "all"
```

Without `--i-understand` nothing happens. The prompt wants the record name
exactly as `records list` shows it (or `all`); anything else deletes nothing.
`--yes` skips the prompt (it still needs `--i-understand`). The backup goes to
`--out` (the current folder by default) as `NN_<name>.csv`; the command then
rewrites the meter's record list, reads it again and prints how many records
are left. If that read does not match, it stops with exit code 5, names the
backup folder and does not try again.

In the desktop app, press **Delete** on a row in the **Records** tab, or
**Delete all**. The dialog names the record and asks you to type its name (or
`all`) before **Delete** is enabled. The backup goes to
`open-powerz-backups/records-<time>` in your Downloads folder. After a failed
delete the list is read again, so it shows what is really on the meter, and
the error names the backup folder.

## Read the settings

From the CLI:

```bash
powerz settings          # one line per field, tagged [confirmed] or [inferred]
powerz settings --json   # the same as JSON
powerz settings --raw    # hex dump of the 180-byte block
```

In the desktop app, the **Meter settings** panel under Stats shows the same
fields once a meter streams; **Refresh** reads them again. A field tagged
*confirmed* was checked against a meter's screen; *inferred* means the
mapping comes from analysis and may be wrong. The calibration values are shown
as raw numbers.

## Change the settings

You can change the language, brightness, dim timeout, theme, rotation and
sample rate. Calibration, auto-rotate and the startup logo are never written.

From the CLI, list the changes as `field=value`; fields you leave out keep
their values:

```bash
powerz settings set brightness=80 theme=pink --i-understand
```

Values: `language=chinese|english`, `brightness=1..100`,
`dim_timeout=off|15|30|60`, `theme=blue|green|pink|orange|yellow|dark-green|cyan`,
`rotation=0|90|180|270`, `sample_rate=2|10|50|1000`. A value out of range is
refused with exit code 2 before anything is sent. The command sends one
settings write over the meter's virtual serial port (the meter ignores it on
the bulk interface), reads the settings back and prints them. If the meter
reads back something else, it stops with exit code 5 and names the field;
nothing is retried.

In the desktop app, press **Edit** on the **Meter settings** panel. Only the
fields above get controls, and a summary lists each change ("Brightness: 50%
→ 80%") before **Apply** sends it; **Cancel** leaves the meter alone. The
panel then shows the settings as read back.

The meter saves the change at once and its Settings menu shows the new value,
but its live screen redraws a new brightness or theme only after you open
that setting in the meter's own menu. Both the CLI and the app say so in one
line after a write.

## Notes

- Reading and deleting recordings and reading the settings use the vendor
  bulk interface (the default transport); the settings write goes over the
  meter's virtual serial port, which open-powerz opens for it on its own.
- Downloads read the record's own window of the meter's memory, at most 256
  bytes per read, and nothing outside it. See [docs/protocol.md](../protocol.md)
  for the formats and their confidence tags.
