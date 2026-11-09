# Use the web edition

The web edition runs open-powerz in the browser, with nothing to install:
<https://nesquikm.github.io/open-powerz/>. It talks to the meter over WebUSB
and runs the same protocol code as the desktop app, compiled to
WebAssembly.

## Browser support

The web edition needs WebUSB, so it works in **Chrome** and **Edge** on the
desktop (Windows, macOS, Linux). Firefox and Safari have no WebUSB, and mobile
browsers are not supported. The page must be opened over HTTPS (the GitHub
Pages address is) or from `localhost`.

In a browser with neither WebUSB nor WebHID, such as Safari, the page shows a
"This browser is not supported" box with a link to the desktop app. The demo
meter still works there, and the page shows no connect hint next to the device
menu.

## Connect a meter: the device picker and permission

1. Plug the meter into the computer.
2. Choose **Connect a meter…**, the last item of the device menu. The
   browser opens its own USB device picker, filtered to the POWER-Z vendor, so only POWER-Z meters are listed.
3. Pick the meter and click **Connect**.

Choosing the meter in the picker is the permission: the page cannot see any
USB device you have not picked. The browser remembers that permission per
site, so next time the page can reopen the same meter without the picker. To
take the permission back, open the site settings (the icon left of the
address bar) and remove the device.

## The WebHID fallback

The browser claims the meter's vendor (bulk) interface, picked by its class,
not by its number. When another driver already holds that interface (an OS
driver, or another app such as the desktop app or the `powerz` CLI), the page
falls back to WebHID: the meter's HID interface, which needs no driver on any
OS. WebHID has its own browser picker and permission, separate from the USB
one.

- If the page already has permission for the meter over WebHID, it connects
  over HID on its own.
- Otherwise the error says the meter is busy and a **Connect over HID…**
  item appears in the device menu. Choose it, pick the meter in the
  browser's HID picker, then connect again.
- A browser with WebHID but no WebUSB uses the HID picker for **Connect a
  meter…**. Meters granted over HID are listed as `webhid:N`.

Over HID the meter only gives basic readings: ADC snapshots (voltage, current,
D+/D-, CC1/CC2, temperature) about ten times a second. On the meter's current
firmware any reply longer than one 64-byte report hangs and reboots it, and
control commands get no answer, so a HID session never starts the sample
queue, reads the device info or captures PD traffic. The top bar shows
"HID: basic readings only". The rate picker is disabled, and its tooltip says
the rate is not available over HID. PD capture is refused the same way. For
the full feature set, free the vendor interface (see below) and connect over
WebUSB.

Close any other app that has the meter open before you connect; only one
program can hold the interface at a time.

## Linux: udev access

On Linux the browser runs as your user, so it needs the same device
permissions as the desktop app. Install the udev rule described in
[Linux setup](../linux.md), then unplug and replug the meter. If the kernel's
`powerz` hwmon driver holds the vendor interface, unbind it as that page
shows.

## Windows: driver notes for WebUSB

On Windows, WebUSB can only open an interface bound to the **WinUSB** driver.
The meter's firmware asks Windows to bind WinUSB to its vendor interface by
itself (the same binding the desktop app uses), so usually there is nothing to
do. If another driver was installed for that interface (for example the
vendor's libusb driver installer), WebUSB cannot claim it: switch the
interface back to WinUSB.

## What works in the browser

The web edition does what the desktop app does, apart from the features in the
next section. Each feature needs one browser interface:

- **Live readings, charts and PD capture:** WebUSB (or WebHID for basic
  readings, see above).
- **Reading the meter's settings:** WebUSB. The Meter settings panel shows the
  same rows as the desktop app.
- **Listing the offline records and downloading a record:** WebUSB. The
  Records view lists the records and opens one in the record chart.
- **Deleting an offline record:** WebUSB. Type the record's name (or `all`) to
  confirm. The browser first asks for a folder and writes a CSV backup of every
  affected record there, under `open-powerz-backups/records-<time>`. A
  cancelled folder picker or a failed backup deletes nothing. A browser that
  cannot write a folder saves the backups as downloads instead.
- **Startup logo upload and remove:** WebUSB. Only a KM003C takes a logo.
- **Changing the meter's settings:** Web Serial. The meter takes a settings
  change only on its virtual serial port. The first change opens the
  browser's port picker, filtered to POWER-Z ports. Pick the meter's port and
  the browser remembers it.
- **Protocol triggering:** Web Serial, on the same port as the settings
  change. The first trigger session opens the port picker if the page has no
  port yet. The safeguards are the desktop app's: a request above 5 V needs
  your confirmation, and leaving the panel or disconnecting goes back to 5 V
  first.

Protocol triggering needs a one-time reload: the page must be cross-origin
isolated, so the web edition installs a small service worker, and the first
visit may ask you to reload once.

Web Serial, like WebUSB, works in Chrome and Edge only. If another program
holds the serial port, close it and try again. A HID session cannot read or
change settings, records or the logo; it can still trigger, since triggering
uses only the serial port.

## What stays in the desktop app

These controls say "Desktop app only" in the browser:

- **Firmware update.** An interrupted update could brick the meter, and a
  browser tab can be closed, reloaded or put to sleep at any time.
- **Showing a saved file in the file manager.** A browser page cannot open the
  file manager or see where the browser saved a file.
- **The PDF chart export.** The browser has no PDF writer; export the chart as
  PNG or SVG instead.

Get the desktop app from the
[releases page](https://github.com/nesquikm/open-powerz/releases/latest) (see
[Install a release](install-a-release.md)).
