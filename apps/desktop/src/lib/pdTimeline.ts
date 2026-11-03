/**
 * The PD timeline on the power chart (FR TGE83D), free of the DOM: which kind of marker
 * a PD record gets, the kinds' colours and legend, the markers inside a session time
 * range, and how dense markers merge into counted badges.
 */

import type { PdRow, PdStatusSample } from "./backend";
import type { PdStore } from "./pdStore";
import { THEME_COLORS, type ResolvedTheme } from "./theme";

/** What a marker stands for; its colour says it. */
export type PdMarkerKind = "negotiation" | "reset" | "error" | "attach_detach" | "other";

/** The legend: every kind once, in a fixed order. */
export const PD_MARKER_KINDS: readonly { kind: PdMarkerKind; label: string }[] = [
  { kind: "negotiation", label: "Negotiation" },
  { kind: "reset", label: "Reset" },
  { kind: "error", label: "Error" },
  { kind: "attach_detach", label: "Attach / detach" },
  { kind: "other", label: "Other" },
];

/** Markers closer than this merge into one counted badge, CSS px. */
export const PD_MARKER_MERGE_PX = 4;

/** Radius of a lone marker, CSS px: half the merge distance, so two lone markers that
 * stay apart never overlap. */
export const PD_MARKER_R = PD_MARKER_MERGE_PX / 2;

/** Height of the marker lane over the plot, CSS px. */
export const PD_LANE_PX = 16;

/** Half the drawn width of a cluster, CSS px: a lone marker's radius, or its count
 * badge's (about 6 px per digit of bold 10 px text, plus padding). */
export function pdClusterHalfWidth(c: Pick<PdMarkerCluster, "count">): number {
  if (c.count === 1) return PD_MARKER_R;
  return Math.max(PD_LANE_PX / 2 - 1, 3 * String(c.count).length + 4);
}

/** The messages of a power negotiation (SPR and EPR). */
const NEGOTIATION = new Set([
  "Source_Capabilities",
  "Sink_Capabilities",
  "Get_Source_Cap",
  "Get_Sink_Cap",
  "Request",
  "Accept",
  "Reject",
  "Wait",
  "PS_RDY",
  "EPR_Request",
  "EPR_Source_Capabilities",
  "EPR_Sink_Capabilities",
  "EPR_Mode",
  "EPR_KeepAlive",
  "EPR_KeepAlive_Ack",
]);

/** The marker kind of a record. */
export function markerKind(row: Pick<PdRow, "name" | "kind">): PdMarkerKind {
  const { name } = row;
  if (row.kind === "event") {
    if (name === "Attach" || name === "Detach") return "attach_detach";
    if (name === "Hard Reset" || name === "Cable Reset") return "reset";
    if (name.startsWith("Receive Error") || name === "Bad Message ID") return "error";
    return "other";
  }
  if (name === "Soft_Reset") return "reset";
  if (NEGOTIATION.has(name)) return "negotiation";
  return "other";
}

/** Each kind's marker colour in `theme`, from the theme tokens. */
export function markerColors(theme: ResolvedTheme): Record<PdMarkerKind, string> {
  return THEME_COLORS[theme].pdMarker;
}

/** One record on the chart. */
export interface PdMarker {
  /** Record index. */
  index: number;
  /** Session time, ms. */
  ms: number;
  kind: PdMarkerKind;
}

/** The markers of the records at session times `t0Ms..=t1Ms`, in capture order. A
 * store whose times never go back is binary-searched, so a draw costs the markers in
 * view, not the whole capture. */
export function markersInRange(store: PdStore, t0Ms: number, t1Ms: number): PdMarker[] {
  const out: PdMarker[] = [];
  let lo = 0;
  let hi = store.length;
  if (store.sessionSorted) {
    lo = sessionBound(store, t0Ms, false);
    hi = sessionBound(store, t1Ms, true);
  }
  for (let i = lo; i < hi; i++) {
    const ms = store.sessionMs(i);
    if (!(ms >= t0Ms && ms <= t1Ms)) continue;
    out.push({
      index: store.indexAt(i),
      ms,
      kind: markerKind({ name: store.name(i), kind: store.isEvent(i) ? "event" : "message" }),
    });
  }
  return out;
}

/** First store position with session time `>= x` (or `> x` when `after`), in a store
 * whose session times are sorted. */
function sessionBound(store: PdStore, x: number, after: boolean): number {
  let lo = 0;
  let hi = store.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const ms = store.sessionMs(mid);
    if (ms < x || (after && ms === x)) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** One marker on screen, or a badge for several. */
export interface PdMarkerCluster {
  /** Position of the first marker, px. */
  px: number;
  /** Markers in it. */
  count: number;
  /** Its markers, in order. */
  markers: PdMarker[];
}

/** Merges markers closer than {@link PD_MARKER_MERGE_PX} (by `toPx`) into clusters, at
 * most `cap` of them: it stops at the marker that would start one more (AC-C6VR9K.5). */
export function clusterMarkers(
  markers: readonly PdMarker[],
  toPx: (ms: number) => number,
  cap = Infinity,
): PdMarkerCluster[] {
  const out: PdMarkerCluster[] = [];
  let current: PdMarkerCluster | null = null;
  let lastPx = -Infinity;
  for (const m of markers) {
    const px = toPx(m.ms);
    // A marker joins the cluster when it is closer than the merge distance to the last
    // marker, or when its dot would land inside the cluster's drawn badge.
    if (
      current !== null &&
      (px - lastPx < PD_MARKER_MERGE_PX ||
        px - PD_MARKER_R < current.px + pdClusterHalfWidth(current))
    ) {
      current.markers.push(m);
      current.count++;
    } else {
      if (out.length >= cap) break;
      current = { px, count: 1, markers: [m] };
      out.push(current);
    }
    lastPx = px;
  }
  return out;
}

/** How far from a lone marker's centre a click still hits it, CSS px. */
export const PD_MARKER_HIT_PX = 6;

/** Half the width of a cluster's hit area: a lone marker's, or its count badge's. */
function hitHalfWidth(c: PdMarkerCluster): number {
  if (c.count === 1) return PD_MARKER_HIT_PX;
  return Math.max(PD_MARKER_HIT_PX + 2, pdClusterHalfWidth(c) + 1);
}

/** The cluster under `px`, or null (FR TGE83D, AC .2): the nearest one whose marker or
 * badge covers `px`. */
export function hitMarker(
  clusters: readonly PdMarkerCluster[],
  px: number,
): PdMarkerCluster | null {
  let best: PdMarkerCluster | null = null;
  let bestDistance = Infinity;
  for (const c of clusters) {
    const distance = Math.abs(c.px - px);
    if (distance <= hitHalfWidth(c) && distance < bestDistance) {
      best = c;
      bestDistance = distance;
    }
  }
  return best;
}

/** A visible time range. */
export interface PdTimeView {
  t0Ms: number;
  t1Ms: number;
}

/** The view that shows `ms` (FR TGE83D, AC .3): `view` itself when `ms` is already in
 * it, else the same span centred on `ms`, kept inside the session. */
export function revealTime(
  view: PdTimeView,
  ms: number,
  session: { firstMs: number; lastMs: number },
): PdTimeView {
  if (ms >= view.t0Ms && ms <= view.t1Ms) return view;
  const span = view.t1Ms - view.t0Ms;
  let t0 = ms - span / 2;
  if (t0 + span > session.lastMs) t0 = session.lastMs - span;
  if (t0 < session.firstMs) t0 = session.firstMs;
  return { t0Ms: t0, t1Ms: t0 + span };
}

/** Status-block columns for a time range. */
export interface PdStatusColumns {
  t: number[];
  vbus: number[];
  ibus: number[];
  cc1: number[];
  cc2: number[];
}

/** Status samples a {@link PdStatusBuffer} keeps by default (FR AXHK2W, AC .2): as
 * many as the backend's status log and the PD table's rows. */
export const PD_STATUS_MAX_SAMPLES = 100_000;

/** The notice for `dropped` oldest status samples gone past the cap, or null when
 * nothing was dropped (FR AXHK2W, AC .2). */
export function pdStatusDroppedText(dropped: number): string | null {
  if (dropped <= 0) return null;
  const n = dropped.toLocaleString("en-US");
  const cap = PD_STATUS_MAX_SAMPLES.toLocaleString("en-US");
  const what = dropped === 1 ? "status sample was" : "status samples were";
  return `The oldest ${n} ${what} dropped: the chart keeps the newest ${cap}.`;
}

/** The PD status blocks of a session, for plotting (FR TGE83D, AC .4), at most
 * `maxSamples` of them: past it the oldest go first and are counted (FR AXHK2W, AC .2). */
export class PdStatusBuffer {
  #t: number[] = [];
  #vbus: number[] = [];
  #ibus: number[] = [];
  #cc1: number[] = [];
  #cc2: number[] = [];
  /** Index of the oldest kept sample in the columns: those before it were dropped and
   * are compacted away now and then, not on every append. */
  #start = 0;
  #dropped = 0;
  /** The backend's own count of samples it dropped since the last clear (AC-AXHK2W.3). */
  #backendDropped = 0;
  readonly #max: number;

  constructor(options: { maxSamples?: number } = {}) {
    this.#max = Math.max(0, options.maxSamples ?? PD_STATUS_MAX_SAMPLES);
  }

  get length(): number {
    return this.#t.length - this.#start;
  }

  /** Samples dropped past the cap since the last clear: the larger of this buffer's
   * own count and the backend's, which describe the same oldest samples. */
  get dropped(): number {
    return Math.max(this.#dropped, this.#backendDropped);
  }

  /** Takes the backend's count of samples dropped since the last clear (AC-AXHK2W.3). */
  noteBackendDropped(n: number): void {
    if (Number.isFinite(n) && n > this.#backendDropped) this.#backendDropped = n;
  }

  /** Appends samples in time order, dropping the oldest past the cap. */
  append(samples: readonly PdStatusSample[]): void {
    for (const s of samples) {
      this.#t.push(s.session_ms);
      this.#vbus.push(s.vbus_v);
      this.#ibus.push(s.ibus_a);
      this.#cc1.push(s.cc1_v);
      this.#cc2.push(s.cc2_v);
    }
    const excess = this.length - this.#max;
    if (excess > 0) {
      this.#start += excess;
      this.#dropped += excess;
    }
    // Compact once the dropped part outgrows the kept part: amortized O(1) a sample.
    if (this.#start > 0 && this.#start >= this.length) {
      const k = this.#start;
      this.#t = this.#t.slice(k);
      this.#vbus = this.#vbus.slice(k);
      this.#ibus = this.#ibus.slice(k);
      this.#cc1 = this.#cc1.slice(k);
      this.#cc2 = this.#cc2.slice(k);
      this.#start = 0;
    }
  }

  /** The buffer's own columns, not copied: read them only between
   * {@link bounds}' indices, and never write to them. */
  get columns(): Readonly<{ [K in keyof PdStatusColumns]: readonly number[] }> {
    return { t: this.#t, vbus: this.#vbus, ibus: this.#ibus, cc1: this.#cc1, cc2: this.#cc2 };
  }

  /** The `[lo, hi)` indices of the samples with `t0Ms <= session_ms <= t1Ms` in
   * {@link columns}. */
  bounds(t0Ms: number, t1Ms: number): [number, number] {
    const lo = lowerBound(this.#t, t0Ms, false, this.#start);
    return [lo, Math.max(lo, lowerBound(this.#t, t1Ms, true, this.#start))];
  }

  /** The samples with `t0Ms <= session_ms <= t1Ms`, copied. */
  range(t0Ms: number, t1Ms: number): PdStatusColumns {
    const [lo, hi] = this.bounds(t0Ms, t1Ms);
    return {
      t: this.#t.slice(lo, hi),
      vbus: this.#vbus.slice(lo, hi),
      ibus: this.#ibus.slice(lo, hi),
      cc1: this.#cc1.slice(lo, hi),
      cc2: this.#cc2.slice(lo, hi),
    };
  }

  clear(): void {
    this.#t = [];
    this.#vbus = [];
    this.#ibus = [];
    this.#cc1 = [];
    this.#cc2 = [];
    this.#start = 0;
    this.#dropped = 0;
    this.#backendDropped = 0;
  }
}

/** First index from `from` with `a[i] >= x` (or `> x` when `after`) in the sorted `a`. */
function lowerBound(a: readonly number[], x: number, after = false, from = 0): number {
  let lo = from;
  let hi = a.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (a[mid]! < x || (after && a[mid] === x)) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
