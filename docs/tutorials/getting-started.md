# Getting started with open-powerz

This tutorial builds the `powerz` command-line tool and takes a first reading.
You don't need a meter: the built-in demo device stands in for one.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.96 or newer.
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

## Where to next

- Recipes: [record a high-rate CSV](../how-to/record-a-csv.md)
- Every command and option: [CLI reference](../reference/cli.md)
- Why it is built this way: [architecture](../explanation/architecture.md)
