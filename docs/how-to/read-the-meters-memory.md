# Read the meter's recordings and settings

The meter can record on its own, without a computer, and keeps its settings
in a small block of memory. open-powerz reads both. Neither the CLI nor the
desktop app writes anything to the meter here: no record is deleted and no
setting is changed.

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

## Notes

- Both features need the vendor bulk interface (the default transport).
- Downloads read the record's own window of the meter's memory, at most 256
  bytes per read, and nothing outside it. See [docs/protocol.md](../protocol.md)
  for the formats and their confidence tags.
