# Getting started with open-powerz

This tutorial builds the `powerz` command-line tool and the desktop app, and
takes a first reading with each. You don't need a meter: the built-in demo
device stands in for one.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) through rustup. It installs
  the version pinned in `rust-toolchain.toml` on the first cargo command.
- For the desktop app: [Bun](https://bun.sh) and the
  [Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/)
  for your OS.
- On Linux: `libudev-dev` and `libusb-1.0-0-dev` (or your distribution's
  equivalents) to build, and the udev rule from [Linux setup](../linux.md)
  to reach a real meter without root.
- Optional: a ChargerLAB POWER-Z KM003C or KM002C and a USB-C cable.

## A first run with the demo device

From a clone of the repository:

```bash
cargo run -p powerz-cli -- --transport sim info
cargo run -p powerz-cli -- --transport sim --scenario load-5v-1a read
```

`info` prints the simulated KM003C's model, versions, serial and UID. `read`
prints one snapshot:

```
VBUS:   5.000 V
IBUS:   1.000 A
Power:  5.000 W
Temp:   30.0 °C
D+:     0.600 V
D−:     0.600 V
CC1:    1.680 V
CC2:    0.000 V
```

Add `--json` to get the same values as one JSON object, or run `watch` to see
them refresh live until you press Ctrl-C.

## The same with a real meter

Plug the meter into your computer, then:

```bash
cargo run -p powerz-cli -- list
cargo run -p powerz-cli -- read
```

`list` shows each attached meter with its model, serial and USB bus/address.
With more than one attached, pick one with `--device SERIAL`.

## The desktop app

From the repository root:

```bash
bun install
bun run dev
```

The app opens with an empty chart that says "Connect a meter to start". Pick
"Demo device — <scenario>" (or your meter) in the device picker and press
Connect: the big readout shows VBUS, IBUS, power and temperature, the chart
follows the newest data, and the Stats panel counts charge and energy. Choose a
sample rate in the rate picker, or press 1 to 4 (2, 10, 50 or 1000 samples per
second).

Press Record to write a CSV while you watch, or Export to save the whole
session so far. Press ? for every keyboard shortcut.

## Watch a PD negotiation

Open the **PD analyzer** tab and press **Start capture**. With a demo device
connected, the table fills with a scripted USB PD negotiation: one row per
message or event, with a one-line summary. Select a row to see its decoded
fields and raw bytes. Back on the **Meter** tab, the same messages show as
markers above the chart.

The CLI shows the same capture as text:

```bash
cargo run -p powerz-cli -- --transport sim pd
```

Press Ctrl-C to stop it.

## Where to next

- Recipes: [record a high-rate CSV](../how-to/record-a-csv.md),
  [explore a session in the desktop app](../how-to/explore-a-session.md),
  [analyze USB PD traffic](../how-to/analyze-pd-traffic.md),
  [trigger a charger](../how-to/trigger-a-charger.md) (ask it for 9 V, 20 V
  and more, with the safety rules first),
  [update the meter's firmware](../how-to/update-firmware.md)
- Every command and option: [CLI reference](../reference/cli.md)
- Why it is built this way: [architecture](../explanation/architecture.md)
