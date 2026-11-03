# Explore a session in the desktop app

The desktop app keeps every sample of the current session, so you can pause,
zoom and scroll back through it while the meter keeps measuring. Pick a meter
(or the demo device) in the device picker, press Connect, and the chart starts
following the newest data.

## Pause the chart

While the chart follows the newest data, its Live button reads "Live" and is
pressed. Click it to pause the chart on what it shows: the stream and any
recording keep running. Zooming or panning the chart pauses it too.

While paused, the button reads "Paused · Go live". Click it, or press End, to
follow the newest data again.

## Move around a long session

The chart works like a video editor's timeline.

- **Overview strip:** under the chart, it shows the whole session. Drag its
  window to move, drag an edge to change the range, or click elsewhere on the
  strip to jump there.
- **Mouse:** the wheel zooms around the pointer; Shift+wheel or a sideways
  trackpad scroll pans. Click to place the playhead and drag to scrub it;
  middle-drag pans, Alt/Option-drag zooms to a range, and a double-click shows
  the whole session.
- **Keys:** ← and → move the playhead (Shift: a tenth of the view). J, K and L
  play backwards, stop and play forwards, faster on each press. + and − zoom,
  Home jumps to the start, End returns to live, and Shift+Z shows the whole
  session.
- **Zoom slider and presets:** the slider sets the span from 10 ms to the whole
  session; 10 s, 1 min, 10 min and Whole session set it too and keep a paused
  chart paused. The chart remembers the preset you picked and restores it the
  next time the app loads.

Away from the plot, the readout shows every series at the playhead. The chart
never zooms narrower than two samples at the current rate, and its lines run to
the edges of the view, so a deep zoom always shows the trace.

## Read the whole session

The time axis counts from the start of the session, not from the meter's own
clock. Short ranges read in seconds, longer ones in minutes and seconds (m:ss),
and sessions over an hour in hours, minutes and seconds. The hover readout and
the screen-reader announcements use the same time.

At every zoom each pixel column shows the lowest and highest value it covers,
so a single short spike stays visible in "Whole session". The hover readout
shows a single value when the lowest and highest values in a stretch round to
the same number. Power is stored with the session history, so a paused, zoomed
or whole-session view draws the Power trace too, with the true highest and
lowest power in each stretch.

The overview strip draws slow stretches (10 or 50 samples per second over a
long session) as continuous lines.

## Gaps

When the meter is unplugged, disconnected or restarts its clock, the session
goes on and the chart shows the stretch without data as a shaded band labelled
"No data · <length>" (the label appears when the band is wide enough). No line
is drawn across it, in any view, including "Whole session" after a reconnect.
The overview strip shades the same stretches, with their length when the label
fits. Gaps longer than a minute shrink to a narrow labelled break, so the data
on both sides stays readable.

After a disconnect the chart keeps showing the last session, and the message
"Meter disconnected, showing the last session" sits in the line above the plot,
where the hover readout appears while you point at the chart. With no meter
connected at all, the chart says "Connect a meter to start".

## When the meter is unplugged and plugged back in

If the meter's cable comes out while a session is streaming, the app keeps that
meter picked and the top bar reads **Waiting for meter**. It never switches to a
demo device on its own.

Plug the meter back in and the app reconnects to the same meter (matched by its
serial number) within about 3 seconds, at the sample rate you had chosen. The
session carries on along the same timeline, and the time the cable was out shows
as a **No data** band in the live chart, in Whole session and in CSV exports.

- A different meter plugged in meanwhile is listed but not connected.
- Picking another device in the picker (a demo device, or another meter) stops the
  wait; the unplugged meter is then only listed when it comes back.
- A recording that was running at the unplug ends at that moment, its file is
  complete, and a notice says the meter was unplugged. Start a new recording after
  the reconnect if you need one.

## Axes

With zero-based axes off, each value axis spans at least 0.1 V, 0.02 A or
0.1 W. Noise around an idle output no longer fills the whole axis; real swings
still autoscale. The overview strip uses the same minimum span.

## Chart tools

**Chart options** above the chart opens the chart's extra tools. Each one is
off until you turn it on. The app remembers your choices, so they are kept
after a restart. An opened recording has every tool too: the ruler, segment
marks, E.D., the sub-chart and Reverse current. It shares the choices with the
live chart, so turning a tool on or off in one turns it on or off in the other.

### Ruler

**Ruler** puts two cursors, T1 and T2, on the chart. Drag either one with the
mouse by its line, or focus it with Tab and use the keys:

- the arrow keys move it one pixel's worth; hold Shift to move a tenth of the
  view;
- Page Up and Page Down move it a tenth of the view;
- Home and End take it to the start and the end of the session (or the file).

An arrow key keeps a cursor inside the range on screen. When a cursor is
outside it (the live chart scrolls on, or you zoomed or panned), it shows at
the edge of the plot with its name and time, and you can still focus it.
**Reset ruler** puts both cursors back in the range on screen.

Between the cursors the ruler shows the delta time, the delta charge in Ah and
the delta energy in Wh, plus the averages:
average voltage in V, current in A and power in W. It reads the raw samples, not the points drawn on screen, so a
zoomed-out view gives the same numbers. While a new answer is on its way the
values show "…". A screen reader announces the values once they stop changing.

### Segment marks

**Segment marks** draws a dashed line each time the current run reaches the
next step of its total charge or energy, labelled with the percentage. Pick
charge or energy, then the **Mark step**: 5%, 10% (the default), 20% or 25%.
The marks follow the current run only, that is the samples since the last
break in the stream. While the run goes on its total grows, so the marks move.
When the run has no charge (or energy) yet, the chart says so instead of
drawing marks.

### E.D.

**E.D.** draws a dashed line over the chart: the energy share, from 0% at the
bottom of the plot to 100% at the top. Unlike the marks it is a share of the
whole session's energy, summed over its runs; nothing counts across a break.
The text under the options says what it is, in the same words as here:

E.D. is our own definition, not the vendor's number: the energy measured from
the start of the session up to each moment, as a percentage of the whole
session's energy.

While a session is still running, "the whole session" means everything
measured so far, so the line ends at 100% at the newest sample.

### Sub-chart

**Sub-chart** adds a second, smaller plot under the main one.
It has its own series and its own legend.
It shares the main plot's time range, cursor and zoom,
so both always show the same moment at the same place across the window.

### Reverse current

**Reverse current** draws the current axis upside down.
Reverse current changes no data: the readings, the statistics and every export keep their sign.

## Chart style and Save image

**Chart options** also sets how the chart looks.

- **VBUS range** and **IBUS range** pick each axis's range. **Auto** follows
  the data. **Auto from zero** follows the data but always starts at 0. A
  fixed range, such as 0–6, 0–12 or 0–24 V for VBUS and 0–2, 0–5 or 0–8 A for
  IBUS, stays put whatever the data does.
- **Series style** sets each series' colour, line width and transparency.
  Each line keeps its own dash pattern, so the lines stay apart without
  colour. **Reset** puts a series back on its theme look. A colour that is
  hard to see on the chart background gets a low-contrast warning, but you
  can still use it.
- **Chart title** shows a title above the chart and in saved images. Leave it
  empty for none.

These settings are kept for the app, not per meter: they survive a restart
and apply to whichever meter you connect.

**Save image** next to **Chart options** saves the current view as a PNG, SVG
or PDF file. Pick the width and height, each from 320 to 7680 px, and the
format. Draw on the theme's background, or tick **White background** for
white. The file is named `powerz-chart-YYYYMMDD-HHMMSS` with the format's
extension. No serial number or UID goes into the image or its file name.

The image holds the series, axes, title and legend of the current view, on
the same axis ranges as the chart (including **Zero-based axes** and
**Reverse current**). It leaves out the chart's overlays: PD markers, segment
marks, the E.D. line, the ruler and the playhead.

## Sample rate during a recording

While a recording runs, the sample rate is fixed: the rate picker is locked,
and the number keys 1 to 4 show "Stop the recording to change the rate."
instead of changing it.

## Keyboard and screen readers

Press ? for the list of shortcuts. Esc closes whatever is open: the menu, a
dialog or the clear-history confirmation, one at a time.

In a narrow window, theme and About live behind the ⋯ button. Open it with
Enter or Space, move with the arrow keys, Home and End, and pick with Enter or
Space; Tab or a click elsewhere closes it. Closing About puts the keyboard focus
back where you opened it.

The chart's status announces every view change for screen readers, for example
"Paused: 1:20 to 1:30" or "Live: 1 min". It writes as many decimals as the
visible span needs: one from 1 s up, two from 100 ms, three below that, so a
40 ms view reads "Paused: 11.200 s to 11.240 s".

## Hide serial numbers

Turn this on before a screenshot or a screen share. Open **About** and turn on
**Hide serial numbers**. It is off by default and stays set after a restart.

With it on, the app shows no serial number or UID anywhere. Those fields read
"Hidden". Meter tiles are titled by model and position, for example
"KM003C 2", so two meters stay easy to tell apart. Turn it off and the values
come back with no reconnect.

The CLI has a matching flag: see
[Hide serial numbers](../reference/cli.md#hide-serial-numbers) in the CLI
reference.

Error messages never name the serial port, with the option on or off, because
on macOS that name contains the meter's serial. They say "the meter's serial
port" instead.

## Info popovers for sections and terms

Each section title has a small (i) button beside it. Press it, or focus it and
press Enter or Space, for a short note on what the section does. The firmware,
trigger, console and settings notes also say what can go wrong there.

Abbreviations such as VBUS, CC1, PPS or EPR have a dotted underline. Press one
for the same kind of note on what the term means.

Only one note is open at a time. Escape, a click outside it or a second press
of its button closes it and puts the focus back on the button. Escape closes
only the note, not the panel behind it.

To hide the (i) buttons, open **About** and turn off **Show (i) icons**, beside
Hide serial numbers. It is on by default and stays set after a restart. The
dotted-underline terms stay either way.

## A window that stays put

Starting an export moves nothing in the top bar, at any window width: the
progress fills the Export button itself, and the exported count is in its
tooltip. When the window is so narrow that Export lives in the ⋯ menu, a thin
progress line runs along the bottom edge of the top bar instead.

Notices about a saved recording or a finished export never move the chart.
Below 1200 px wide they appear in the page between the chart and the Stats
panel; in wider windows they float over the side panel's bottom-right corner.
