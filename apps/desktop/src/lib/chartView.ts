import type { Series, SeriesBuckets } from "./backend";
import { LIVE_WINDOWS, type LiveWindowId } from "./chartData";
import { formatSessionTime } from "./chartSeries";

/** The chart never redraws more often than this, whatever the display's refresh rate. */
export const MAX_REDRAW_HZ = 60;

/** What the chart shows: the scrolling live window, or a fixed range of the history. */
export type ChartView =
  | { mode: "live"; window: LiveWindowId }
  | { mode: "history"; window: LiveWindowId; t0Ms: number; t1Ms: number };

/** A `requestAnimationFrame` source (injected so tests can drive frames by hand). */
export interface FrameClock {
  request(cb: (ts: number) => void): number;
  cancel(id: number): void;
}

/** The browser's animation frames. */
export const animationFrames: FrameClock = {
  request: (cb) => requestAnimationFrame(cb),
  cancel: (id) => cancelAnimationFrame(id),
};

/** Slack for frame-timestamp jitter, so a 60 Hz display still draws every frame. */
const JITTER_MS = 1;

/** Draws on an animation frame after `invalidate()`, coalescing invalidations and
 * keeping at least 1/MAX_REDRAW_HZ between draws. */
export class RedrawScheduler {
  #draw: () => void;
  #clock: FrameClock;
  #pending: number | null = null;
  #lastDrawTs: number | null = null;
  #stopped = false;

  constructor(draw: () => void, clock: FrameClock = animationFrames) {
    this.#draw = draw;
    this.#clock = clock;
  }

  /** Something changed: draw on a coming frame. */
  invalidate(): void {
    if (this.#stopped || this.#pending !== null) return;
    this.#pending = this.#clock.request((ts) => this.#onFrame(ts));
  }

  /** No more draws (the chart is gone). */
  stop(): void {
    this.#stopped = true;
    if (this.#pending !== null) this.#clock.cancel(this.#pending);
    this.#pending = null;
  }

  #onFrame(ts: number): void {
    this.#pending = null;
    if (this.#stopped) return;
    const minGap = 1000 / MAX_REDRAW_HZ - JITTER_MS;
    if (this.#lastDrawTs !== null && ts - this.#lastDrawTs < minGap) {
      // Too soon: still dirty, try the next frame.
      this.#pending = this.#clock.request((t) => this.#onFrame(t));
      return;
    }
    this.#lastDrawTs = ts;
    this.#draw();
  }
}

/** A user gesture on the chart (AC-AH2AEM.3). */
export type ChartGesture =
  | { kind: "select"; t0Ms: number; t1Ms: number }
  | { kind: "wheel"; atMs: number; deltaY: number }
  | { kind: "pan"; deltaMs: number }
  | { kind: "key"; key: string }
  | { kind: "pause" }
  | { kind: "live" };

/** A time range in ms. */
export interface TimeRange {
  t0Ms: number;
  t1Ms: number;
}

/** The session's extent in ms (first and newest sample). */
export interface SessionExtent {
  firstMs: number;
  lastMs: number;
}

/** One zoom step (wheel notch, arrow key) shrinks or grows the span by this factor. */
const ZOOM_STEP = 0.8;
/** One arrow-key pan step moves the view by this fraction of its span. */
const PAN_STEP = 0.1;
/** The narrowest view, in ms. */
const MIN_SPAN_MS = 1;

/** Zooms `shown` by `factor` keeping `atMs` at the same fraction of the view; never
 * wider than the session. */
function zoom(shown: TimeRange, atMs: number, factor: number, session: SessionExtent): TimeRange {
  const span = shown.t1Ms - shown.t0Ms;
  const maxSpan = Math.max(MIN_SPAN_MS, session.lastMs - session.firstMs);
  const next = Math.min(maxSpan, Math.max(MIN_SPAN_MS, span * factor));
  const frac = span > 0 ? (atMs - shown.t0Ms) / span : 0.5;
  const t0Ms = atMs - frac * next;
  return { t0Ms, t1Ms: t0Ms + next };
}

/** `r` moved (never resized, unless wider than the session) to lie inside the session,
 * so a pan cannot scroll into empty time before the first or after the newest sample. */
function clampToSession(r: TimeRange, session: SessionExtent): TimeRange {
  const { firstMs, lastMs } = session;
  if (!(lastMs > firstMs)) return r;
  const span = r.t1Ms - r.t0Ms;
  if (span >= lastMs - firstMs) return { t0Ms: firstMs, t1Ms: lastMs };
  if (r.t0Ms < firstMs) return { t0Ms: firstMs, t1Ms: firstMs + span };
  if (r.t1Ms > lastMs) return { t0Ms: lastMs - span, t1Ms: lastMs };
  return r;
}

/** The view after `gesture`: every zoom or pan gesture leaves live mode for a fixed
 * history range inside the session, the Live gesture returns to the remembered live
 * window. */
export function applyGesture(
  view: ChartView,
  gesture: ChartGesture,
  shown: TimeRange,
  session: SessionExtent,
): ChartView {
  const history = (r: TimeRange): ChartView => {
    const { t0Ms, t1Ms } = clampToSession(r, session);
    return { mode: "history", window: view.window, t0Ms, t1Ms };
  };
  const span = shown.t1Ms - shown.t0Ms;
  const mid = shown.t0Ms + span / 2;
  switch (gesture.kind) {
    case "live":
      // Already live: the same view, so nothing redraws.
      return view.mode === "live" ? view : { mode: "live", window: view.window };
    case "pause":
      // Freezes exactly the range on screen (AC-DDDYDJ.2); already paused, unchanged.
      if (view.mode === "history") return view;
      return { mode: "history", window: view.window, t0Ms: shown.t0Ms, t1Ms: shown.t1Ms };
    case "select": {
      // A selection is clipped to the session, never shifted.
      const t0Ms = Math.max(session.firstMs, Math.min(gesture.t0Ms, gesture.t1Ms));
      const t1Ms = Math.min(session.lastMs, Math.max(gesture.t0Ms, gesture.t1Ms));
      if (t1Ms - t0Ms < MIN_SPAN_MS) return view;
      return history({ t0Ms, t1Ms });
    }
    case "wheel":
      if (gesture.deltaY === 0) return view;
      return history(
        zoom(shown, gesture.atMs, gesture.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP, session),
      );
    case "pan":
      return history({ t0Ms: shown.t0Ms + gesture.deltaMs, t1Ms: shown.t1Ms + gesture.deltaMs });
    case "key":
      switch (gesture.key) {
        case "ArrowLeft":
          return history({
            t0Ms: shown.t0Ms - span * PAN_STEP,
            t1Ms: shown.t1Ms - span * PAN_STEP,
          });
        case "ArrowRight":
          return history({
            t0Ms: shown.t0Ms + span * PAN_STEP,
            t1Ms: shown.t1Ms + span * PAN_STEP,
          });
        case "ArrowUp":
          return history(zoom(shown, mid, ZOOM_STEP, session));
        case "ArrowDown":
          return history(zoom(shown, mid, 1 / ZOOM_STEP, session));
        case "End":
          // End returns to live (AC-DDDYDJ.5); already live, unchanged.
          return view.mode === "live" ? view : { mode: "live", window: view.window };
        default:
          return view;
      }
  }
}

/** A live-window button shows pressed only while that window scrolls live. */
export function windowPressed(view: ChartView, id: LiveWindowId): boolean {
  return view.mode === "live" && view.window === id;
}

/** What the Live button shows for a view: pressed and reading "Live" while the chart
 * follows the newest data (AC-DDDYDJ.1). */
export function liveButton(view: ChartView): { label: string; pressed: boolean; title: string } {
  if (view.mode === "live") {
    return {
      label: "Live",
      pressed: true,
      title: "Pause the chart on this range; the stream and any recording keep running",
    };
  }
  return {
    label: "Paused · Go live",
    pressed: false,
    title: "Return the chart to live: press this button or End",
  };
}

/** The gesture the Live button sends: pause while live, go live while paused
 * (AC-DDDYDJ.2). */
export function liveToggle(view: ChartView): ChartGesture {
  return view.mode === "live" ? { kind: "pause" } : { kind: "live" };
}

/** The wheel zooms only when the chart has focus or Ctrl / ⌘ is held (a trackpad pinch
 * sends Ctrl); otherwise it scrolls the page as usual. */
export function wheelZooms(e: { focused: boolean; ctrlKey: boolean; metaKey: boolean }): boolean {
  return e.focused || e.ctrlKey || e.metaKey;
}

/** What the screen-reader status says on entering history mode or returning to live;
 * times count from `originMs`, the session start (AC-272WSN.2). `paused` marks a
 * history view entered by the pause gesture (AC-DDDYDJ.6). */
export function rangeAnnouncement(
  view: ChartView,
  originMs = 0,
  { paused = false }: { paused?: boolean } = {},
): string {
  if (view.mode === "history") {
    const from = formatSessionTime(view.t0Ms - originMs, 1);
    const to = formatSessionTime(view.t1Ms - originMs, 1);
    return `${paused ? "Paused" : "History"}: ${from} to ${to}`;
  }
  const label = LIVE_WINDOWS.find((w) => w.id === view.window)?.label ?? view.window;
  return `Live: ${label}`;
}

/** The keys the focused chart handles itself. */
export const CHART_KEYS: readonly string[] = [
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "End",
];

/** A `history_range` call. */
export interface HistoryRequest {
  series: Series[];
  t0Ms: number;
  t1Ms: number;
  buckets: number;
  /** A zoom / pan view, or the whole session (which also tells its true start). */
  purpose?: "view" | "session";
}

/** Every stored series the chart draws (power is derived from VBUS and IBUS). */
const HISTORY_SERIES: readonly Series[] = ["vbus", "ibus", "dp", "dm", "cc1", "cc2"];

/** The `history_range` call for a history view: one bucket per pixel of plot width;
 * `null` in live mode or before the plot has a width. */
export function historyRequestFor(view: ChartView, widthPx: number): HistoryRequest | null {
  if (view.mode !== "history") return null;
  const buckets = Math.floor(widthPx);
  if (!(buckets >= 1)) return null;
  return {
    series: [...HISTORY_SERIES],
    t0Ms: view.t0Ms,
    t1Ms: view.t1Ms,
    buckets,
    purpose: "view",
  };
}

/** The `history_range` call for the whole session up to `lastMs`, one bucket per pixel
 * (at least one, so it also serves to learn the session start). */
export function sessionRequestFor(lastMs: number, widthPx: number): HistoryRequest {
  const buckets = Math.max(1, Math.floor(Number.isFinite(widthPx) ? widthPx : 1));
  return { series: [...HISTORY_SERIES], t0Ms: 0, t1Ms: lastMs, buckets, purpose: "session" };
}

/** The session's first sample time from a whole-session answer (the earliest first
 * bucket); `null` when it holds no buckets. */
export function sessionStartOf(result: readonly SeriesBuckets[]): number | null {
  let start: number | null = null;
  for (const s of result) {
    const t = s.buckets[0]?.t_ms;
    if (t !== undefined && (start === null || t < start)) start = t;
  }
  return start;
}

/** Which frame the chart draws: the zoomed history answer, the whole-session answer
 * (the "all" window when the session began before the live buffer), or the live
 * buffer (also standing in until a history answer arrives). */
export function frameSource(
  view: ChartView,
  s: {
    sessionStartMs: number | null;
    bufferFirstMs: number | null;
    hasHistoryFrame: boolean;
    hasSessionFrame: boolean;
  },
): "history" | "session" | "live" {
  if (view.mode === "history") return s.hasHistoryFrame ? "history" : "live";
  if (
    view.window === "all" &&
    s.hasSessionFrame &&
    s.sessionStartMs !== null &&
    s.bufferFirstMs !== null &&
    s.sessionStartMs < s.bufferFirstMs
  ) {
    return "session";
  }
  return "live";
}

/** Lets something happen at most once per `intervalMs`. */
export class RateGate {
  #intervalMs: number;
  #last: number | null = null;

  constructor(intervalMs: number) {
    this.#intervalMs = intervalMs;
  }

  /** True (and counts as a go) when `intervalMs` has passed since the last go. */
  ready(nowMs: number): boolean {
    if (this.#last !== null && nowMs - this.#last < this.#intervalMs) return false;
    this.#last = nowMs;
    return true;
  }

  /** The next `ready` goes at once. */
  reset(): void {
    this.#last = null;
  }
}

/** What a {@link HistoryRequester} talks to. */
export interface HistoryRequesterOptions {
  /** Sends one `history_range` call. */
  fetch: (req: HistoryRequest) => Promise<SeriesBuckets[]>;
  /** The answer for the newest request (stale answers never arrive here). */
  onResult: (req: HistoryRequest, result: SeriesBuckets[]) => void;
  /** The newest request failed. */
  onError?: (error: unknown) => void;
  /** Runs the send on a coming frame (default: `requestAnimationFrame`). */
  schedule?: (cb: () => void) => void;
}

/** Keeps at most one `history_range` call in flight (AC-AH2AEM.4): a newer request
 * supersedes an older pending one, and an answer that is no longer the newest is
 * dropped. Sends happen on the next scheduled frame. */
export class HistoryRequester {
  #opts: HistoryRequesterOptions;
  #schedule: (cb: () => void) => void;
  #pending: HistoryRequest | null = null;
  #inFlight = false;
  #frameQueued = false;
  /** Bumped by `cancel()` so an answer for a cancelled request is dropped. */
  #epoch = 0;

  constructor(opts: HistoryRequesterOptions) {
    this.#opts = opts;
    this.#schedule = opts.schedule ?? ((cb) => void requestAnimationFrame(() => cb()));
  }

  /** Asks for `req`, replacing any request not yet sent. */
  request(req: HistoryRequest): void {
    this.#pending = req;
    this.#queue();
  }

  /** Forgets the pending request and drops the answer of the one in flight. */
  cancel(): void {
    this.#pending = null;
    this.#epoch++;
  }

  #queue(): void {
    if (this.#frameQueued || this.#inFlight) return;
    this.#frameQueued = true;
    this.#schedule(() => {
      this.#frameQueued = false;
      this.#send();
    });
  }

  #send(): void {
    const req = this.#pending;
    if (this.#inFlight || req === null) return;
    this.#pending = null;
    this.#inFlight = true;
    const epoch = this.#epoch;
    const current = (): boolean => epoch === this.#epoch && this.#pending === null;
    this.#opts.fetch(req).then(
      (result) => {
        this.#inFlight = false;
        if (current()) this.#opts.onResult(req, result);
        else if (this.#pending !== null) this.#queue();
      },
      (error: unknown) => {
        this.#inFlight = false;
        if (current()) this.#opts.onError?.(error);
        else if (this.#pending !== null) this.#queue();
      },
    );
  }
}
