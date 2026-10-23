---
title: Desktop trigger panel
milestone: M_X4VEQ3
status: active
archived_at: null
id: fr_01M5KJYD899REQQGB2GZDGSQ6S
created_at: 2026-10-23T05:15:09.449Z
changelog_category: Added
---

# Desktop trigger panel

## Summary

The desktop app gets a trigger panel beside the meter and PD views.
It lists the charger's offers per protocol and lets the user pick a voltage.
The live meter voltage shows how the charger answers.
Every request above 5 V opens a confirmation that names the voltage.
It also reminds the user to keep anything unsafe off the output.
One button returns the charger to 5 V, and leaving the panel or disconnecting closes the session.

## Requirement

The desktop app lets the user detect protocols, read offers and request voltages for every supported protocol, with a per-request confirmation above 5 V, live voltage feedback and a reliable way back to 5 V.

## Acceptance Criteria

- AC-DGSQ6S.1: A Trigger view shows the detection result and one section per protocol, and protocols the charger does not support are marked as such.
- AC-DGSQ6S.2: The PD section lists the decoded offers as buttons, with sliders for PPS and AVS ranges, and shows the PD emulation settings.
- AC-DGSQ6S.3: Every request above 5 V opens a dialog that names the voltage and asks the user to confirm that nothing unsafe is connected to the output. Cancel sends nothing.
- AC-DGSQ6S.4: After a request, the panel shows the reply and the live VBUS from the meter stream, and flags it when VBUS differs from the request by more than 5 percent after 2 s.
- AC-DGSQ6S.5: A Back to 5 V button is always visible while a session is open. Leaving the view, disconnecting, or quitting the app returns to 5 V where the protocol allows it and closes the session.
- AC-DGSQ6S.6: The view works with the keyboard alone, the axe check passes, and it follows the themes.
- AC-DGSQ6S.7: Playwright tests on the mocked backend cover detection, a PD request with the confirmation, cancel, and Back to 5 V. A real-meter drive with the 140 W charger and nothing on the output is recorded in the Notes.

## Technical Design

The backend runs the PDM session in the device worker next to the measurement stream and exposes commands for detect, list, request and reset. The risk classifier decides when the UI must confirm, and the backend refuses unconfirmed high-voltage commands.

## Testing

Worker tests against the simulator, Vitest for the panel state, and Playwright with axe on the mocked backend, one browser project per local run.

## Notes

The live voltage comes from the existing measurement stream; the queue is kept at 50 samples per second or less while triggering, as the vendor advises.
