/**
 * The PD message builder and the raw console (FR DGSQ6T), on the open trigger
 * session: built control / data messages for SOP, SOP' and SOP'', the Discover
 * Identity preset to the cable and its e-marker details, raw text lines with the
 * transcript and its times, GoodCRC refused with a reason, and the trigger panel's
 * confirmation for anything classified above 5 V.
 */

import type {
  CableView,
  ConsoleLine,
  PdMessageRequest,
  PdSendReply,
  RiskClass,
  UiError,
} from "./backend";

/** The backend calls the builder and the console make (the Tauri backend or the mock). */
export interface ConsoleApi {
  messageRisk(message: PdMessageRequest): Promise<RiskClass>;
  messageSend(message: PdMessageRequest, confirmed: boolean): Promise<PdSendReply>;
  consoleRisk(line: string): Promise<RiskClass>;
  consoleSend(line: string, confirmed: boolean): Promise<ConsoleLine[]>;
}

/** Discover Identity REQ to the cable plug (SOP'). */
export const DISCOVER_IDENTITY_CABLE: PdMessageRequest = {
  sop: "sop_prime",
  msg_type: 15,
  // Structured VDM header: SVID 0xFF00 (PD SID), VDM 2.1, REQ, Discover Identity.
  data_objects: [0xff00a801],
};

/** Why GoodCRC is never sent. */
export const GOODCRC_REASON =
  "GoodCRC is never sent by hand: the meter's PD stack sends it itself.";

/** The confirmation's text: a built message or raw line can raise VBUS to a voltage
 * the UI does not know. */
const CONFIRM_TEXT =
  "This can raise VBUS above 5 V. Confirm that nothing unsafe is connected to the output.";

/** Data objects typed as hex words (space or comma separated, `0x` optional); null
 * when a word is not 1-8 hex digits or there are more than 7. */
export function parseDataObjects(text: string): number[] | null {
  const words = text.split(/[\s,]+/).filter((w) => w !== "");
  if (words.length > 7) return null;
  const out: number[] = [];
  for (const word of words) {
    const hex = word.replace(/^0x/i, "");
    if (!/^[0-9a-f]{1,8}$/i.test(hex)) return null;
    out.push(parseInt(hex, 16) >>> 0);
  }
  return out;
}

/** Why the builder's message type is not a 5-bit Message Type (an integer 0-31), or
 * null. An emptied number input gives null or NaN. */
export function messageTypeError(msgType: number | null | undefined): string | null {
  return typeof msgType === "number" && Number.isInteger(msgType) && msgType >= 0 && msgType <= 31
    ? null
    : "The message type is a whole number from 0 to 31.";
}

/** Why the builder refuses `message` (GoodCRC), or null. */
export function messageRefusal(message: PdMessageRequest): string | null {
  return message.msg_type === 1 && message.data_objects.length === 0 ? GOODCRC_REASON : null;
}

/** Why `pdm open` / `pdm close` are not typed in the console. */
export const SESSION_REASON =
  "The console opens and closes the PDM session itself: pdm open and pdm close are not sent by hand.";

/** Why a `pd cmd` / `pd data` line whose value cannot be read is refused. */
function unsureReason(text: string): string {
  return `Cannot tell whether "${text}" is GoodCRC, which is never sent by hand: write it as pd cmd=<decimal> or pd data=<HEX>.`;
}

const U64_MAX = 0xffff_ffff_ffff_ffffn;

/** A non-negative number: decimal (leading zeros and `+` allowed) or `0x` hex, at
 * most 64 bits; null for anything else (a `-` sign included). */
function lenientNumber(text: string): bigint | null {
  if (!/^\+?(0x[0-9a-f]+|[0-9]+)$/.test(text)) return null;
  const n = BigInt(text.replace(/^\+/, ""));
  return n > U64_MAX ? null : n;
}

/** Hex digits only (either case), an even count; null otherwise. */
function hexBytes(text: string): number[] | null {
  if (text.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(text)) return null;
  const out: number[] = [];
  for (let i = 0; i < text.length; i += 2) out.push(parseInt(text.slice(i, i + 2), 16));
  return out;
}

/** Why the console refuses `line`, or null: GoodCRC as `pd cmd` or a `pd data`
 * control message in any spelling (lowercased, whitespace removed, the value read
 * leniently; a value that cannot be read is refused too), and `pdm open` / `pdm
 * close`. The backend applies the same rule; `goodcrc_parity.json` pins the two
 * together (U+0085 is whitespace to Rust but not to `\s`). */
export function lineRefusal(line: string): string | null {
  const text = line.trim();
  const norm = text.replace(/[\s\u0085]+/g, "").toLowerCase();
  if (norm.startsWith("pdmopen") || norm.startsWith("pdmclose")) return SESSION_REASON;
  if (norm.startsWith("pdcmd")) {
    const n = norm.startsWith("pdcmd=") ? lenientNumber(norm.slice("pdcmd=".length)) : null;
    if (n === null) return unsureReason(text);
    // The 5-bit Message Type field of the value.
    return (n & 0x1fn) === 1n ? GOODCRC_REASON : null;
  }
  if (norm.startsWith("pddata")) {
    const bytes = norm.startsWith("pddata=") ? hexBytes(norm.slice("pddata=".length)) : null;
    if (bytes === null || bytes.length < 3) return unsureReason(text);
    // SOP byte, then the PD header LE: a control GoodCRC has no data objects,
    // is not extended and has message type 1.
    const header = bytes[1]! | (bytes[2]! << 8);
    if ((header & 0x8000) === 0 && ((header >> 12) & 7) === 0 && (header & 0x1f) === 1) {
      return GOODCRC_REASON;
    }
  }
  return null;
}

/** `HH:MM:SS.mmm` in local time. */
export function formatTime(atMs: number): string {
  // A local value read once, never state: no reactivity needed.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const d = new Date(atMs);
  const two = (n: number): string => String(n).padStart(2, "0");
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}.${String(
    d.getMilliseconds(),
  ).padStart(3, "0")}`;
}

/** One transcript line as shown: time, `>` sent / `<` received, then the text (the
 * parsed block for a block). */
export function formatLine(line: ConsoleLine): string {
  const arrow = line.direction === "tx" ? ">" : "<";
  return `${formatTime(line.at_ms)} ${arrow} ${line.block ?? line.text}`;
}

/** A send waiting for the user's answer. */
export interface PendingSend {
  kind: "line" | "message";
  /** The console line, or the built message. */
  line: string | null;
  message: PdMessageRequest | null;
  /** What the dialog says (the trigger panel's confirmation text). */
  text: string;
}

export class PdConsole {
  /** The transcript, oldest first. */
  lines = $state<ConsoleLine[]>([]);
  /** The send waiting for confirmation, null when none. */
  pending = $state<PendingSend | null>(null);
  /** The cable's details from the last Discover Identity, null when none. */
  cable = $state<CableView | null>(null);
  /** The last failure or refusal, null when none. */
  error = $state<UiError | null>(null);
  /** A send is in flight: the controls are off and further sends are ignored. */
  busy = $state(false);

  #api: ConsoleApi;

  constructor(api: ConsoleApi) {
    this.#api = api;
  }

  /** Runs `job` as the one send in flight; ignored while another runs. */
  async #exclusive(job: () => Promise<void>): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      await job();
    } catch (e) {
      this.error = e as UiError;
    } finally {
      this.busy = false;
    }
  }

  /**
   * Sends a raw line: GoodCRC is refused, above 5 V waits for {@link confirm}.
   * True when the line went out.
   */
  async sendLine(line: string): Promise<boolean> {
    if (this.busy) return false;
    this.error = null;
    const text = line.trim();
    if (text === "") return false;
    const refusal = lineRefusal(text);
    if (refusal !== null) {
      this.error = { kind: "usage", message: refusal, hint: null };
      return false;
    }
    let sent = false;
    await this.#exclusive(async () => {
      // The backend's classifier decides (unknown text is above 5 V).
      if ((await this.#api.consoleRisk(text)) === "above_5v") {
        this.pending = { kind: "line", line: text, message: null, text: CONFIRM_TEXT };
        return;
      }
      this.#append(await this.#api.consoleSend(text, false));
      sent = true;
    });
    return sent;
  }

  /** Sends a built message: GoodCRC is refused, above 5 V waits for {@link confirm}. */
  async sendMessage(message: PdMessageRequest): Promise<void> {
    if (this.busy) return;
    this.error = null;
    const refusal = messageRefusal(message);
    if (refusal !== null) {
      this.error = { kind: "usage", message: refusal, hint: null };
      return;
    }
    await this.#exclusive(async () => {
      // The backend's classifier decides, not the UI's own guess.
      if ((await this.#api.messageRisk(message)) === "above_5v") {
        this.pending = { kind: "message", line: null, message, text: CONFIRM_TEXT };
        return;
      }
      await this.#sendMessage(message, false);
    });
  }

  /** Sends the pending line or message, confirmed. True when a line went out. */
  async confirm(): Promise<boolean> {
    const pending = this.pending;
    if (!pending || this.busy) return false;
    this.pending = null;
    this.error = null;
    let lineSent = false;
    await this.#exclusive(async () => {
      if (pending.message) {
        await this.#sendMessage(pending.message, true);
      } else if (pending.line !== null) {
        this.#append(await this.#api.consoleSend(pending.line, true));
        lineSent = true;
      }
    });
    return lineSent;
  }

  /** Drops the pending send; nothing is sent. */
  cancel(): void {
    this.pending = null;
  }

  async #sendMessage(message: PdMessageRequest, confirmed: boolean): Promise<void> {
    const reply = await this.#api.messageSend(message, confirmed);
    this.#append(reply.lines);
    if (reply.cable) this.cable = reply.cable;
  }

  /** Appends to the transcript as it is now (read after the await, never before). */
  #append(lines: ConsoleLine[]): void {
    this.lines = [...this.lines, ...lines];
  }

  /** Empties the transcript. */
  clear(): void {
    this.lines = [];
  }

  /** The trigger session closed: transcript, cable, error and pending send go. */
  reset(): void {
    this.lines = [];
    this.cable = null;
    this.error = null;
    this.pending = null;
  }
}
