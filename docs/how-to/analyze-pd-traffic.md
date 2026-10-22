# Analyze USB PD traffic

The **PD analyzer** tab of the desktop app captures the USB Power Delivery
messages on the cable that passes through the meter. The capture is passive:
the meter keeps passing power and nothing is triggered.

This page walks through a capture in task order: record it, read it, line it
up with the power chart, then save it or open an older one.

## Capture a negotiation

1. Connect the meter, or pick a demo device. A demo device plays a scripted
   negotiation.
2. Open the **PD analyzer** tab and press **Start capture**. While no meter is
   connected, Start and Stop are disabled and the reason is shown.
3. Plug the charger into the meter's input and the phone or laptop into its
   output.
4. Watch the table fill. Each message and event is one row: index, time since
   the capture started, SOP, message name, message ID, direction, object
   count, spec revision, VBUS and IBUS at that moment, and a one-line summary.
   Events (attach, detach, resets) look different from messages.
5. Press **Stop capture** when you are done.

While capturing, the table follows new rows. Scroll up to stop following;
**Follow** jumps back to the newest row. **Clear capture** empties the table
after you confirm.

## Narrow the table

- GoodCRC replies are hidden by default. Tick **Show GoodCRC** to see them.
- **Message type** and **SOP** narrow the table further.

Filters only hide rows. Nothing captured is dropped.

## Read a message

Select a row: click it, or use the arrow keys, Page Up/Down, Home and End. The
**Packet details** pane shows every decoded field with its bit range and
meaning, and the raw bytes in hex. Selecting a field highlights its bytes.

## Line it up with the power chart

While a PD capture runs, every message and event also shows on the **Meter**
tab's chart, in a lane above the plot, at the moment it happened.

- Colours tell the kind apart: negotiation, reset, error, attach / detach and
  other. A legend names them.
- Markers too close to tell apart merge into a badge with a count. Zoom in to
  see them one by one.
- Click a marker (or a badge) to open the PD analyzer with that message
  selected.
- Select a row in the packet table to move the chart's playhead to its moment.
  If the moment is already in view, the zoom stays as it is; otherwise the
  chart centres on it.
- The VBUS, IBUS, CC1 and CC2 values the meter reports with each PD packet are
  drawn as dots on the matching series. The CC1 and CC2 buttons hide and show
  them like the other series.

## Save a capture

Press **Save capture** to write the current capture to a `.pdcap` file. This is
an open, documented format; see the [pdcap reference](../reference/pdcap.md).
The meter's serial number is never written.

A notice names the file, with buttons to show it in the file manager or copy
its path.

## Open a saved capture or a vendor log

Stop a running capture first. Then:

- **Open capture** loads a `.pdcap` file back into the table, the details and
  the chart, exactly as saved. No meter is needed. If the file is damaged,
  every line before the damage loads and a notice names the line where reading
  stopped.
- **Import vendor log** opens a PD log database saved by the vendor's Windows
  app (`.sqlite`), read-only, and shows its messages. Rows that cannot be read
  are skipped and counted in the notice.

**Clear capture** or **Start capture** returns to the live view.

## From the command line

The `powerz` CLI captures and reads the same files:

```bash
powerz pd --save capture.pdcap     # print the capture and save it
powerz pd decode capture.pdcap     # print a saved file; no meter needed
```

See the [CLI reference](../reference/cli.md) for the output formats.
