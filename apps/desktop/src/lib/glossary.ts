// FR CW66T1: the one glossary every info popover reads. Components pass only a key
// (InfoButton key="section.chart", Term key="term.vbus"); all popover text lives here.

/** One popover's text. */
export interface GlossaryEntry {
  /** The section title or abbreviation the entry explains. */
  readonly term: string;
  /** One short line: the popover's first line and the hover preview. */
  readonly short: string;
  /** An optional longer line shown below the short one. */
  readonly long?: string;
  /** Where the text comes from: "our definition", a docs/protocol.md section or a named spec. */
  readonly source: string;
}

const ENTRIES = {
  "section.meter": {
    term: "Meter readout",
    short: "Live voltage, current and power on the meter's VBUS line, with the data and CC lines.",
    source: "docs/protocol.md#adc-snapshot",
  },
  "section.stats": {
    term: "Stats",
    short:
      "Minimum, average and maximum of each reading, with charge and energy, since the last reset.",
    source: "our definition",
  },
  "section.chart": {
    term: "Chart",
    short:
      "Voltage, current and power over time. Alt-drag or the wheel zooms; double-click fits the session.",
    source: "our definition",
  },
  "section.ruler": {
    term: "Ruler",
    short: "Two cursors on the chart: the time, charge, energy and averages between them.",
    source: "our definition",
  },
  "section.segmentMarks": {
    term: "Segment marks",
    short:
      "Lines where the run reached each step, such as every 10%, of its total charge or energy.",
    source: "our definition",
  },
  "section.ed": {
    term: "E.D.",
    short: "This app's own measure: the energy so far as a percentage of the whole session's.",
    long: "open-powerz defines E.D., not a charging spec; an E.D. in other tools may mean something else.",
    source: "our definition",
  },
  "section.subChart": {
    term: "Sub-chart",
    short: "A second plot under the main one, with its own series, on the same time range.",
    source: "our definition",
  },
  "section.pd": {
    term: "PD analyzer",
    short: "Captures and decodes the USB Power Delivery messages sent on the CC line.",
    source: "docs/protocol.md#pd-capture",
  },
  "section.records": {
    term: "Records",
    short: "Recordings the meter made on its own, which you can read back, export or delete.",
    source: "docs/protocol.md#offline-records",
  },
  "section.trigger": {
    term: "Trigger",
    short: "Asks the charger for a fast-charge voltage. VBUS can rise above 5 V.",
    long: "Only trigger with nothing on the meter's output that cannot take the higher voltage.",
    source: "docs/protocol.md#pdm-text-session-protocol-triggering",
  },
  "section.cable": {
    term: "Cable",
    short:
      "Asks the cable's e-marker for its maker, current and voltage rating, speed and EPR support.",
    source: "USB PD 3.2",
  },
  "section.console": {
    term: "Console",
    short:
      "Sends a raw command line to the meter exactly as typed, such as a PD message for the CC line.",
    long: "A wrong message can make a charger or device misbehave; use it only if you know the protocol.",
    source: "docs/protocol.md#pdm-text-session-protocol-triggering",
  },
  "section.firmware": {
    term: "Firmware",
    short: "Rewrites the meter's firmware with a new image file.",
    long: "Do not unplug the meter while it runs. If an update stops, the meter stays in its boot loader and you can run it again.",
    source: "docs/protocol.md#firmware-download",
  },
  "section.settings": {
    term: "Settings",
    short: "Writes display and sampling settings to the meter, then reads them back to check.",
    source: "docs/protocol.md#settings-write",
  },
  "section.recorder": {
    term: "Recorder",
    short: "Saves the live readings to a CSV file on this computer while it runs.",
    source: "our definition",
  },
  "term.vbus": {
    term: "VBUS",
    short: "The USB-C power line; its voltage is what the charger delivers.",
    source: "USB Type-C 2.3",
  },
  "term.ibus": {
    term: "IBUS",
    short:
      "The current on VBUS through the meter; negative when it flows back from output to input.",
    source: "docs/protocol.md#adc-snapshot",
  },
  "term.sps": {
    term: "SPS",
    short: "Samples per second: the meter's sampling rate, 2, 10, 50 or 1000 SPS.",
    source: "docs/protocol.md#meter-settings-fields",
  },
  "term.pd": {
    term: "PD",
    short: "USB Power Delivery: the protocol a charger and device use to agree on power.",
    source: "USB PD 3.2",
  },
  "term.pps": {
    term: "PPS",
    short:
      "Programmable Power Supply: a PD mode where the device asks for a voltage in 20 mV steps and a current limit.",
    source: "USB PD 3.2",
  },
  "term.epr": {
    term: "EPR",
    short:
      "Extended Power Range: PD above 20 V (28, 36, 48 V fixed or AVS), up to 240 W, on 5 A EPR cables only.",
    source: "USB PD 3.2",
  },
  "term.sop": {
    term: "SOP / SOP' / SOP''",
    short:
      "Who a PD message is for: the port partner (SOP), the cable plug near the VCONN source (SOP') or the far plug (SOP'').",
    source: "USB PD 3.2",
  },
  "term.ed": {
    term: "E.D.",
    short:
      "This app's own energy share: the energy since the session began, in percent of its total.",
    source: "our definition",
  },
  "term.ah": {
    term: "Ah",
    short:
      "Ampere-hours: the charge through the meter since the last reset, summed by this app from IBUS.",
    source: "our definition",
  },
  "term.wh": {
    term: "Wh",
    short:
      "Watt-hours: the energy through the meter since the last reset, summed by this app from power.",
    source: "our definition",
  },
  "term.dataLines": {
    term: "D+ / D−",
    short: "The USB 2.0 data pair. Chargers also use its voltages to signal fast-charge modes.",
    source: "docs/protocol.md#adc-snapshot",
  },
  "term.cc": {
    term: "CC1 / CC2",
    short: "The two USB-C configuration channels: they set the plug's orientation and carry PD.",
    source: "USB Type-C 2.3",
  },
  "term.avs": {
    term: "AVS",
    short:
      "Adjustable Voltage Supply: a PD mode where the device asks for a voltage in 100 mV steps (SPR 9-20 V, EPR 15-48 V).",
    source: "USB PD 3.2",
  },
  "term.spr": {
    term: "SPR",
    short:
      "Standard Power Range: PD at up to 20 V and 100 W; more than 3 A needs a 5 A e-marked cable.",
    source: "USB PD 3.2",
  },
  "term.vconn": {
    term: "VCONN",
    short: "Power sent on the spare CC pin to run the chip inside an e-marked cable.",
    source: "USB Type-C 2.3",
  },
  "term.qc": {
    term: "QC",
    short:
      "Qualcomm Quick Charge: D+ / D− voltages set VBUS; QC 2.0 picks 5, 9, 12 or 20 V, QC 3.0 moves in 200 mV steps.",
    source: "docs/protocol.md#pdm-text-session-protocol-triggering",
  },
  "term.ufcs": {
    term: "UFCS",
    short:
      "Universal Fast Charging Specification: a Chinese industry fast-charge standard that talks over the data lines.",
    source: "docs/protocol.md#pdm-text-session-protocol-triggering",
  },
  "term.afcFcpScp": {
    term: "AFC / FCP / SCP",
    short:
      "Phone makers' fast-charge modes (Samsung AFC, Huawei FCP and SCP), signalled on D+ / D−.",
    source: "docs/protocol.md#pdm-text-session-protocol-triggering",
  },
  "term.bc12": {
    term: "BC1.2",
    short:
      "USB Battery Charging 1.2: how a port shows on D+ / D− that it can give up to 1.5 A, more than plain USB.",
    source: "BC 1.2",
  },
  "term.delta": {
    term: "ΔT / ΔAh / ΔWh",
    short: "The time between the ruler's two cursors, and the charge and energy that flowed in it.",
    source: "our definition",
  },
  "term.emarker": {
    term: "e-marker",
    short:
      "A chip in a USB-C cable that reports its maker, current and voltage rating and speed over PD (SOP').",
    source: "USB PD 3.2",
  },
} as const satisfies Record<string, GlossaryEntry>;

/** A key the UI may pass to an InfoButton or Term. */
export type GlossaryKey = keyof typeof ENTRIES;

/** Every entry typed as a GlossaryEntry, so the optional `long` is readable on any of them. */
export const GLOSSARY: Readonly<Record<GlossaryKey, GlossaryEntry>> = ENTRIES;
