# Linux setup

On Linux, two things stand between a normal user and the POWER-Z KM002C /
KM003C: device permissions, and the kernel's own `powerz` hwmon driver.

## 1. Install the udev rule

The repo ships [`packaging/linux/60-powerz.rules`](../packaging/linux/60-powerz.rules).
It matches vendor `5fc9`, products `0061` (KM002C) and `0063` (KM003C), and:

- tags the USB device, its `hidraw` node and its CDC tty with `uaccess`, so
  the user logged in at the seat can open them without root or extra groups;
- sets `ID_MM_DEVICE_IGNORE=1`, so ModemManager does not probe the CDC serial
  port (`/dev/ttyACM*`) and send it AT commands.

Install it and reload udev:

```sh
sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Then unplug and replug the tester.

## 2. The `powerz` hwmon driver

Mainline Linux has a `powerz` hwmon driver that binds interface 0
(the vendor bulk interface) of both models and exposes voltage and current via
hwmon/`sensors`. While it holds that interface, no user-space app can claim it.

The HID and CDC interfaces are not affected, so you only need to act if you
want the vendor bulk transport.

The bulk transport tries to detach the kernel driver from interface 0 when it
opens the device (this needs the udev rule above). If that fails, or you want
the driver out of the way for good, do it by hand:

**Unload the module** (until the next reboot or replug):

```sh
sudo modprobe -r powerz     # or: sudo rmmod powerz
```

**Unbind just this device** (leaves the module loaded for other testers):

```sh
# find the interface, e.g. 1-2:1.0
ls /sys/bus/usb/drivers/powerz/
echo -n 1-2:1.0 | sudo tee /sys/bus/usb/drivers/powerz/unbind
```

**Blacklist it permanently:**

```sh
echo "blacklist powerz" | sudo tee /etc/modprobe.d/powerz-blacklist.conf
```

To get hwmon readings back, load it again with `sudo modprobe powerz` (and
remove the blacklist file if you created one).

## 3. Flatpak

A Flatpak build needs `--device=all` to reach raw USB and hidraw nodes; the
udev rule must still be installed on the host.

## Headless, SSH and container use

`TAG+="uaccess"` grants access only to the user logged in at the local seat.
On a headless machine (Raspberry Pi, server), over SSH or inside a container,
use a group instead: add `GROUP="plugdev", MODE="0660"` to each line of the
rule (or copy it to `61-powerz-plugdev.rules` with those keys), add yourself
to the group with `sudo usermod -aG plugdev $USER`, then log in again and
re-plug the meter.
