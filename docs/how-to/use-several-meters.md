# Use several meters at once

The desktop app can stream from more than one meter in the same window, for
example a charger's input and output. Each meter has its own session,
readout, chart, recording and replug handling.

## Add a meter

1. Connect the first meter as usual. The app looks exactly as with one meter.
2. Press **Add meter** (at the right end of the tab strip).
3. In **Add a meter**, pick another device and press **Connect**. Meters
   that are already connected are not offered.

With two or more meters the Meter tab shows a grid: one tile per meter with
its name, the last digits of its serial number, its state and a live readout.
A wide window shows two columns; a narrow one stacks the tiles.

Each tile has its own **Record**, **Export**, rate and **Disconnect** controls,
named after its meter, so they act on that meter only. The tiles keep their
order across a reload.

## Look at one meter

- **Expand** on a tile opens that meter in the full single-meter view: chart,
  statistics, Meter settings, recording, PD analyzer and trigger panel all act
  on that meter. **All meters** goes back to the grid. A PD capture keeps
  running when its meter leaves the full view.
- **Disconnect** on a tile ends only that meter's session. With one meter
  left, the app goes back to the single-meter view.

## Unplug and reload

- Unplugging one meter leaves the others streaming. Its tile shows **Waiting
  for meter** and it reconnects on its own when plugged back in, as described
  in [Explore a session](explore-a-session.md).
- A reload of the window picks up every running meter, each with its session
  and any open recording.

## Update firmware on one meter

While one meter updates, you can still add or keep using another. The meter
being updated stays locked. So does any meter the app cannot tell apart from
it, which is one with the same serial number, one with none, or one not in
application mode.

## Try it without hardware

The demo devices (`load-5v-1a`, `pd-9v-2a`, …) can be added like real meters,
so the grid works with no meter plugged in, or with one real meter beside a
demo.
