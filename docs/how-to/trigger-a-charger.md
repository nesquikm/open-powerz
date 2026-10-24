# Trigger a charger

The meter can act as a sink and ask a charger for a voltage: a PD offer, a
PPS or AVS voltage, or a QC, FCP, AFC, SFCP, SCP, VFCP, TFCP or UFCS level.
This page shows how to do it in the desktop app's **Trigger** tab and with the
`powerz` CLI. Both talk to the meter over its virtual serial port.

## Safety rules first

A trigger request can raise VBUS well above 5 V (up to 48 V on an EPR
charger). Before you start:

- **Keep the output empty.** Plug the charger into the meter's input and leave
  the output empty, or connect only a device rated for the voltage you ask
  for. Detection can also briefly raise VBUS above 5 V on some chargers.
- **Every request above 5 V needs your confirmation.** The app asks in a
  dialog every time; the CLI refuses unless you pass `--allow-high-voltage`.
  The confirmation covers one request only.
- **Know the way back.** In the app, **Back to 5 V** is always shown while a
  trigger session is open. The CLI returns to 5 V on its own when a command
  ends, on error and on Ctrl-C. Leaving the Trigger tab, disconnecting the
  meter or quitting the app also returns to 5 V and closes the session.
- **Power-cycle the meter when it falls out of step.** Sometimes the meter
  answers a command with the reply to another command. The app and the CLI
  then stop sending and say so. Unplug the meter, plug it back in, and open
  the session again.

## Detect what the charger supports

In the app:

1. Connect the meter and open the **Trigger** tab.
2. Press **Detect**. The app opens the trigger session, checks BC1.2 and
   Apple, runs the meter's automatic protocol list, and then lists each
   protocol. Live measurements pause while the list runs and resume when it
   ends.
3. Read the **Detection** card, then the protocol sections below it. A
   protocol the charger does not answer says "Not supported by this charger."
   If the list stalls, the card says it ended early.

With the CLI:

```bash
powerz detect              # BC1.2, Apple and the D+/D- hint
powerz detect --list       # also the automatic list, one line per protocol
powerz detect --list --json
```

When two checks disagree, the result names which check said what.

## Request a voltage

In the app:

1. Press **Detect** first, so each section shows what the charger offers.
2. In the **USB PD** section, press an offer button for a fixed voltage. For
   a PPS or AVS offer, set the voltage and current with the sliders, then
   press the request button next to them. QC and the other protocols have
   voltage buttons, a QC 3.0 voltage slider, or voltage and current sliders
   inside the charger's limits.
3. For anything above 5 V, a dialog names the voltage. Check the output, then
   press **Confirm**. **Cancel** (or Escape) sends nothing.
4. Read the **Request result** card: the command, the requested and delivered
   values, and the reply. **Live VBUS** comes from the measurement stream.
   The card warns when VBUS is more than 5 % away from the request after 2 s.
   A failed request shows the error and its reason.
5. Press **Back to 5 V** when you are done.

With the CLI, list first, then request by position:

```bash
powerz trigger pd list
powerz trigger pd request 2 --allow-high-voltage --hold 10      # fixed 9 V for 10 s
powerz trigger pd request 9 --voltage-mv 20000 --current-ma 3000 --allow-high-voltage
powerz trigger qc request 9 --allow-high-voltage
powerz trigger ufcs list
```

Without `--hold`, the CLI holds the voltage until you press Ctrl-C. Then it
returns to 5 V and closes the session. A request the charger does not offer
is refused before anything is sent. See the
[CLI reference](../reference/cli.md#protocol-triggering) for every protocol.

## Send PD messages and raw commands

While a trigger session is open, the Trigger tab also shows the
**PD message builder** and the **Console**.

- **PD message builder.** Pick the **Target** (SOP for the charger, SOP' or
  SOP'' for the cable plugs), the **Message type**, and the
  **Data objects** (leave it empty for a control message). Press
  **Send message**. GoodCRC is always refused: the meter sends it itself.
- **Console.** Type a PDM command line and press **Send**. The transcript
  shows each sent and received line with its time; binary capability blocks
  are shown parsed.

The same risk check applies to both. A message or line that could raise VBUS
above 5 V, including any text the app cannot parse, asks for the same
confirmation as a request.

CLI equivalents:

```bash
powerz pd send 0700                    # Get_Source_Cap: PD header, little-endian
printf 'pdm parm\n' | powerz console   # one command line per stdin line
```

## Read the cable's e-marker

Press **Discover cable identity** in the builder, or run:

```bash
powerz pd send --preset discover-identity-cable --allow-high-voltage
```

It sends a Discover Identity request to the cable (SOP'). When the cable
answers, the **Cable identity** card shows its VID, PID, type, maximum voltage
and current, and whether it supports EPR. The preset asks for confirmation
because the risk check does not yet rate cable messages as 5 V.

A cable answers only the side that powers it (the VCONN source). Behind some
chargers it stays silent, and the card keeps saying "No cable has answered
yet."
