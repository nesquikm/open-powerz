# `powerz` CLI reference

`powerz [GLOBAL OPTIONS] <COMMAND> [OPTIONS]`. Run `powerz <COMMAND> --help` for
the built-in help.

## Commands

| command | what it does | options |
|---|---|---|
| `list` | One line per attached meter: model, serial, USB bus/address. With none attached it prints `no POWER-Z devices found` to stderr and still exits 0. | `--json` (array of `model`, `serial`, `bus`, `address`), `--include-demo` (also list the simulated demo device) |
| `info` | Model, mode (`application`, `bootloader` or `unknown`), application firmware and bootloader versions with their dates, hardware version, serial and UID, then one `Warning:` line per firmware warning: older than 1.7.0, or older than 2.0.7 (the newest version known). Outside application mode it prints only the mode and sends nothing. Each part of the firmware status (the bootloader block, the application block, then HEAD) that cannot be read costs only that part: the rest, the dated versions and the version warnings are still printed. When HEAD cannot be read it adds `Update check: unavailable (<reason>)`, with the reason and the raw reply. A HEAD field of 0 means the firmware is current; any other value (2.0.6 answers `0x00001000`) is reported the same way as an unrecognised reply naming the field and the raw bytes, never as "update required", because what that field means is not yet confirmed on a real meter. Each info block that cannot be read adds one `Firmware status: <problem>` line naming the block, its address and why (the versions shown for it then come from the device info). If the meter cannot be reconnected after a failed step, the remaining steps are skipped and one `Firmware status:` line says so; `info` still succeeds. | `--json` (top level: `model`, `mode`, `firmware_version`, `hardware_version`, `update_required` (null when HEAD was not read or its field was not 0), `warnings` as codes `too_old` / `outdated`, `firmware_problems` (one line per status part that could not be read, empty when all were read), `serial`, `uid`; plus `application` and `bootloader`, each an object with `model`, `version` and `date`, or null) |
| `read` | One snapshot: VBUS (V), IBUS (A, signed), power (W), temperature (°C), D+, D−, CC1, CC2 (V). | `--json` (`vbus_v`, `ibus_a`, `power_w`, `temp_c` or null, `dp_v`, `dm_v`, `cc1_v`, `cc2_v`) |
| `watch` | The `read` view, refreshed until Ctrl-C. Redraws in place on a terminal, one line per refresh otherwise. | `--interval MS` (default 200, minimum 50) |
| `stream` | Records the ADC sample stream to CSV until Ctrl-C or `--duration`; prints the sample count and any gaps to stderr. | `--rate {2,10,50,1000}`, `--csv FILE` (created or overwritten), `--duration SECONDS` (device time, fractions allowed) |
| `pd` | Captures USB PD traffic (messages and events) until Ctrl-C and prints one line per record: elapsed ms, sender (`meter` or `line`), then for a message its SOP, PD header and data objects in hex and a one-line decoded summary that starts with the message name; for an event its name, with the CC line or a detail where it has one. Bulk pipe (or `sim`) only. | `--json` (one JSON object per record, JSON Lines), `--save FILE` (also save the capture as a [pdcap file](pdcap.md)) |
| `pd decode` | Prints the records of a saved [pdcap file](pdcap.md) as `pd` would; no meter needed. On a damaged file it prints every record before the damage, then the line number where reading stopped, and exits 5. | `--json`, `FILE` |
| `pd send` | Sends one custom PD message through the meter: a control message on SOP as `pd cmd=`, everything else as `pd data=`. See [Custom PD messages](#custom-pd-messages). | `HEX` or `--preset discover-identity-cable`, `--sop {sop,sop1,sop2}` (default `sop`), `--allow-high-voltage`, `--json` |
| `trigger` | Asks a charger for a voltage through the meter (protocol triggering). One subcommand per protocol; see [Protocol triggering](#protocol-triggering). | `pd`, `qc`, `qc3`, `fcp`, `afc`, `sfcp`, `scp`, `vfcp`, `tfcp`, `ufcs` |
| `detect` | Detects the charger's fast-charging protocols: BC1.2, Apple, and the automatic list on request. Never raises VBUS above 5 V. See [Detection](#detection). | `--list`, `--list-plus`, `--stall-secs N` (default 45), `--json` |
| `console` | Raw PDM text console: sends the command lines it reads from stdin and prints what was sent and received, with times. See [Console](#console). | `--allow-high-voltage` |
| `firmware info` | Reads a local firmware container file (`.mencrypt`) and prints its vendor, container version, model, version, date, CRC-32, size and English release notes. Needs no meter and never decrypts the file. A broken, oversized (over 8 MiB) or unreadable file is refused with exit code 2 and the reason, prefixed with the file name. | `FILE`, `--json` (`vendor`, `container_version`, `model`, `version`, `date`, `crc` as 8 hex digits, `size`, `release_notes_en`) |
| `firmware probe` | Read-only check for either mode: prints the meter's USB identity (vendor and product IDs, product string, mode, and each interface's class and string) without opening it, then over the bulk pipe sends only CONNECT and HEAD and prints the results. It always uses the bulk pipe, whatever `--transport` says (except `sim`). If the meter has no vendor-class interface, it prints the identity and sends nothing. | `--json` (`vid`, `pid`, `product`, `mode`, `interfaces`, `connect`, `head`, `head_field`, `head_raw`). `head_raw` holds the reply in hex when it did not decode, and also next to `head_field` when the meter answered with reply id 0 (as it does in application mode), so a run records exactly what it said |
| `firmware flash` | Flashes a firmware container onto the meter's application area through its bootloader, over the bulk pipe only (`--transport bulk` or `sim`). Refused with exit code 2 and nothing sent unless `--i-understand` is given; the file is then inspected (as `firmware info` does) before any meter is chosen or opened, and a broken or truncated file is refused with exit code 2, prefixed with the file name. From application mode it checks that the file's model matches the meter, switches the meter to its bootloader and waits for it to come back. Prints each stage on its own line (switch, erase, write as bytes done / total about every tenth of the image, verify, finished, return to application mode), then the new version the meter reports, or asks you to unplug and replug the meter if it stays in its bootloader. A failed flash exits 5 with the vendor error number and how to recover. See [Firmware flashing](#firmware-flashing). | `FILE`, `--i-understand` |
| `firmware jump` | Switches the meter between modes with one write-only jump over the bulk pipe only (`--transport bulk` or `sim`): `--to bootloader` sends only JUMP_DFU (from application mode), `--to application` only JUMP_APROM (from bootloader mode), through the same strict guard as `firmware flash`, and nothing else (no CONNECT, HEAD or info read). Refused with exit code 2 and nothing sent unless `--i-understand` is given. A meter already in the target mode, or in an unknown mode, is refused with exit code 5 and nothing sent. After the jump it waits up to 5 s for the meter to come back and prints the mode it was in and the mode it is in now; if the meter does not come back in the target mode it exits 5 naming the mode seen (after `--to bootloader`, with the hold-Back hint). See [Firmware flashing](#firmware-flashing). | `--to bootloader\|application`, `--i-understand` |

## Global options

| option | meaning |
|---|---|
| `--device SERIAL` | The meter to use. Required when several are attached. |
| `--transport bulk\|cdc\|hid\|sim` | The USB pipe. `bulk` (default) supports every measuring command; `cdc` and `hid` support only `read` and `watch` (and `pd decode`, which needs no meter); `sim` is the demo device. `trigger`, `detect`, `console` and `pd send` always talk to the meter's virtual serial port, whatever pipe is chosen; with `sim` they talk to a simulated meter and charger instead. For `detect`, the pipe also takes the one ADC snapshot behind the D+/D− hint. |
| `--scenario NAME` | What the demo device plays: `idle` (default), `load-5v-1a`, `pd-9v-2a`, `noisy-negative`. |
| `--trace` | Logs every frame to stderr as hex: `>` sent, `<` received, `x` refused by the safety guard before sending. |

## Protocol triggering

`powerz trigger <PROTOCOL> <ACTION>` opens the PDM text session on the
meter's virtual serial port, enters the protocol, does the action, and ends
the session. Read [Safety](#safety) first.

| command | what it does | options |
|---|---|---|
| `trigger pd list` | Enters PD and lists the charger's offers (SPR and EPR), decoded: position, type, voltage or range, maximum current or power, and whether EPR mode is on. | `--json` (`epr` and `offers`) |
| `trigger pd request POS` | Requests the offer at object position `POS` (see `trigger pd list`; EPR is 8 and up), holds it, then returns to 5 V. A PPS or AVS offer needs `--voltage-mv` and `--current-ma`. The request is checked against the offer before it is sent. Prints the command and the requested and delivered voltage and current. | `--voltage-mv MV`, `--current-ma MA` (required with `--voltage-mv`; for a fixed offer the default is its maximum), `--allow-high-voltage`, `--hold SECS`, `--json` |
| `trigger pd reset` | Sends a Hard Reset: the charger returns to 5 V and offers again. Lists the new offers and VBUS. | `--json` (adds `vbus_mv`, `vbus_ma`) |
| `trigger pd settings` | Shows the meter's PD settings, or changes them. A flag left out keeps the current value. Reserved e-marker codes are shown but never set. | `--type {auto,spr,epr,proprietary-pps}`, `--em {off,20v-5a,50v-240w-epr,la135}`, `--sink {pps-3a,pps-5a}`, `--json` (`type`, `em`, `sink`) |
| `trigger qc request VOLTS` | Quick Charge 2.0: 5, 9, 12 or 20 V. | `--allow-high-voltage`, `--hold SECS`, `--json` |
| `trigger qc3 request --voltage-mv MV` | Quick Charge 3.0: 3600 to 20000 mV in 200 mV steps. | `--allow-high-voltage`, `--hold SECS`, `--json` |
| `trigger qc3 inc N` / `trigger qc3 dec N` | Steps the QC 3.0 voltage up or down by `N` steps of 200 mV. Each run enters QC afresh, so steps count from the entry voltage. `inc` always needs `--allow-high-voltage`. | `--allow-high-voltage`, `--hold SECS`, `--json` |
| `trigger fcp request VOLTS`, `trigger afc request VOLTS`, `trigger sfcp request VOLTS` | Huawei FCP, Samsung AFC, Spreadtrum SFCP: 5, 9 or 12 V. | `--allow-high-voltage`, `--hold SECS`, `--json` |
| `trigger scp list`, `trigger vfcp list`, `trigger tfcp list` | Huawei SCP, vivo VFCP, Transsion TFCP: enters the protocol and shows the decoded limits of its capability block (voltage, current, maximum power). | `--json` (`protocol`, `limits`) |
| `trigger scp request`, `trigger vfcp request`, `trigger tfcp request` | Requests a voltage and current inside the limits, holds it, then returns to 5 V. | `--voltage-mv MV`, `--current-ma MA` (both required), `--allow-high-voltage`, `--hold SECS`, `--json` |
| `trigger ufcs list` | Enters UFCS and shows its output capabilities, numbered from 1. | `--json` |
| `trigger ufcs request N` | Requests a voltage and current inside capability `N`, holds it, then returns to 5 V. | `--voltage-mv MV`, `--current-ma MA` (both required), `--allow-high-voltage`, `--hold SECS`, `--json` |
| `trigger ufcs cmd N` | Sends UFCS control message `N` (its number in the UFCS spec), then returns to 5 V. | `--allow-high-voltage`, `--hold SECS`, `--json` |

`--hold SECS` is how long the voltage is held before the return to 5 V.
Without it, the voltage is held until Ctrl-C. When the charger does not
answer a protocol, the command says the charger does not support it and
exits 5; the meter stays at 5 V.

## Detection

`powerz detect` runs `entry bc` (BC1.2) and `entry apple`, prints a D+/D−
hint, and with `--list` (or `--list-plus`) also runs the meter's automatic
protocol list.

- `--list` runs `entry list`. Each protocol line is printed as it arrives,
  with its result and VBUS/IBUS.
- `--list-plus` runs `entry list+` instead. It adds VOOC and MTK PE and needs
  a dummy load.
- `--stall-secs N`: when the list is silent for `N` seconds (45 by default),
  it ends with `reset` and the result is marked incomplete.
- The hint maps the measured D+ and D− voltages to likely charging modes
  (for example Apple 2.4 A, Samsung 2 A, DCP). It is labelled as a hint, not
  a measurement.
- When BC1.2 or Apple and the list disagree, a `disagree` line names which
  check said what.
- `--json` prints one object instead of labelled lines.

Detection commands are rated 5 V or less and never need
`--allow-high-voltage`. The automatic list lets the meter probe protocols,
which can briefly raise VBUS above 5 V on some chargers, so `detect --list`
prints a note to keep the output empty.

## Custom PD messages

`powerz pd send [HEX]` sends one PD message through the meter's PDM session
(not the bulk pipe that `pd` captures on).

- `HEX` is the PD header, little-endian, then the data objects, little-endian.
  No SOP byte.
- `--sop` picks the target: `sop` (the charger, default), `sop1` (SOP', the
  near cable plug) or `sop2` (SOP'', the far plug).
- `--preset discover-identity-cable` sends a Discover Identity request to the
  cable (SOP') instead of `HEX`. When the cable answers, its e-marker details
  are decoded: VID, PID, passive or active, maximum voltage and current, and
  EPR capability. The cable answers only the VCONN source, so it may stay
  silent behind some chargers.
- A message that could have raised VBUS is followed by a return to 5 V.
- GoodCRC (`pd cmd=1`) is always refused: the meter's PD stack sends it
  itself.

## Console

`powerz console` reads command lines from stdin, sends each one through the
PDM session, and prints the transcript:

```
00:00:00.100 > pdm parm
00:00:00.100 < type=0,em=2,sink=0
```

Times are UTC clock times. `>` is sent, `<` is received, and a binary
capability block in a reply is printed parsed into words. A refused line is
reported on stderr (`refused: ...`) and the console goes on with the next
line. When stdin ends, or on Ctrl-C, the console returns to 5 V if a line
could have raised VBUS, then sends `pdm close`.

## Safety

Every command that `trigger`, `detect`, `console` and `pd send` send is
classified before it is written:

| risk class | what it covers |
|---|---|
| read-only | Reads only: the settings, the PD offers, the UFCS capabilities. |
| 5 V or less | Session control, protocol entry, detection, Hard Reset, and requests that provably stay at or below 5 V. |
| above 5 V | Anything that may raise VBUS above 5 V. This includes every command the classifier cannot prove safe: unknown or raw text, a request for a position with no known offer, `qc3 inc`, UFCS control messages, PD role swaps, and any PD data message other than a Request (a Request is rated by the voltage it asks for). So the cable Discover Identity preset needs the flag too. |

- **`--allow-high-voltage`.** A command rated above 5 V is refused with exit
  code 2 unless this flag is given. Nothing is written to the meter before
  the check. Example: `` `pd req=4` can raise VBUS above 5 V; pass
  --allow-high-voltage to send it ``.
- **Keep the output empty.** Run triggering with nothing on the meter's
  output, or only a device rated for the voltage you ask for.
- **Always back to 5 V, always `pdm close`.** A request holds its voltage,
  then returns to 5 V: PD sends `pd req=1` (a Hard Reset if that fails), the
  other protocols send `reset`. Then the session ends with `pdm close`. This
  also happens on error and on Ctrl-C: Ctrl-C only sets a stop flag, so it
  never kills the process between a request and the return to 5 V. If the
  return to 5 V fails, the CLI warns that VBUS may still be above 5 V and
  tells you to unplug the charger from the meter.
- **GoodCRC is refused** in `pd send` and `console`, whatever the flags.
- **Out of step.** If the meter answers a command with a reply that belongs
  to another command, its replies lag behind the commands. The session
  refuses every later command and the CLI exits 5 with a message that says
  to power-cycle the meter (unplug it and plug it back in). After an
  incomplete automatic list, `detect` checks with a read-only command that
  the meter is still in step before it sends anything else.

## Firmware flashing

`powerz firmware flash FILE --i-understand` is the only command that writes
to the meter's flash. `powerz firmware jump --to bootloader|application
--i-understand` writes nothing to flash, but it is the only other command
that sends a write-capable frame: one JUMP_DFU or JUMP_APROM.

- **`--i-understand`.** Without it the command exits 2, explains the risk
  and opens no meter.
- **The file is checked first.** It must parse as a firmware container
  (header fields and sizes) before any meter is chosen or opened, and its
  model must match the meter's.
- **The bootloader is never written.** Only the application area (from
  0x4000) can be named; every other command is refused before it is sent.
- **No automatic retry.** If a step fails once the meter is in its
  bootloader, nothing more is sent and the meter stays there. Keep it
  plugged in and run the command again; if the meter does not reach its
  bootloader on its own, hold the Back button while plugging it in.
- **Ctrl-C** stops the flash at its next safe point: before the switch to
  the bootloader, before the descriptor, or between two payload writes.
  The command then exits 5 saying the flash was interrupted, with the same
  recovery text as a failure; once the meter is in its bootloader nothing
  more is sent and it stays there until you flash again.
- **Finding the meter again.** After the switch the meter is looked up
  afresh: the one with the `--device` serial, else the only POWER-Z
  attached, even if its serial changed. With several other meters attached
  the command stops and names them; unplug the others.
- **Do not unplug the meter** while the flash runs.
- **`firmware jump`** needs `--i-understand` as well (exit 2, nothing sent
  without it) and runs on the bulk pipe only. It sends exactly one jump
  frame through the flash guard, only from the opposite mode, and nothing
  else. Ctrl-C applies only before the jump frame is sent: it then sends
  nothing and exits as interrupted; once the frame is written the jump
  cannot be undone, and the command waits for the meter as usual. The
  `Was in:` line is printed as soon as the jump frame is written, so it
  shows even when that write fails. In its bootloader the meter
  takes no measurements: run `firmware jump --to application
  --i-understand`, or unplug the meter and plug it back in.

## Output formats

- Human views print 3 decimals, temperature 1 decimal (`30.5 °C`).
- JSON numbers are SI units, written as plain decimals, never in exponent form.
- CSV header: `ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR`.
  ElapsedTime is `h:mm:ss.mmm` and Unix is seconds with 3 decimals, both from
  the first sample. Numeric columns have at most 8 significant digits as plain
  decimals. TEMP is empty (the queue carries no temperature). CHARGE (Ah) and
  ENERGY (Wh) are integrated from signed current; PWR = VBUS × IBUS.
- `pd --json` and `pd decode --json` lines: `kind` (`message`, `big_message`
  or `event`), `elapsed_ms`, `from_meter`; messages add `sop`, `header` (as a
  number), `body` (hex), and the decoded `name` (for example
  `Source_Capabilities`) and one-line `summary`; events add `event` (for example `attach`), `cc` for
  attach and detach, and `detail` for receive errors and monitor types.
- `pd --save` files: the [pdcap format](pdcap.md), JSON Lines with a header
  line and one line per status sample or record. The serial number is never
  written.

## Exit codes

| code | meaning |
|---|---|
| 0 | success |
| 2 | usage error, including an ambiguous device choice, `firmware flash` or `firmware jump` without `--i-understand` or with a file that fails inspection, a request the charger does not offer or allow, and a command above 5 V without `--allow-high-voltage` |
| 3 | no matching device (or no POWER-Z serial port) |
| 4 | permission denied (on Linux the message names the udev rule) |
| 5 | device or protocol error, including a failed firmware flash, a protocol the charger does not support, a failed request, a meter out of step, and a failed return to 5 V |

## Test knobs (demo device)

These environment variables are for tests only. They work only with
`--transport sim`; any other value than the ones listed is a usage error.

| variable | effect |
|---|---|
| `POWERZ_SIM_CLOCK=fake` | Deterministic simulated time. |
| `POWERZ_SIM_FAULT` | Injects faults, for example `timeout@2,disconnect@3,wrong-id@4,permission`. |
| `POWERZ_SIM_CTRL_C_AFTER=N` | Acts as if Ctrl-C was pressed after `N` polls (`firmware flash`: after `N` payload writes; `firmware jump`: only `0` applies, a Ctrl-C before the jump). |
| `POWERZ_SIM_CHARGER` | The simulated charger behind `trigger`, `detect`, `console` and `pd send`: `epr-140w` (default), `pps-65w` or `multi-protocol`. |
| `POWERZ_SIM_CABLE=epr-240w` | Puts an e-marked 240 W EPR cable on the simulated meter, so `pd send --preset discover-identity-cable` gets an answer. Unset, there is no cable. |
| `POWERZ_SIM_ENTRY_BC=fail` | `entry bc` answers `fail`. |
| `POWERZ_SIM_PDM_LAG=after-list` | The simulated meter answers one reply late after the `reset` that ends a stalled list, to test the out-of-step error. |
| `POWERZ_SIM_HEAD=N` | The simulated meter's HEAD reply carries old-firmware field `N` (non-zero means update required). |
| `POWERZ_SIM_APP_VERSION=V` | The simulated application info block reports version `V` (default `2.0.6`). |
| `POWERZ_SIM_MODE` | `application` (default) or `bootloader`; read only by `firmware probe`, `firmware flash` and `firmware jump`. |
| `POWERZ_SIM_FLASH_FAULT` | The simulated bootloader injects one flash fault; read only by `firmware flash` and `firmware jump`: `jump-dfu-ignored`, `jump-dfu-write-fails`, `jump-dfu-disconnects`, `connect-no-accept`, `head-wrong-type`, `descriptor-answered`, `early-accept-during-erase`, `no-accept-after-erase`, `missing-sync@N` or `write-fails@N` (payload write `N`, 1-based), `no-finished`, `wrong-verify-reply`, `jump-aprom-ignored`, `jump-aprom-disconnects`, `info-read-fails-after-return`, `image-not-taken`. |
