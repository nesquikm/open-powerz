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
  dialog; the CLI refuses unless you pass `--allow-high-voltage`. The
  confirmation covers one request only, unless you turn on
  [Don't ask again](#dont-ask-again).
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
   press **Confirm**. **Cancel** (or Escape) sends nothing. The dialog does not
   appear for a request that [Don't ask again](#dont-ask-again) covers.
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

## Don't ask again

The dialog for a PD request can offer a checkbox: **Don't ask again until I go
back to 5 V or disconnect**. It starts unchecked every time. Check it and press
**Confirm**, and the app sends that request and stops asking for the next ones.

**What it covers.** USB PD fixed, PPS and SPR AVS offers. It stops at 20 V.

**What always asks.** The dialog still appears for:

- a request above 20 V, or at an unknown voltage
- an EPR position, at any voltage
- a battery or variable offer
- QC, FCP, UFCS and every other protocol that is not PD
- built PD messages and Console lines above 5 V, or of unknown effect

**What ends it.** While it is on, the panel shows **Not asking up to 20 V**
with a **Clear** button. Press **Clear** to end it at once. These end it too:

- **Back to 5 V**, or any request at or below 5 V
- a hard reset
- a request that fails after something was sent
- **Detect**
- any built PD message, or a **Read cable** press
- any Console line that is not read-only
- a change in what the charger offers
- a request that does not qualify, once you confirm it
- the charger going away: VBUS below 2 V once a contract was up
- closing the session: leaving the Trigger tab, disconnecting, losing the meter,
  reloading the page or quitting the app

A reload closes the trigger session and returns the charger to 5 V. A new
session always starts with it off.

If the offers change, a request that names a changed offer is refused and not
sent. Press **Detect** to see the new offers.

If the charger goes away, the app ends the option at once, forgets the offers
and says so. No request goes out, even if you plug the same charger back in, and
**Back to 5 V** sends a hard reset, since no offers are left to ask for. Press
**Detect** to go on. The check reads VBUS ten times a second, so a swap faster
than that could go unseen: end the option yourself before you swap chargers.

Leaving the Trigger tab closes the session, so the offers go too. Press
**Detect** when you come back.

**The CLI does not change.** `powerz` has no such option. Each command above
5 V still needs `--allow-high-voltage`, and nothing carries over to the next
command.

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

## Read a cable

The **Cable** card shows what a cable's e-marker says: vendor, VID, PID, maximum
current and voltage, USB speed, active or passive, and EPR.

1. Press a PD offer first. **Read cable** never enters a session, so it needs
   one that is already open. **Detect** alone is not enough. The 5 V offer will
   do, and it needs no dialog.
2. Press **Read cable**. One press sends one message. It never retries.

**No dialog.** That one message cannot raise VBUS, so the button does not ask.
Every other data message to a cable still asks, and so do the same bytes typed in the
Console.

**No answer.** The cable may have no e-marker, or the meter may not be the VCONN
source. Nothing is retried on its own. Press the button again to try once more.

**Unknown vendor.** A vendor missing from our table shows as its hex VID.

**No session.** With no session open, the card shows an identity held in a PD
capture, marked "Seen in the capture." It sends nothing.

The demo device answers with a sample cable.

## Read the cable's e-marker from the builder

Press **Discover cable identity** in the builder, or run:

```bash
powerz pd send --preset discover-identity-cable --allow-high-voltage
```

It sends a Discover Identity request to the cable (SOP'). When the cable
answers, the **Cable identity** card shows its VID, PID, type, maximum voltage
and current, and whether it supports EPR. The app sends these exact bytes with
no dialog. The CLI still needs `--allow-high-voltage`.

A cable answers only the side that powers it (the VCONN source). Behind some
chargers it stays silent, and the card keeps saying "No cable has answered
yet."
