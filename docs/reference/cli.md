# `powerz` CLI reference

`powerz [GLOBAL OPTIONS] <COMMAND> [OPTIONS]`. Run `powerz <COMMAND> --help` for
the built-in help.

## Commands

| command | what it does | options |
|---|---|---|
| `list` | One line per attached meter: model, serial, USB bus/address. With none attached it prints `no POWER-Z devices found` to stderr and still exits 0. | `--json` (array of `model`, `serial`, `bus`, `address`), `--include-demo` (also list the simulated demo device) |
| `info` | Model, firmware version, hardware version, serial and UID. | `--json` (`model`, `firmware_version`, `hardware_version`, `serial`, `uid`) |
| `read` | One snapshot: VBUS (V), IBUS (A, signed), power (W), temperature (°C), D+, D−, CC1, CC2 (V). | `--json` (`vbus_v`, `ibus_a`, `power_w`, `temp_c` or null, `dp_v`, `dm_v`, `cc1_v`, `cc2_v`) |
| `watch` | The `read` view, refreshed until Ctrl-C. Redraws in place on a terminal, one line per refresh otherwise. | `--interval MS` (default 200, minimum 50) |
| `stream` | Records the ADC sample stream to CSV until Ctrl-C or `--duration`; prints the sample count and any gaps to stderr. | `--rate {2,10,50,1000}`, `--csv FILE` (created or overwritten), `--duration SECONDS` (device time, fractions allowed) |

## Global options

| option | meaning |
|---|---|
| `--device SERIAL` | The meter to use. Required when several are attached. |
| `--transport bulk\|cdc\|hid\|sim` | The USB pipe. `bulk` (default) supports every command; `cdc` and `hid` support only `read` and `watch`; `sim` is the demo device. |
| `--scenario NAME` | What the demo device plays: `idle` (default), `load-5v-1a`, `pd-9v-2a`, `noisy-negative`. |
| `--trace` | Logs every frame to stderr as hex: `>` sent, `<` received, `x` refused by the safety guard before sending. |

## Output formats

- Human views print 3 decimals, temperature 1 decimal (`30.5 °C`).
- JSON numbers are SI units, written as plain decimals, never in exponent form.
- CSV header: `ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR`.
  ElapsedTime is `h:mm:ss.mmm` and Unix is seconds with 3 decimals, both from
  the first sample. Numeric columns have at most 8 significant digits as plain
  decimals. TEMP is empty (the queue carries no temperature). CHARGE (Ah) and
  ENERGY (Wh) are integrated from signed current; PWR = VBUS × IBUS.

## Exit codes

| code | meaning |
|---|---|
| 0 | success |
| 2 | usage error, including an ambiguous device choice |
| 3 | no matching device |
| 4 | permission denied (on Linux the message names the udev rule) |
| 5 | device or protocol error |

## Test knobs (demo device)

`POWERZ_SIM_CLOCK=fake` (deterministic simulated time), `POWERZ_SIM_FAULT`
(for example `timeout@2,disconnect@3,wrong-id@4,permission`) and
`POWERZ_SIM_CTRL_C_AFTER=N` (act as if Ctrl-C was pressed after N polls).
