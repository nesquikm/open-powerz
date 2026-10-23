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
- AC-DGSQ6S.8: The QC, QC 3.0, FCP, AFC, SFCP, SCP, VFCP, TFCP and UFCS sections offer request controls for what the charger reports (fixed-voltage buttons, a QC 3.0 voltage control, voltage and current inside the decoded limits, UFCS by capability), with the same confirmation above 5 V, live VBUS check and Back to 5 V as the PD section.
- AC-DGSQ6S.9: A request that fails (an error reply, a timeout or a refusal) shows the failure and its reason in the request result; after a confirmed request the panel never keeps saying "No request yet".
- AC-DGSQ6S.10: The session recognises a reply that belongs to another command (for example `req error` answering `pdm parm`) and never takes it as the answer to the current command. It stops with an error saying the meter needs a power cycle, and the panel shows that error and disables requests until the session is reopened.
- AC-DGSQ6S.11: While the desktop runs the automatic list, the measurement stream pauses polling without reconnecting the meter, and resumes when the list ends, also when it ends early or with an error.
- AC-DGSQ6S.12: After polling resumes, the measurement stream survives the meter's backlog: a queue reply shorter than it declares keeps its whole records, the lost samples are reported as a gap, and the stream and the trigger session carry on.
- AC-DGSQ6S.13: The session tracks which protocol is entered; a request or PD message for another protocol (PD, QC, UFCS or any other) enters its own protocol again first, refreshing what the charger offers, in the app and the command line tool alike.

## Technical Design

The PDM session runs beside the measurement stream, serving detect, list, request and reset. The risk classifier decides what needs confirming; the backend refuses unconfirmed high-voltage commands. An open session caps the queue at 50 SPS and pauses PD capture. A bulk connect (it silences PDM), a dropped meter or a reloaded page first returns to 5 V and closes the session. PD contracts always end through PD. Other protocols get fixed-voltage buttons, a QC 3.0 control, or voltage/current controls clamped to their block's limits and power. Out of sync, a safety path still writes `pd hrst` or `reset` blind, then `pdm close`; stream polling is held during the list and always released.

## Testing

Worker tests against the simulator, Vitest for the panel state, and Playwright with axe on the mocked backend, one browser project per local run.

## Notes

Hardware 2026-10-23, Apple 140 W via the 240 W cable, output empty: the panel's backend drove the real meter (bulk first, then the trigger session). Unconfirmed 9 V was refused; 9 V read 8.869 V, 28 V 28.009 V; Back to 5 V 5.19 V. The UI ran on the mocked backend; a by-hand click-through is open.
