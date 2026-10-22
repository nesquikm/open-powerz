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

## Axes

With zero-based axes off, each value axis spans at least 0.1 V, 0.02 A or
0.1 W. Noise around an idle output no longer fills the whole axis; real swings
still autoscale. The overview strip uses the same minimum span.

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

## A window that stays put

Starting an export moves nothing in the top bar, at any window width: the
progress fills the Export button itself, and the exported count is in its
tooltip. When the window is so narrow that Export lives in the ⋯ menu, a thin
progress line runs along the bottom edge of the top bar instead.

Notices about a saved recording or a finished export never move the chart.
Below 1200 px wide they appear in the page between the chart and the Stats
panel; in wider windows they float over the side panel's bottom-right corner.

## For scripts and other front ends

The backend command `historyGaps` (a list of `HistoryGap` entries, each with a
start and an end time) lists the breaks between measuring runs that overlap a
time range.
