# Update the meter's firmware

This guide updates a KM003C's application firmware from a firmware file you
downloaded from ChargerLAB (a `.mencrypt` container, for example
`KM003C_V2.0.7.mencrypt`). open-powerz does not ship or download firmware.
open-powerz never sends a write below address 0x4000, where the bootloader
lives, so on bootloader 2.5 a failed update can be retried from bootloader
mode [device]. On the old bootloader 2.1 the same layout and the retry are
expected but not verified (see [Old bootloader](#old-bootloader)).

Tested on a real KM003C going from application 2.0.6 to 2.0.7 (bootloader 2.5).

Firmware update is not in the web edition: an interrupted update could brick
the meter, and a browser tab can be closed, reloaded or put to sleep at any
time. Use the desktop app or the CLI.

## Before you start

- Plug the meter straight into the computer, on a port that has been stable.
  Leave its USB-C output empty.
- Keep the laptop on mains power and awake for a few minutes.
- Close every other program that uses the meter (the desktop app too, when
  you use the CLI).
- Unplug the meter only when you are asked to.

## Check the file

```bash
powerz firmware info KM003C_V2.0.7.mencrypt
```

It prints the vendor, model, version, date, CRC-32, image size and the
English release notes. It needs no meter and never decrypts the file. Make
sure the model is your meter's.

## Update from the command line

```bash
powerz firmware flash KM003C_V2.0.7.mencrypt --i-understand
```

Without `--i-understand` the command explains the risk and sends nothing.
With it:

1. The file is inspected again and its model is checked against the meter.
2. From application mode the command signs in to the meter (CONNECT, the UID
   read and a level-1 authentication) and sends the switch to the bootloader.
   The meter normally restarts in its bootloader by itself. If it does not,
   the command asks you to unplug it and plug it back in (no buttons) and
   waits up to two minutes.
3. It erases the application area, writes the image in 4080-byte blocks and
   waits for the bootloader to accept it, printing each stage.
4. It switches the meter back to its application, again asking for a replug
   only if the meter does not restart by itself, and prints the version the
   meter now reports.

The 2.0.7 update took about 11 seconds on a KM003C, plus any replug. Add `--trace` to see every
frame on stderr.

To start from the bootloader instead, unplug the meter, hold **Back** while
plugging it in, then run the same command. Check it first with
`powerz firmware probe`, which sends only CONNECT and HEAD.

## Update from the desktop app

1. Open **About** and turn on **Advanced firmware**. The **Firmware** tab
   appears; the switch is off by default.
2. Connect the meter, open the **Firmware** tab and press **Choose file…**.
   The file's details show, and **Update** is enabled only when the file
   passes inspection and its model matches the meter. When the file stores
   more bytes than its image (the 2.0.7 file stores 16 more), a **Sent to the
   meter** row shows the stored size: the update sends all of it.
3. Press **Update**, type the version shown (for example `2.0.7`) and press
   **Start update**.
4. The screen shows each stage and the write progress, which counts the bytes
   sent. If it asks you to replug the meter, unplug it and plug it back in
   without pressing any button.
5. When it ends, the screen shows the version read back from the meter, and a
   **Device list** line shows the labels the meter took during the update, for
   example `KM003C — serial → KM003C — serial (bootloader mode) → KM003C —
   serial`. The line stays until you press **Close**.

While the update runs, the measurement stream, PD capture, the trigger panel
and device switching are blocked, and the window cannot be closed.

Cancel update works until the erase begins. If the meter is switching to its
bootloader, the update stops once it gets there and the meter stays in its
bootloader, with the application untouched (see below). After the erase has begun,
Cancel is off and says why. If you reload the window during an update, the page
shows the running stage, but the update's result stays with the page that
started it: check the firmware version afterwards.

If the update would end a session on the meter, or on another meter the app
cannot tell from it, the app lists those sessions, says which are recording,
and waits for your answer. **Keep sessions** (or **Keep recording**) leaves
everything running; **End sessions and update** (or **End recording and
update**) ends them and starts the update.

## If something goes wrong

Nothing is retried automatically. After a failure the meter stays in its
bootloader:

- **"firmware details" screen:** the bootloader, with a valid application.
- **Blank screen with a blinking green bar:** the bootloader, with no valid
  application (the area was erased). The meter is fine; it needs a flash.
- **A "write flash" screen whose progress stopped:** the bootloader was left
  in the middle of a download. Send nothing to it.

In every case: unplug the meter, wait ten seconds, hold **Back** while
plugging it back in, then run the update again with the same file. If
`powerz firmware probe` says the bootloader was left mid-download, do the
same power cycle first.

If the update stops because the meter is on an old bootloader, see
[Old bootloader](#old-bootloader).

## Old bootloader

Some KM003C meters run bootloader 2.1; current meters have 2.5. The desktop
app cannot update a meter with bootloader 2.1; the CLI has an experimental
path for it, described below.

**What you see:** after the jump to the bootloader, the meter shows its
bootloader screen with a flashing green bar. Without
`--experimental-old-bootloader`, open-powerz recognises the old bootloader and
stops at once: the message says the meter is now in its old bootloader, that
this bootloader is "supported only experimentally, from the CLI", that you can
retry with the experimental flag while it stays there, and that a replug
returns it to normal. It sends nothing to the bootloader, so the firmware on
the meter is unchanged. With the flag, the update goes on into the
experimental path below.

**In the app:** while the meter sits in the old bootloader, the device list
names it "(old bootloader, update via CLI)" and Connect refuses it, naming the
experimental CLI command. `powerz firmware probe` prints:

```
Mode: bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)
```

**Back to normal:** unplug the meter and plug it back in. You do not need to
hold any button. The meter starts its application firmware again and works as
before.

**Updating for now:** the tested route is ChargerLAB's official Windows PC
software, from the
[official support page](https://www.power-z.com/blogs/technical-support/power-z-km003c-km002c-technical-support).

**Experimental: update from the CLI.** v0.14.0 found that bootloader 2.1 does
answer, but numbers its replies itself instead of echoing ours; `powerz
firmware probe` now shows a clean CONNECT and HEAD with "answered with
mismatched ids". The CLI can flash such a meter, but only when you ask for it. Either start
from the application (the meter switches itself to its bootloader, as for any
update), or start from the bootloader:

1. Unplug the meter, then hold **Back** while plugging it in, so it starts
   straight in its bootloader (the green bar). Skip this step to start from
   the application.
2. Run:

   ```bash
   powerz firmware flash FILE --i-understand --experimental-old-bootloader
   ```

If an update you started without the flag stopped because the meter landed in
its old bootloader, you can run the command above right away, while the meter
still shows the green bar.

This path has been tested only in simulation, never on a real meter with
bootloader 2.1, and the command says so before it sends anything to that bootloader. open-powerz
never sends a write below 0x4000. Bootloader 2.1 keeping that layout and taking
a retry after a failed run is expected but not verified on a real meter. To retry, hold Back while
plugging the meter in and run the same command again. Without the flag the command still refuses the meter and
sends nothing.

**Help add support:** open an issue on
[GitHub](https://github.com/nesquikm/open-powerz/issues) and attach a report
with the output of these read-only commands, run while the meter shows the
bootloader screen (they send only CONNECT and HEAD):

```bash
powerz firmware probe --json
powerz --trace firmware probe
lsusb -v -d 5fc9:0063    # Linux only
```

The probe output lists the meter's USB endpoints and string descriptors, and
says which half of CONNECT or HEAD timed out (the write to the meter or the
read back). If the standard CONNECT gets no answer, the probe also tries three
read-only variants of it and prints which variant, if any, the meter answered.
All of this helps us find out how bootloader 2.1 expects to be spoken to.

Only run `powerz firmware flash` on a meter with bootloader 2.1 with
`--experimental-old-bootloader`, as described above.

See also: [`powerz` CLI reference](../reference/cli.md#firmware-flashing) and
the firmware download sequence in [docs/protocol.md](../protocol.md#firmware-download).
