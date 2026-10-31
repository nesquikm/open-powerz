# Update the meter's firmware

This guide updates a KM003C's application firmware from a firmware file you
downloaded from ChargerLAB (a `.mencrypt` container, for example
`KM003C_V2.0.7.mencrypt`). open-powerz does not ship or download firmware.
The bootloader, which lives below address 0x4000, is never written, so a
failed update can always be retried.

Tested on a real KM003C going from application 2.0.6 to 2.0.7 (bootloader 2.5).

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
   passes inspection and its model matches the meter.
3. Press **Update**, type the version shown (for example `2.0.7`) and press
   **Start update**.
4. The screen shows each stage and the write progress. If it asks you to
   replug the meter, unplug it and plug it back in without pressing any
   button.
5. When it ends, the screen shows the version read back from the meter.

While the update runs, the measurement stream, PD capture, the trigger panel
and device switching are blocked, and the window cannot be closed.

Cancel update works until the erase begins. If the meter is switching to its
bootloader, the update stops once it gets there and the meter stays in its
bootloader, with the application untouched (see below). After the erase has begun,
Cancel is off and says why. If you reload the page during an update, the page
shows the running stage, but the update's result stays with the page that
started it: check the firmware version afterwards.

If the meter is recording when you start the update, the app asks before it
ends the recording.

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

See also: [`powerz` CLI reference](../reference/cli.md#firmware-flashing) and
the firmware download sequence in [docs/protocol.md](../protocol.md#firmware-download).
