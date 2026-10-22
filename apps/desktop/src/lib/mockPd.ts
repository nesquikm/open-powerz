/**
 * The mock backend's PD capture (FR TGE83C): a scripted PD conversation like the
 * simulator's negotiation scenario, played in a loop for as long as the capture runs.
 * Each cycle has an attach, an SPR negotiation with a GoodCRC after every message, a
 * Discover Identity to the cable on SOP' (first, as sources do to
 * learn the cable's rating) and a Hard Reset. Record `k` of the endless
 * script is computed on demand, so a 100,000-record seed costs one number per record.
 * All values are synthetic.
 */

import type { PdDetails, PdDirection, PdField, PdRow } from "./backend";

/** One cycle of the script, ms. */
export const PD_CYCLE_MS = 800;

type Sender = "source" | "sink" | "cable";

interface MessageStep {
  at: number;
  kind: "message";
  /** Meter SOP code: 0 SOP, 1 SOP'. */
  sop: 0 | 1;
  sender: Sender;
  /** Data message (with objects) or control message code. */
  type: number;
  name: string;
  objects: number[];
  /** For a GoodCRC: the step it acknowledges (its MessageID is reused). */
  acks?: number;
}

interface EventStep {
  at: number;
  kind: "event";
  name: string;
  /** Event code of the meter's record. */
  code: number;
}

type Step = MessageStep | EventStep;

/** A Fixed Supply PDO of `mv` / `ma`. */
function fixedPdo(mv: number, ma: number): number {
  return (((mv / 50) & 0x3ff) << 10) | ((ma / 10) & 0x3ff);
}

/** A Fixed RDO for object `position`, `ma` operating and maximum. */
function fixedRdo(position: number, ma: number): number {
  return ((position & 0xf) << 28) | (((ma / 10) & 0x3ff) << 10) | ((ma / 10) & 0x3ff);
}

const PDOS = [
  fixedPdo(5000, 3000),
  fixedPdo(9000, 3000),
  fixedPdo(15000, 3000),
  fixedPdo(20000, 3000),
];

/** Discover Identity REQ to the PD SID, structured VDM version 2.0. */
const DISCOVER_IDENTITY = ((0xff00 << 16) | (1 << 15) | (1 << 13) | 1) >>> 0;

const SCRIPT: Step[] = [
  { at: 0, kind: "event", name: "Attach", code: 1 },
  {
    at: 250,
    kind: "message",
    sop: 1,
    sender: "source",
    type: 15,
    name: "Vendor_Defined",
    objects: [DISCOVER_IDENTITY],
  },
  {
    at: 251,
    kind: "message",
    sop: 1,
    sender: "cable",
    type: 1,
    name: "GoodCRC",
    objects: [],
    acks: 1,
  },
  {
    at: 280,
    kind: "message",
    sop: 0,
    sender: "source",
    type: 1,
    name: "Source_Capabilities",
    objects: PDOS,
  },
  {
    at: 281,
    kind: "message",
    sop: 0,
    sender: "sink",
    type: 1,
    name: "GoodCRC",
    objects: [],
    acks: 3,
  },
  {
    at: 290,
    kind: "message",
    sop: 0,
    sender: "sink",
    type: 2,
    name: "Request",
    objects: [fixedRdo(2, 3000)],
  },
  {
    at: 291,
    kind: "message",
    sop: 0,
    sender: "source",
    type: 1,
    name: "GoodCRC",
    objects: [],
    acks: 5,
  },
  { at: 300, kind: "message", sop: 0, sender: "source", type: 3, name: "Accept", objects: [] },
  {
    at: 301,
    kind: "message",
    sop: 0,
    sender: "sink",
    type: 1,
    name: "GoodCRC",
    objects: [],
    acks: 7,
  },
  { at: 420, kind: "message", sop: 0, sender: "source", type: 6, name: "PS_RDY", objects: [] },
  {
    at: 421,
    kind: "message",
    sop: 0,
    sender: "sink",
    type: 1,
    name: "GoodCRC",
    objects: [],
    acks: 9,
  },
  { at: 700, kind: "event", name: "Hard Reset", code: 3 },
];

/** MessageID of every message step: a counter per SOP and sender, from 0 each cycle
 * (the Hard Reset at the end of a cycle resets them); a GoodCRC repeats the ID it
 * acknowledges. */
const MESSAGE_IDS: number[] = (() => {
  const counters = new Map<string, number>();
  const ids: number[] = [];
  SCRIPT.forEach((step, i) => {
    if (step.kind !== "message") {
      ids.push(0);
      return;
    }
    if (step.acks !== undefined) {
      ids.push(ids[step.acks] ?? 0);
      return;
    }
    const key = `${step.sop}:${step.sender}`;
    const id = counters.get(key) ?? 0;
    counters.set(key, (id + 1) % 8);
    ids[i] = id;
  });
  return ids;
})();

/** Ms since the capture start of script record `k`. */
export function pdRecordTime(k: number): number {
  const step = SCRIPT[k % SCRIPT.length]!;
  return Math.floor(k / SCRIPT.length) * PD_CYCLE_MS + step.at;
}

function stepOf(k: number): { step: Step; pos: number } {
  const pos = k % SCRIPT.length;
  return { step: SCRIPT[pos]!, pos };
}

/** The 16-bit header of message step `pos`. */
function header(step: MessageStep, pos: number): number {
  const id = MESSAGE_IDS[pos] ?? 0;
  const bit8 = step.sop === 0 ? step.sender === "source" : step.sender === "cable";
  const dataRole = step.sop === 0 && step.sender === "source";
  return (
    ((step.objects.length & 7) << 12) |
    ((id & 7) << 9) |
    (bit8 ? 1 << 8 : 0) |
    (2 << 6) |
    (dataRole ? 1 << 5 : 0) |
    (step.type & 0x1f)
  );
}

function volts(mv: number): string {
  return `${(mv / 1000).toFixed(2)} V`;
}

function amps(ma: number): string {
  return `${(ma / 1000).toFixed(2)} A`;
}

function bits(word: number, hi: number, lo: number): number {
  return Math.floor(word / 2 ** lo) % 2 ** (hi - lo + 1);
}

function direction(step: MessageStep): PdDirection {
  if (step.sop !== 0) return "cable";
  return step.sender === "source" ? "source_to_sink" : "sink_to_source";
}

function summary(step: Step): string {
  if (step.kind === "event") return step.name;
  switch (step.name) {
    case "Source_Capabilities":
      return `${step.name}: ${step.objects
        .map((p) => `Fixed ${volts(bits(p, 19, 10) * 50)} ${amps(bits(p, 9, 0) * 10)}`)
        .join(", ")}`;
    case "Request": {
      const rdo = step.objects[0] ?? 0;
      return `${step.name}: Fixed #${bits(rdo, 31, 28)}, ${amps(bits(rdo, 19, 10) * 10)}`;
    }
    case "Vendor_Defined":
      return `${step.name}: Discover Identity REQ (PD SID)`;
    default:
      return step.name;
  }
}

/** VBUS and IBUS at script position `pos`: 5 V until the PS_RDY, 9 V after it. */
function power(pos: number): { vbus_v: number; ibus_a: number } {
  return pos >= 7 && pos < 11 ? { vbus_v: 9.02, ibus_a: 1.5 } : { vbus_v: 5.01, ibus_a: 0.05 };
}

/** The table row of script record `k` as record `index` of the capture. */
export function pdRow(k: number, index: number): PdRow {
  const { step, pos } = stepOf(k);
  const base = { index, elapsed_ms: pdRecordTime(k), ...power(pos), summary: summary(step) };
  if (step.kind === "event") {
    return {
      ...base,
      kind: "event",
      sop: null,
      name: step.name,
      message_id: null,
      direction: null,
      objects: null,
      spec_revision: null,
    };
  }
  return {
    ...base,
    kind: "message",
    sop: step.sop === 0 ? "SOP" : "SOP'",
    name: step.name,
    message_id: MESSAGE_IDS[pos] ?? 0,
    direction: direction(step),
    objects: step.objects.length,
    spec_revision: "3.x",
  };
}

/** Field `name` of bits `hi..=lo` of the word at byte `word`. */
function field(
  name: string,
  value: number,
  hi: number,
  lo: number,
  word: number,
  meaning: string,
): PdField {
  return {
    name,
    meaning,
    raw: bits(value, hi, lo),
    bytes: [word + Math.floor(lo / 8), word + Math.floor(hi / 8) + 1],
    children: [],
  };
}

function headerField(h: number, step: MessageStep): PdField {
  const cable = step.sop !== 0;
  return {
    name: "Header",
    meaning: `0x${h.toString(16).toUpperCase().padStart(4, "0")} ${step.name}`,
    raw: h,
    bytes: [0, 2],
    children: [
      field("Extended", h, 15, 15, 0, "No"),
      field("Number of Data Objects", h, 14, 12, 0, String(bits(h, 14, 12))),
      field("MessageID", h, 11, 9, 0, String(bits(h, 11, 9))),
      cable
        ? field("Cable Plug", h, 8, 8, 0, bits(h, 8, 8) ? "Cable Plug" : "Port")
        : field("Port Power Role", h, 8, 8, 0, bits(h, 8, 8) ? "Source" : "Sink"),
      field("Specification Revision", h, 7, 6, 0, "Revision 3.x"),
      cable
        ? field("Reserved", h, 5, 5, 0, "0x0")
        : field("Port Data Role", h, 5, 5, 0, bits(h, 5, 5) ? "DFP" : "UFP"),
      field("Message Type", h, 4, 0, 0, step.name),
    ],
  };
}

function objectField(step: MessageStep, obj: number, i: number): PdField {
  const at = 2 + 4 * i;
  const node = (name: string, children: PdField[]): PdField => ({
    name,
    meaning: `0x${(obj >>> 0).toString(16).toUpperCase().padStart(8, "0")}`,
    raw: obj >>> 0,
    bytes: [at, at + 4],
    children,
  });
  switch (step.name) {
    case "Source_Capabilities":
      return node(`PDO ${i + 1}`, [
        field("PDO Type", obj, 31, 30, at, "Fixed Supply"),
        field("Voltage", obj, 19, 10, at, volts(bits(obj, 19, 10) * 50)),
        field("Maximum Current", obj, 9, 0, at, amps(bits(obj, 9, 0) * 10)),
      ]);
    case "Request":
      return node("RDO", [
        field("Object Position", obj, 31, 28, at, String(bits(obj, 31, 28))),
        field("Operating Current", obj, 19, 10, at, amps(bits(obj, 19, 10) * 10)),
        field("Maximum Operating Current", obj, 9, 0, at, amps(bits(obj, 9, 0) * 10)),
      ]);
    default:
      return node("VDM Header", [
        field("SVID", obj, 31, 16, at, "0xFF00 (PD SID)"),
        field("VDM Type", obj, 15, 15, at, "Structured"),
        field("Command", obj, 4, 0, at, "Discover Identity"),
      ]);
  }
}

/** Raw bytes and the field tree of script record `k` as record `index`. */
export function pdDetailsOf(k: number, index: number): PdDetails {
  const { step, pos } = stepOf(k);
  if (step.kind === "event") {
    const t = pdRecordTime(k);
    return {
      index,
      raw: [0x05, t & 0xff, (t >>> 8) & 0xff, (t >>> 16) & 0xff, (t >>> 24) & 0xff, step.code],
      fields: [{ name: "Event", meaning: step.name, raw: step.code, bytes: [5, 6], children: [] }],
    };
  }
  const h = header(step, pos);
  const raw = [h & 0xff, h >>> 8];
  for (const o of step.objects) {
    raw.push(o & 0xff, (o >>> 8) & 0xff, (o >>> 16) & 0xff, (o >>> 24) & 0xff);
  }
  return {
    index,
    raw,
    fields: [headerField(h, step), ...step.objects.map((o, i) => objectField(step, o, i))],
  };
}
