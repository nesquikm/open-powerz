# open-powerz Docs

This tree follows the [Diátaxis framework](https://diataxis.fr/): four kinds of
documentation, each for a different need. Pick the one that matches what you
want to do.

## Tutorials {#tutorials}

Learn by doing. New to open-powerz? Start with
[tutorials/getting-started.md](tutorials/getting-started.md): build the `powerz`
CLI and the desktop app and take your first readings, with or without a meter.

## How-to guides {#how-to}

Recipes for one task each, in [how-to/](how-to/):

- [Record a high-rate CSV](how-to/record-a-csv.md), from the CLI or the desktop app
- [Explore a session in the desktop app](how-to/explore-a-session.md): pause, zoom, gaps, replug, keyboard
- [Analyze USB PD traffic](how-to/analyze-pd-traffic.md): capture, filter and read PD messages, see them on the power chart, save and open captures
- [Trigger a charger](how-to/trigger-a-charger.md): detect protocols, request a voltage safely, send PD messages and console commands, read a cable's e-marker
- [Update the meter's firmware](how-to/update-firmware.md) from a ChargerLAB firmware file, from the CLI or the desktop app, and recover from a failed update
- [Set up Linux permissions](linux.md) (udev rule, `powerz` hwmon driver)

## Reference {#reference}

Facts to look up, in [reference/](reference/):

- [`powerz` CLI](reference/cli.md): commands, options, triggering and its safety rules, output formats, exit codes
- [pdcap file format](reference/pdcap.md): saved PD captures, with a worked example
- [`powerz-protocol` API](reference/api/powerz-protocol.md): the library's public modules, PD capture and pdcap included
- [`powerz-pd` API](reference/api/powerz-pd.md): the USB PD message decoder
- [Desktop backend seam](reference/desktop-backend.md): the commands and events between the desktop UI and its backend
- [USB protocol](protocol.md): the meter's wire protocol, with confidence tags

## Explanation {#explanation}

Why the code is shaped the way it is. See
[explanation/architecture.md](explanation/architecture.md).
