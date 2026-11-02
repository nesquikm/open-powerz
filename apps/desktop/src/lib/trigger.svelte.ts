/**
 * The trigger view's state (FR DGSQ6S): detection, one section per protocol
 * (unsupported ones marked), the confirmation every request above 5 V needs, the
 * reply with the live VBUS from the meter stream and the 5 % / 2 s deviation flag,
 * Back to 5 V, and the cleanup when the view is left.
 */

import type {
  BackendEvent,
  CableRead,
  CableView,
  Detection,
  ProtocolSection,
  RiskClass,
  SupportName,
  TriggerLimits,
  TriggerProtocol,
  TriggerReply,
  TriggerRequest,
  UiError,
} from "./backend";
import { TRIGGER_PROTOCOLS } from "./backend";

/** The backend calls the panel makes (the Tauri backend or the mock). */
export interface TriggerApi {
  open(): Promise<void>;
  detect(): Promise<Detection>;
  list(protocol: TriggerProtocol): Promise<ProtocolSection>;
  risk(request: TriggerRequest): Promise<RiskClass>;
  request(request: TriggerRequest, confirmed: boolean, remember?: boolean): Promise<TriggerReply>;
  armed(): Promise<boolean>;
  disarm(): Promise<void>;
  /** The backend saw VBUS collapse under a contract. */
  chargerGone(): Promise<boolean>;
  /** `leaving`: the view was left, not a press of Back to 5 V. */
  backTo5v(leaving?: boolean): Promise<void>;
  close(): Promise<void>;
  cableRead(): Promise<CableRead>;
  cableSeen(): Promise<CableView | null>;
}

/** VBUS is flagged when it differs from the request by more than this fraction. */
export const DEVIATION_LIMIT = 0.05;
/** ... measured this long after the reply. */
export const DEVIATION_DELAY_MS = 2000;
/** VBUS below this, V, is worth asking the backend about. It decides. */
export const VBUS_GONE_V = 2;

/** Section headings, by protocol. */
export const PROTOCOL_TITLES: Record<TriggerProtocol, string> = {
  pd: "USB PD",
  qc: "QC",
  fcp: "FCP",
  afc: "AFC",
  sfcp: "SFCP",
  scp: "SCP",
  vfcp: "VFCP",
  tfcp: "TFCP",
  ufcs: "UFCS",
};

/** The protocols requested by a fixed voltage. */
export type FixedVoltProtocol = "qc" | "fcp" | "afc" | "sfcp";

/** The fixed-voltage buttons per protocol, V: QC 2.0 has 20 V, FCP / AFC / SFCP stop at 12 V. */
export const LEGACY_FIXED_VOLTS: Record<FixedVoltProtocol, readonly number[]> = {
  qc: [5, 9, 12, 20],
  fcp: [5, 9, 12],
  afc: [5, 9, 12],
  sfcp: [5, 9, 12],
};

export function isFixedVoltProtocol(p: TriggerProtocol): p is FixedVoltProtocol {
  return p === "qc" || p === "fcp" || p === "afc" || p === "sfcp";
}

/** The QC 3.0 control: 3.6-20 V in 200 mV steps. */
export const QC3_RANGE = { minMv: 3600, maxMv: 20000, stepMv: 200 } as const;

/** The protocols requested by a voltage and a current inside decoded limits. */
export type LimitsProtocol = "scp" | "vfcp" | "tfcp" | "ufcs";

export function isLimitsProtocol(p: TriggerProtocol): p is LimitsProtocol {
  return p === "scp" || p === "vfcp" || p === "tfcp" || p === "ufcs";
}

/** `voltageMv` and `currentMa` kept inside the block's ranges and, when it has one,
 * its power at that voltage. */
export function clampToLimits(
  limits: TriggerLimits,
  voltageMv: number,
  currentMa: number,
): { voltage_mv: number; current_ma: number } {
  const clamp = (x: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, x));
  let voltage_mv = clamp(Math.round(voltageMv), limits.min_voltage_mv, limits.max_voltage_mv);
  let current_ma = clamp(Math.round(currentMa), limits.min_current_ma, limits.max_current_ma);
  if (limits.max_power_mw !== null && voltage_mv > 0) {
    const cap = limits.max_power_mw * 1000;
    current_ma = Math.min(current_ma, Math.floor(cap / voltage_mv));
    // The cap would push the current below its minimum: lower the voltage
    // so the minimum current still fits the power.
    if (current_ma < limits.min_current_ma && limits.min_current_ma > 0) {
      current_ma = limits.min_current_ma;
      voltage_mv = Math.max(limits.min_voltage_mv, Math.floor(cap / current_ma));
    }
  }
  return { voltage_mv, current_ma };
}

/** The request of an SCP / VFCP / TFCP / UFCS control, clamped to `limits`; UFCS
 * addresses the capability by the block's index. */
export function limitsRequest(
  protocol: LimitsProtocol,
  limits: TriggerLimits,
  voltageMv: number,
  currentMa: number,
): TriggerRequest {
  const c = clampToLimits(limits, voltageMv, currentMa);
  // A block without a number is never sent to a guessed capability: 0 is no
  // capability (they count from 1), so the backend refuses it.
  if (protocol === "ufcs") return { protocol, index: limits.index ?? 0, ...c };
  return { protocol, ...c };
}

/** One section of the view. */
export interface SectionView {
  protocol: TriggerProtocol;
  title: string;
  /** `pending` until the protocol was listed. */
  state: "pending" | "supported" | "not-supported";
  section: ProtocolSection | null;
}

/** One section per protocol, none listed yet. */
function unlistedSections(): SectionView[] {
  return TRIGGER_PROTOCOLS.map((protocol) => ({
    protocol,
    title: PROTOCOL_TITLES[protocol],
    state: "pending",
    section: null,
  }));
}

/** A request above 5 V waiting for the user's answer. */
export interface PendingConfirmation {
  request: TriggerRequest;
  /** The voltage the request asks for, mV. */
  voltageMv: number;
  /** What the dialog says: names the voltage and asks the user to confirm that
   * nothing unsafe is connected to the output. */
  text: string;
  /** The dialog offers "don't ask again". */
  rememberable: boolean;
}

/** Whether `vbusV` differs from `requestedMv` by more than {@link DEVIATION_LIMIT}. */
export function deviates(vbusV: number, requestedMv: number): boolean {
  if (requestedMv <= 0) return false;
  return Math.abs(vbusV * 1000 - requestedMv) / requestedMv > DEVIATION_LIMIT;
}

/** The voltage `request` asks for, mV (a fixed PD offer's from its section), or null
 * when it is not known. */
export function requestVoltageMv(
  request: TriggerRequest,
  sections: readonly ProtocolSection[],
): number | null {
  switch (request.protocol) {
    case "pd": {
      if (request.voltage_mv !== null) return request.voltage_mv;
      const pd = sections.find((s) => s.protocol === "pd");
      const offer = pd?.offers.find((o) => o.position === request.position);
      return offer ? offer.max_voltage_mv : null;
    }
    case "qc":
    case "fcp":
    case "afc":
    case "sfcp":
      return request.volts * 1000;
    default:
      return request.voltage_mv;
  }
}

/** A USB id as `0x1234`. */
export function hexId(id: number): string {
  return `0x${id.toString(16).toUpperCase().padStart(4, "0")}`;
}

/** A cable's vendor: the name from our own table, else its VID as `0x1234`. */
export function cableVendorText(cable: CableView): string {
  return cable.vendor ?? hexId(cable.vid);
}

/** What the Cable card shows, and where it came from. */
export interface CableCard {
  cable: CableView;
  source: "read" | "capture";
}

/** The confirmation dialog's text for a request of `voltageMv`. */
export function confirmationText(voltageMv: number): string {
  const volts = `${Number((voltageMv / 1000).toFixed(2))} V`;
  return (
    `The charger will raise VBUS to ${volts}. ` +
    "Confirm that nothing unsafe is connected to the output: only the meter, " +
    `or a device rated for ${volts}.`
  );
}

/** One BC1.2 / Apple row of the Detection card. */
export interface DetectionCheck {
  label: "BC1.2" | "Apple";
  text: string;
}

const SUPPORT_TEXT: Record<SupportName, string> = {
  supported: "Supported",
  "not-supported": "Not supported",
  unknown: "Unknown",
};

/**
 * The BC1.2 and Apple rows. When `entry bc` / `entry apple` and the automatic
 * list's matching line (`bc1.2` / an `Apple …` mode line) say supported vs not
 * supported, the row names both sources:
 * `entry bc: not supported · list: supported`. Like the library's
 * `detect::disagreements`, every clashing Apple mode line is named:
 * `entry apple: not supported · list: supported (Apple 2.4A, Apple 2.1A)`.
 */
export function detectionChecks(d: Detection): DetectionCheck[] {
  const rows: [DetectionCheck["label"], string, SupportName, (name: string) => boolean, boolean][] =
    [
      ["BC1.2", "entry bc", d.bc12, (n) => n.toLowerCase() === "bc1.2", false],
      ["Apple", "entry apple", d.apple, (n) => n.startsWith("Apple"), true],
    ];
  return rows.map(([label, check, says, pairs, named]) => {
    // A decided check against decided lines: every clash says the opposite.
    const clashes =
      says === "unknown"
        ? []
        : d.entries.filter((e) => pairs(e.name) && e.support !== "unknown" && e.support !== says);
    const first = clashes[0];
    if (first === undefined) return { label, text: SUPPORT_TEXT[says] ?? says };
    const names = named ? ` (${clashes.map((e) => e.name).join(", ")})` : "";
    const text = `${check}: ${SUPPORT_TEXT[says].toLowerCase()} · list: ${SUPPORT_TEXT[first.support].toLowerCase()}${names}`;
    return { label, text };
  });
}

/** The dialog's text when the voltage of a request is not known. */
const UNKNOWN_VOLTAGE_TEXT =
  "The charger may raise VBUS above 5 V. " +
  "Confirm that nothing unsafe is connected to the output.";

/** What a request gets with no session. */
const NO_SESSION: UiError = {
  kind: "usage",
  message: "No trigger session. Press Detect.",
  hint: null,
};

export class TriggerPanel {
  /** A trigger session is open. */
  sessionOpen = $state(false);
  /** The last detection, null before one. */
  detection = $state<Detection | null>(null);
  /** One per protocol, PD first. */
  sections = $state<SectionView[]>([]);
  /** The request waiting for confirmation, null when none. */
  pending = $state<PendingConfirmation | null>(null);
  /** The dialog's "don't ask again" box; unchecked each time a dialog opens. */
  remember = $state(false);
  /** The backend is armed: later requests up to 20 V send with no dialog. The backend
   * decides, this only shows it. */
  armed = $state(false);
  /** VBUS collapsed under a contract: the backend said so. */
  chargerGone = $state(false);
  /** The charger's answer to the last request. */
  reply = $state<TriggerReply | null>(null);
  /** A request is out to the charger; a dialog waiting for the user is not. */
  requesting = $state(false);
  /** Why the last request failed (error reply, timeout, refusal), null when it did not. */
  requestFailure = $state<UiError | null>(null);
  /** The voltage of the last request, mV. */
  requestedMv = $state<number | null>(null);
  /** Live VBUS from the meter stream, V. */
  vbusV = $state<number | null>(null);
  /** VBUS differs from the request by more than 5 % (2 s after the reply). */
  deviation = $state(false);
  /** The last failure, null when none. */
  error = $state<UiError | null>(null);
  /** The meter's replies fell out of step with the commands (`out_of_sync`): no
   * request is sent until the session is reopened. */
  requestsDisabled = $state(false);
  /** What the Cable card shows, null when it has nothing. */
  cable = $state<CableCard | null>(null);
  /** The last read ended with no answer. */
  cableNoAnswer = $state(false);
  /** A read is running. */
  cableBusy = $state(false);
  /** Detect is running: nothing else is sent to the meter, and a press made meanwhile
   * is dropped. */
  detecting = $state(false);

  #api: TriggerApi;
  /** The 2 s after the reply have passed: VBUS is compared with the request. */
  #settled = false;
  /** The view was left since the last {@link detect} started. */
  #left = false;
  /** Bumped by each armed read and each session end: only the latest read counts. */
  #armedRead = 0;
  /** A request or the way back was accepted since the last {@link detect} started. */
  #contract = false;
  /** {@link #askChargerGone}'s question is out. */
  #askingGone = false;
  /** The press being handled, from the press to its dialog or answer: a second one
   * is ignored. A session end drops it, so a request that never answers holds nothing. */
  #press: object | null = null;
  /** Bumped by each {@link detect}: a press whose answer comes back after one started
   * is dropped. */
  #detects = 0;
  /** The running Detect's number, null when none runs: a run that is no longer this
   * one stops at its next step and leaves {@link detecting} alone. */
  #detectRun: number | null = null;
  #onDetect: () => void;

  /** The view was left or the session closed: late answers are dropped. */
  get #gone(): boolean {
    return this.#left || !this.sessionOpen;
  }
  #settleTimer: ReturnType<typeof setTimeout> | null = null;

  /** `onDetect` runs as each Detect starts: the page drops the console's dialog there. */
  constructor(api: TriggerApi, hooks: { onDetect?: () => void } = {}) {
    this.#api = api;
    this.#onDetect = hooks.onDetect ?? (() => {});
    this.sections = unlistedSections();
  }

  /** Back to 5 V is shown while a session is open. */
  get showBackTo5v(): boolean {
    return this.sessionOpen;
  }

  /** Opens the session if needed, detects, then lists every protocol. One at a time. */
  async detect(): Promise<void> {
    if (this.#detectRun !== null) return;
    const run = ++this.#detects;
    this.#detectRun = run;
    this.detecting = true;
    // No dialog from before Detect is answered after it.
    this.pending = null;
    this.#onDetect();
    try {
      await this.#detect(run);
    } finally {
      // A newer run, or none after a leave or a session end: not this run's to clear.
      if (this.#detectRun === run) this.#stopDetect();
    }
  }

  /** The running Detect, if any, stops at its next step. */
  #stopDetect(): void {
    this.#detectRun = null;
    this.detecting = false;
  }

  async #detect(run: number): Promise<void> {
    /** This run was stopped, or the view ended. */
    const stale = () => this.#gone || this.#detectRun !== run;
    this.error = null;
    this.#left = false;
    // Detecting takes VBUS down on purpose.
    this.#contract = false;
    this.chargerGone = false;
    // The backend disarms as it detects: the status goes now, not with the answer.
    this.#notArmed();
    try {
      // An out-of-step session refuses everything: detecting again reopens it.
      if (this.requestsDisabled && this.sessionOpen) {
        try {
          await this.#api.close();
        } catch {
          // The session is dropped either way.
        }
        if (this.#detectRun !== run) return;
        this.#ended(false);
      }
      if (!this.sessionOpen) {
        await this.#api.open();
        this.sessionOpen = true;
        // The view was left while the session was opening: close it again,
        // so no session stays open behind another tab.
        if (this.#left) {
          await this.leave();
          return;
        }
        if (stale()) return;
        // What the backend says, not what this page last saw.
        await this.refreshArmed();
        if (stale()) return;
      }
      try {
        const detection = await this.#api.detect();
        // A late answer must not put a charger back on a view that ended.
        if (!stale()) this.detection = detection;
      } finally {
        // The backend disarmed before it wrote anything, so a failed detection counts too.
        if (!stale()) await this.refreshArmed();
      }
      for (const protocol of TRIGGER_PROTOCOLS) {
        if (stale()) return;
        const section = await this.#api.list(protocol);
        if (stale()) return;
        this.sections = this.sections.map((s) =>
          s.protocol === protocol
            ? { ...s, section, state: section.supported ? "supported" : "not-supported" }
            : s,
        );
      }
    } catch (e) {
      // An error from a session the view already left is not news.
      if (!this.#left && this.#detectRun === run) this.#fail(e);
    }
  }

  /** Asks for `request`: above 5 V it waits in {@link pending} for {@link confirm}. */
  async ask(request: TriggerRequest): Promise<void> {
    // Nothing to ask with: the press says so instead of doing nothing.
    if (this.#gone) {
      this.error = NO_SESSION;
      return;
    }
    // Out of step: nothing goes to the meter, and the power-cycle error stays.
    if (this.requestsDisabled) return;
    // Detect holds the meter: the press is dropped, never answered later.
    if (this.detecting) return;
    if (this.#press !== null) return;
    const press = (this.#press = {});
    const detects = this.#detects;
    /** A Detect runs or ran since the press: its answer is stale. */
    const detected = () => this.detecting || this.#detects !== detects;
    this.error = null;
    try {
      // The backend's classifier decides, not the UI's own guess.
      const risk = await this.#api.risk(request);
      if (this.#gone || detected()) return;
      // The backend looked at the port for this answer, so it may have just disarmed.
      if (this.armed && risk !== "armed") await this.refreshArmed();
      // The charger may have gone meanwhile: no dialog opens on an offer now gone.
      if (this.#gone || this.chargerGone || this.#press !== press || detected()) return;
      // `armed` needs no dialog: it is sent unconfirmed, and the backend checks it again.
      if (risk === "above_5v" || risk === "above_5v_rememberable") {
        const sections = this.sections.flatMap((s) => (s.section ? [s.section] : []));
        const mv = requestVoltageMv(request, sections);
        this.remember = false;
        this.pending = {
          request,
          voltageMv: mv ?? 0,
          text: mv === null ? UNKNOWN_VOLTAGE_TEXT : confirmationText(mv),
          rememberable: risk === "above_5v_rememberable",
        };
        return;
      }
      await this.#send(request, false);
    } catch (e) {
      if (!this.#left) {
        this.#fail(e);
        // The refusal may be the first this page hears of a collapse.
        await this.#askChargerGone();
      }
    } finally {
      this.#release(press);
    }
  }

  /** Sends the pending request, confirmed. */
  async confirm(): Promise<void> {
    const pending = this.pending;
    if (!pending) return;
    this.pending = null;
    // Detect holds the meter: a dialog pressed meanwhile sends nothing.
    if (this.requestsDisabled || this.detecting) return;
    const press = (this.#press = {});
    this.error = null;
    try {
      await this.#send(pending.request, true, pending.rememberable && this.remember);
    } catch (e) {
      if (!this.#left) this.#fail(e);
    } finally {
      this.#release(press);
    }
  }

  /** `press` is done, unless a session end already dropped it. */
  #release(press: object): void {
    if (this.#press !== press) return;
    this.#press = null;
    this.requesting = false;
  }

  /** Shows `e`; an `out_of_sync` failure also disables requests. */
  #fail(e: unknown): void {
    const error = e as UiError;
    this.error = error;
    if (error?.kind === "out_of_sync") this.requestsDisabled = true;
  }

  /** Drops the pending request; nothing is sent. */
  cancel(): void {
    this.pending = null;
  }

  /** Sends `request` and keeps the reply. */
  async #send(request: TriggerRequest, confirmed: boolean, remember = false): Promise<void> {
    let reply: TriggerReply;
    this.requesting = true;
    try {
      reply = remember
        ? await this.#api.request(request, confirmed, true)
        : await this.#api.request(request, confirmed);
    } catch (e) {
      // The failure is this request's result: it replaces the previous reply.
      if (!this.#gone) {
        this.reply = null;
        this.requestFailure = e as UiError;
        // A failure may have ended "don't ask again"; the backend knows.
        await this.refreshArmed();
      }
      throw e;
    }
    // Left (or closed) meanwhile: no result, no timer on a closed session.
    if (this.#gone) return;
    this.requestFailure = null;
    this.reply = reply;
    this.#contract = true;
    this.requestedMv =
      reply.requested_mv ??
      requestVoltageMv(
        request,
        this.sections.flatMap((s) => (s.section ? [s.section] : [])),
      );
    this.#restartSettle();
    await this.refreshArmed();
  }

  /** The status goes now, and a read still out no longer counts. */
  #notArmed(): void {
    this.#armedRead += 1;
    this.armed = false;
  }

  /** Takes the armed state from the backend; one that cannot answer is not armed. */
  async refreshArmed(): Promise<void> {
    const read = ++this.#armedRead;
    let armed = false;
    try {
      armed = await this.#api.armed();
    } catch {
      // Reads as not armed.
    }
    if (read === this.#armedRead) this.armed = armed;
  }

  /** Ends "don't ask again" at once. */
  async clearArmed(): Promise<void> {
    this.error = null;
    try {
      await this.#api.disarm();
    } catch (e) {
      this.#fail(e);
    }
    await this.refreshArmed();
  }

  /** One press of Read cable: one message inside the open session, no dialog, no retry. */
  async readCable(): Promise<void> {
    if (!this.sessionOpen || this.requestsDisabled || this.cableBusy || this.detecting) return;
    // The charger went away: nothing answers until Detect finds it again.
    if (this.chargerGone) return;
    this.error = null;
    this.cableBusy = true;
    // The card shows this press's answer or nothing: the cable may have changed.
    this.cable = null;
    this.cableNoAnswer = false;
    try {
      const read = await this.#api.cableRead();
      if (this.#gone) return;
      this.cable = read.kind === "found" ? { cable: read.cable, source: "read" } : null;
      this.cableNoAnswer = read.kind === "no_answer";
    } catch (e) {
      if (!this.#left) this.#fail(e);
    } finally {
      this.cableBusy = false;
      // The backend disarmed before it wrote anything, so a failed read counts too.
      if (!this.#gone) await this.refreshArmed();
    }
  }

  /** Another meter is on screen: nothing on the panel describes it yet. */
  async meterChanged(): Promise<void> {
    // Nothing asked or detected on the other meter goes on here.
    this.pending = null;
    this.#press = null;
    this.requesting = false;
    this.#stopDetect();
    this.cable = null;
    this.cableNoAnswer = false;
    await this.refreshArmed();
    await this.loadCaptureCable();
  }

  /** With no session, shows the identity a PD capture holds; sends nothing. */
  async loadCaptureCable(): Promise<void> {
    if (this.sessionOpen) return;
    const seen = await this.#api.cableSeen().catch(() => null);
    if (this.sessionOpen) return;
    this.cable = seen === null ? null : { cable: seen, source: "capture" };
  }

  /** Clears the flag and compares VBUS again only {@link DEVIATION_DELAY_MS} from now. */
  #restartSettle(): void {
    if (this.#settleTimer !== null) clearTimeout(this.#settleTimer);
    this.#settled = false;
    this.deviation = false;
    this.#settleTimer = setTimeout(() => {
      this.#settleTimer = null;
      this.#settled = true;
      this.#evaluate();
    }, DEVIATION_DELAY_MS);
  }

  /** Flags VBUS against the request once settled. */
  #evaluate(): void {
    this.deviation =
      this.#settled &&
      this.vbusV !== null &&
      this.requestedMv !== null &&
      deviates(this.vbusV, this.requestedMv);
  }

  /** Back to 5 V; the session stays open. */
  async backTo5v(): Promise<void> {
    if (this.detecting) return;
    this.error = null;
    try {
      await this.#api.backTo5v();
      if (this.#gone) return;
      // The way back is the latest request: nothing of the one before stays on
      // the card. The backend reports no reply for it, so nothing is delivered yet.
      this.reply = {
        command: "Back to 5 V",
        requested_mv: 5000,
        requested_ma: null,
        delivered_mv: null,
        delivered_ma: null,
        reply: null,
      };
      this.requestFailure = null;
      this.requestedMv = 5000;
      this.#contract = true;
      this.#restartSettle();
    } catch (e) {
      if (!this.#left) this.#fail(e);
    }
    // The backend disarmed before it wrote anything, so a failed way back counts too.
    if (!this.#gone) await this.refreshArmed();
  }

  /** The view is left: back to 5 V where the protocol allows it, then close. */
  async leave(): Promise<void> {
    this.pending = null;
    this.#stopDetect();
    // Seen by a detect() still in flight: it closes a session that opens
    // after this and stops listing.
    this.#left = true;
    if (!this.sessionOpen) return;
    try {
      await this.#api.backTo5v(true);
    } catch {
      // Closing still returns to 5 V where it can; the session must not stay open.
    }
    try {
      await this.#api.close();
      this.error = null;
    } catch (e) {
      // The view's own close failing is worth showing.
      this.error = e as UiError;
    }
    this.#ended();
  }

  /** The session is over: the view starts afresh, with no offers to press. A running
   * Detect stops too, unless it is the one ending the session (`stopDetect` false). */
  #ended(stopDetect = true): void {
    if (stopDetect) this.#stopDetect();
    if (this.#settleTimer !== null) clearTimeout(this.#settleTimer);
    this.#settleTimer = null;
    this.#settled = false;
    this.sessionOpen = false;
    this.requestsDisabled = false;
    this.pending = null;
    this.#press = null;
    this.requesting = false;
    this.chargerGone = false;
    this.vbusV = null;
    this.#forgetCharger();
  }

  /** Nothing on the panel describes the charger any more, so no offer can be pressed. */
  #forgetCharger(): void {
    this.#notArmed();
    this.#contract = false;
    this.pending = null;
    this.detection = null;
    this.sections = unlistedSections();
    this.reply = null;
    this.requestFailure = null;
    this.requestedMv = null;
    this.deviation = false;
    this.cable = null;
    this.cableNoAnswer = false;
  }

  /** VBUS read low with a contract standing: asks the backend, which also saw the
   * reading. A backend that cannot answer changes nothing. */
  async #askChargerGone(): Promise<void> {
    if (!this.#contract || this.#askingGone) return;
    this.#askingGone = true;
    let gone = false;
    try {
      gone = await this.#api.chargerGone();
    } catch {
      // Not known to be gone.
    }
    this.#askingGone = false;
    // `#contract` is down again if the session ended or Detect started meanwhile.
    if (!gone || !this.#contract) return;
    this.#forgetCharger();
    this.chargerGone = true;
  }

  /** Takes the session channel's events: live VBUS, and a session that ends. */
  handleEvent(event: BackendEvent): void {
    if (event.type === "status" && event.data.state === "disconnected") {
      // The backend closed the trigger session with the meter's.
      this.#ended();
      return;
    }
    if (event.type === "batch") {
      const last = event.data.samples.at(-1);
      if (last === undefined) return;
      this.vbusV = last.vbus_v;
      this.#evaluate();
      if (last.vbus_v < VBUS_GONE_V) void this.#askChargerGone();
    }
  }
}
