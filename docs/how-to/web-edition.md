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

## Connect a meter: the device picker and permission

1. Plug the meter into the computer.
2. Click **Connect a meter…**. The browser opens its own USB device picker,
   filtered to the POWER-Z vendor, so only POWER-Z meters are listed.
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
  button appears next to the device list. Click it, pick the meter in the
  browser's HID picker, then connect again.
- A browser with WebHID but no WebUSB uses the HID picker for **Connect a
  meter…**. Meters granted over HID are listed as `webhid:N`.

Over HID the meter only gives basic readings: ADC snapshots (voltage, current,
D+/D-, CC1/CC2, temperature) about ten times a second. On the meter's current
firmware any reply longer than one 64-byte report hangs and reboots it, and
control commands get no answer, so a HID session never starts the sample
queue, reads the device info or captures PD traffic. The top bar shows
"HID: basic readings only", the rate is fixed, and PD capture is refused. For
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

## The web edition only reads and streams

The web edition only reads and streams: live readings, charts and PD
capture. It never writes to the meter. Firmware update, the startup logo
upload, settings changes, deleting offline recordings and protocol triggering
are desktop app only, and so, for now, are reading the meter's settings,
offline recordings and the PDF chart export; those controls say "Desktop app only" in the browser. Get
the desktop app from the
[releases page](https://github.com/nesquikm/open-powerz/releases/latest) (see
[Install a release](install-a-release.md)).
