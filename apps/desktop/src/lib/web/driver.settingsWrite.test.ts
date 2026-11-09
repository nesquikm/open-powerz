/**
 * FR W8SHX9: changing the meter's settings over the browser session driver.
 *
 * - AC-W8SHX9.1: `writeSettings` reads SETTINGS twice, sends one type-0x0A write and
 *   reads SETTINGS again, all over the settings pipe (none of it over the session's
 *   own transport); two reads that differ refuse the write and nothing more is sent.
 * - AC-W8SHX9.2: the write frame is the wasm core's; an unknown key or an out-of-range
 *   value is refused before the pipe is even opened.
 * - AC-W8SHX9.3: a read-back without the change fails naming the field; nothing is
 *   retried; a REJECT fails the write; no reply to the write leaves it to the
 *   read-back.
 * - AC-W8SHX9.4: a failure to open the pipe (a cancelled picker, a busy port) ends the
 *   change with that error and sends nothing.
 * - AC-W8SHX9.5: after a change that took, a queue stream is stopped and started
 *   again at the session's rate; not after a failed change, and not in snapshot mode.
 * - AC-W8SHX9.7: a HID session refuses the change with the basic-readings wording.
 *
 * Runs against the wasm demo meter and its CDC twin on Vitest's fake clock.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MeterSettings, SettingChanges, UiError } from "../backend";
import { CdcPipe, DemoCdcTransport } from "./cdcTransport";
import { DemoTransport } from "./demoTransport";
import { MeterDriver } from "./driver";
import type { PowerzWasm } from "./driver";
import { hidOnlyBasicText } from "./features";
import { demoSettingsReply, foreignAdcReply, withId } from "./fakeSerial.test";
import { classify, loadWasm, type FrameName } from "./testWasm.test";
import { timeoutError, type WebTransport } from "./transport";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];

/** The driver with the FR W8SHX9 command. */
type WriteDriver = MeterDriver & {
  writeSettings(
    changes: SettingChanges,
    openPipe: () => Promise<WebTransport>,
  ): Promise<MeterSettings>;
};

/** Records every frame; can make every auth fail (a snapshot-mode session). */
class Recorder implements WebTransport {
  log: Uint8Array[] = [];
  closed = false;
  spoilAuth = false;
  constructor(private inner: WebTransport) {}
  names(from = 0): FrameName[] {
    return this.log.slice(from).map((f) => classify(w, f));
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    this.log.push(frame.slice());
    const reply = await this.inner.transact(frame, timeoutMs);
    if (this.spoilAuth && classify(w, frame) === "auth") {
      const bad = reply.slice();
      for (let i = 4; i < bad.length; i++) bad[i] = (bad[i] ?? 0) ^ 0xa5;
      return bad;
    }
    return reply;
  }
  read(timeoutMs: number): Promise<Uint8Array> {
    return this.inner.read(timeoutMs);
  }
  async close(): Promise<void> {
    this.closed = true;
    await this.inner.close();
  }
}

/** A settings pipe answering from a script: `replies[i]` answers the i-th frame
 * (`"timeout"`: no reply). Each reply gets the request's id. */
class ScriptedPipe implements WebTransport {
  sent: Uint8Array[] = [];
  closed = false;
  constructor(private replies: (Uint8Array | "timeout")[]) {}
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    this.sent.push(frame.slice());
    const next = this.replies.shift();
    if (next === undefined || next === "timeout") {
      await new Promise((r) => setTimeout(r, timeoutMs));
      throw timeoutError();
    }
    return withId(next, frame[1] ?? 0);
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    await new Promise((r) => setTimeout(r, timeoutMs));
    throw timeoutError();
  }
  async close(): Promise<void> {
    this.closed = true;
  }
}

/** Records every frame sent to the demo CDC twin. */
class PipeRecorder implements WebTransport {
  sent: Uint8Array[] = [];
  closed = false;
  constructor(private inner: WebTransport) {}
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    this.sent.push(frame.slice());
    return this.inner.transact(frame, timeoutMs);
  }
  read(timeoutMs: number): Promise<Uint8Array> {
    return this.inner.read(timeoutMs);
  }
  async close(): Promise<void> {
    this.closed = true;
    await this.inner.close();
  }
}

let w: PowerzWasm;

beforeEach(async () => {
  w = await loadWasm();
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
  });
  vi.setSystemTime(1_790_000_000_000);
});

afterEach(() => {
  vi.useRealTimers();
});

async function settle<T>(p: Promise<T>, maxMs = 10_000): Promise<T> {
  let done = false;
  let value: T | undefined;
  let error: unknown;
  let failed = false;
  p.then(
    (v) => {
      done = true;
      value = v;
    },
    (e: unknown) => {
      done = true;
      failed = true;
      error = e;
    },
  );
  for (let i = 0; i < 5 && !done; i++) await Promise.resolve();
  for (let t = 0; t < maxMs && !done; t += 10) await vi.advanceTimersByTimeAsync(10);
  if (!done) throw new Error("promise did not settle on the fake clock");
  if (failed) throw error;
  return value as T;
}

async function rejection(p: Promise<unknown>): Promise<UiError> {
  try {
    await settle(p);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("the call resolved");
}

interface Run {
  demo: DemoTransport;
  rec: Recorder;
  driver: WriteDriver;
  errors: UiError[];
}

async function start(o: { hid?: boolean; spoilAuth?: boolean } = {}): Promise<Run> {
  const demo = new DemoTransport(w, "load-5v-1a", () => Date.now());
  const rec = new Recorder(demo);
  rec.spoilAuth = o.spoilAuth === true;
  const errors: UiError[] = [];
  const options = {
    wasm: w,
    transport: rec,
    rateCode: 2,
    hid: o.hid === true,
    onBatch: () => {},
    onError: (e: UiError) => errors.push(e),
    random: () => 0x1234_5678_9abc_def0n,
  } as unknown as DriverOptions;
  const driver = (await settle(MeterDriver.connect(options))) as WriteDriver;
  await vi.advanceTimersByTimeAsync(250);
  return { demo, rec, driver, errors };
}

/** A pipe opener over the demo meter's CDC twin; counts its calls. */
function twinOpener(demo: DemoTransport): {
  open: () => Promise<WebTransport>;
  pipes: PipeRecorder[];
} {
  const pipes: PipeRecorder[] = [];
  return {
    pipes,
    open: async () => {
      const pipe = new PipeRecorder(new DemoCdcTransport(demo, () => Date.now()));
      pipes.push(pipe);
      return pipe;
    },
  };
}

/** Frames other than the stream's own polls. */
const POLLS: readonly FrameName[] = ["get_data:adc", "get_data:adc_queue"];
const noPolls = (names: FrameName[]) => names.filter((n) => !POLLS.includes(n));

/** Frame kinds over the settings pipe: SETTINGS reads as "read", the write as "write". */
const pipeKinds = (frames: Uint8Array[]) =>
  frames.map((f) =>
    classify(w, f) === "get_data:settings" ? "read" : f[0] === 0x0a ? "write" : "other",
  );

const valueOf = (s: MeterSettings, key: string) => s.fields.find((f) => f.key === key)?.value;

const ACCEPT = new Uint8Array([0x05, 0, 0, 0]);
const REJECT = new Uint8Array([0x06, 0, 0, 0]);

describe("the read-read-write-read sequence (AC-W8SHX9.1, .2)", () => {
  it("reads twice, writes once and reads back over the pipe; the meter keeps the change", async () => {
    const { demo, rec, driver, errors } = await start();
    const pipe = twinOpener(demo);
    const from = rec.log.length;
    const after = await settle(driver.writeSettings({ brightness: 80 }, pipe.open));
    expect(pipe.pipes).toHaveLength(1);
    expect(pipeKinds(pipe.pipes[0]?.sent ?? [])).toEqual(["read", "read", "write", "read"]);
    expect(valueOf(after, "brightness")).toContain("80");
    expect(after.warnings ?? []).toEqual([]);
    // Nothing of the change went over the session's own transport: only the queue
    // restart (AC-W8SHX9.5).
    expect(noPolls(rec.names(from))).toEqual(["stop_queue", "start_queue:2"]);
    expect(pipe.pipes[0]?.closed).toBe(true);
    const read = await settle(driver.readSettings());
    expect(valueOf(read, "brightness")).toContain("80");
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("two reads that differ refuse the write: nothing more is sent", async () => {
    const { rec, driver } = await start();
    const pipe = new ScriptedPipe([
      demoSettingsReply(w),
      demoSettingsReply(w, { brightness: 42 }),
      ACCEPT,
      demoSettingsReply(w, { brightness: 80 }),
    ]);
    const from = rec.log.length;
    const e = await rejection(driver.writeSettings({ brightness: 80 }, async () => pipe));
    expect(e).toEqual({
      kind: "protocol",
      message: "Two reads of the meter's settings differ, so nothing was written.",
      hint: "Check the cable, then apply again.",
    });
    expect(pipeKinds(pipe.sent)).toEqual(["read", "read"]);
    expect(pipe.closed).toBe(true);
    // No queue restart after a write that did not happen.
    expect(noPolls(rec.names(from))).toEqual([]);
    await settle(driver.disconnect());
  });

  for (const [what, changes, message] of [
    ["an unknown key", { nonsense: 1 }, "The setting nonsense cannot be changed."],
    ["calibration", { voltage_cal: 1 }, "The setting voltage_cal cannot be changed."],
    ["auto-rotate", { auto_rotate: 1 }, "The setting auto_rotate cannot be changed."],
    ["an out-of-range value", { brightness: 0 }, "brightness = 0 is out of range (1..=100)."],
    ["no change", {}, "No setting was changed."],
  ] as const) {
    it(`${what} is refused before the pipe opens (AC-W8SHX9.2)`, async () => {
      const { rec, driver } = await start();
      let opened = 0;
      const from = rec.log.length;
      const e = await rejection(
        driver.writeSettings(changes as unknown as SettingChanges, async () => {
          opened++;
          return new ScriptedPipe([]);
        }),
      );
      expect(e).toEqual({ kind: "usage", message, hint: null });
      expect(opened).toBe(0);
      expect(noPolls(rec.names(from))).toEqual([]);
      await settle(driver.disconnect());
    });
  }
});

describe("the read-back (AC-W8SHX9.3)", () => {
  it("a read-back without the change fails naming the field; nothing is retried", async () => {
    const { rec, driver } = await start();
    const same = demoSettingsReply(w);
    const pipe = new ScriptedPipe([same, same, ACCEPT, same]);
    const from = rec.log.length;
    const e = await rejection(driver.writeSettings({ brightness: 80 }, async () => pipe));
    expect(e.kind).toBe("protocol");
    expect(e.message).toMatch(
      /^The meter's settings do not match after the change: brightness reads .+, expected 80\.$/,
    );
    expect(e.hint).toBe("Read the settings again; nothing was retried.");
    expect(pipeKinds(pipe.sent)).toEqual(["read", "read", "write", "read"]);
    expect(pipe.closed).toBe(true);
    expect(noPolls(rec.names(from))).toEqual([]);
    await settle(driver.disconnect());
  });

  it("no reply to the write leaves it to the read-back", async () => {
    const { driver } = await start();
    const before = demoSettingsReply(w);
    const pipe = new ScriptedPipe([
      before,
      before,
      "timeout",
      demoSettingsReply(w, { brightness: 80 }),
    ]);
    const after = await settle(driver.writeSettings({ brightness: 80 }, async () => pipe));
    expect(valueOf(after, "brightness")).toContain("80");
    expect(pipeKinds(pipe.sent)).toEqual(["read", "read", "write", "read"]);
    await settle(driver.disconnect());
  });

  it("a REJECT fails the write and nothing is read again", async () => {
    const { driver } = await start();
    const before = demoSettingsReply(w);
    const pipe = new ScriptedPipe([before, before, REJECT, before]);
    const e = await rejection(driver.writeSettings({ brightness: 80 }, async () => pipe));
    expect(e).toEqual({
      kind: "protocol",
      message: "The meter refused the request.",
      hint: "Updating the meter's firmware may help.",
    });
    expect(pipeKinds(pipe.sent)).toEqual(["read", "read", "write"]);
    expect(pipe.closed).toBe(true);
    await settle(driver.disconnect());
  });
});

describe("opening the pipe (AC-W8SHX9.4)", () => {
  it("a pipe that cannot be opened ends the change with its error, sending nothing", async () => {
    const { rec, driver } = await start();
    const cancelled: UiError = {
      kind: "usage",
      message: "No serial port was chosen, so nothing was sent.",
      hint: null,
    };
    const from = rec.log.length;
    const e = await rejection(
      driver.writeSettings({ brightness: 80 }, () => Promise.reject(cancelled)),
    );
    expect(e).toEqual(cancelled);
    expect(noPolls(rec.names(from))).toEqual([]);
    // The stream goes on.
    const polls = rec.log.length;
    await vi.advanceTimersByTimeAsync(500);
    expect(rec.log.length).toBeGreaterThan(polls);
    await settle(driver.disconnect());
  });
});

describe("the queue after a change (AC-W8SHX9.5)", () => {
  it("restarts the queue at the session's rate after a change that took", async () => {
    const { demo, rec, driver } = await start();
    expect(driver.mode).toBe("queue");
    const from = rec.log.length;
    await settle(driver.writeSettings({ sample_rate: 0 }, twinOpener(demo).open));
    expect(noPolls(rec.names(from))).toEqual(["stop_queue", "start_queue:2"]);
    expect(driver.rateCode).toBe(2);
    await settle(driver.disconnect());
  });

  it("does not touch the stream in snapshot mode", async () => {
    const { demo, rec, driver } = await start({ spoilAuth: true });
    expect(driver.mode).toBe("snapshot");
    const from = rec.log.length;
    await settle(driver.writeSettings({ brightness: 70 }, twinOpener(demo).open));
    expect(noPolls(rec.names(from))).toEqual([]);
    await settle(driver.disconnect());
  });
});

describe("a HID session (AC-W8SHX9.7)", () => {
  it("refuses the change with the basic-readings wording and opens nothing", async () => {
    const { demo, rec, driver } = await start({ hid: true });
    const pipe = twinOpener(demo);
    const from = rec.log.length;
    const e = await rejection(driver.writeSettings({ brightness: 80 }, pipe.open));
    expect(e.kind).toBe("usage");
    expect(e.message).toBe(hidOnlyBasicText("Changing the meter's settings"));
    expect(pipe.pipes).toEqual([]);
    expect(noPolls(rec.names(from))).toEqual([]);
    await settle(driver.disconnect());
  });
});

describe("the user's click (FR W8SHX9 hardening)", () => {
  it("opens the pipe (the port picker) before waiting behind an in-flight poll", async () => {
    const { demo, rec, driver } = await start();
    // Hold the next stream poll in flight until released.
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    const transact = rec.transact.bind(rec);
    rec.transact = async (frame, timeoutMs) => {
      await held;
      return transact(frame, timeoutMs);
    };
    await vi.advanceTimersByTimeAsync(150); // a poll is now waiting on `held`
    const { open, pipes } = twinOpener(demo);
    const write = driver.writeSettings({ brightness: 42 } as SettingChanges, open);
    for (let i = 0; i < 5; i++) await Promise.resolve();
    expect(pipes).toHaveLength(1);
    release();
    rec.transact = transact;
    const after = await settle(write);
    expect(valueOf(after, "brightness")).toBeDefined();
    expect(pipes[0]?.closed).toBe(true);
    await settle(driver.disconnect());
  });
});

/** FR CJ3QC9: a serial link answered by the demo meter's CDC twin, shared (while
 * `shared`) with another program whose ADC replies come first; `waiting` replies are
 * already held when the pipe starts. */
class SharedLink {
  written: Uint8Array[] = [];
  closed = false;
  shared = true;
  private held: Uint8Array[];
  constructor(
    private twin: DemoCdcTransport,
    waiting: Uint8Array[] = [],
  ) {
    this.held = [...waiting];
  }
  async write(bytes: Uint8Array): Promise<void> {
    this.written.push(bytes.slice());
    if (this.shared) this.held.push(foreignAdcReply(w, 0x5a));
    const reply = await this.twin.transact(bytes, 1_000).catch(() => null);
    if (reply) this.held.push(reply);
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    const next = this.held.shift();
    if (next) return next;
    await new Promise((r) => setTimeout(r, timeoutMs));
    throw timeoutError();
  }
  drain(): void {}
  async close(): Promise<void> {
    this.closed = true;
  }
}

describe("another program on the settings port (FR CJ3QC9)", () => {
  const IN_USE: UiError = {
    kind: "device_busy",
    message: "The meter's serial port is in use by another program.",
    hint: "Close that program, then try again.",
  };
  const pipeOn = (link: SharedLink) =>
    new CdcPipe(w, link as unknown as ConstructorParameters<typeof CdcPipe>[1]);
  const linkWrites = (link: SharedLink) => link.written.filter((f) => f[0] === 0x0a);

  it("stops with the in-use message and sends no write (AC-CJ3QC9.1)", async () => {
    const { demo, rec, driver } = await start();
    const link = new SharedLink(new DemoCdcTransport(demo, () => Date.now()));
    const from = rec.log.length;
    const e = await rejection(driver.writeSettings({ brightness: 80 }, async () => pipeOn(link)));
    expect(e).toEqual(IN_USE);
    expect(linkWrites(link)).toEqual([]);
    expect(noPolls(rec.names(from))).toEqual([]);
    expect(valueOf(await settle(driver.readSettings()), "brightness")).not.toContain("80");
    await settle(driver.disconnect());
  });

  it("replies already waiting when the change starts count the same (AC-CJ3QC9.2)", async () => {
    const { demo, driver } = await start();
    const link = new SharedLink(new DemoCdcTransport(demo, () => Date.now()), [
      foreignAdcReply(w, 0x21),
      foreignAdcReply(w, 0x22),
    ]);
    link.shared = false;
    const e = await rejection(driver.writeSettings({ brightness: 80 }, async () => pipeOn(link)));
    expect(e).toEqual(IN_USE);
    expect(linkWrites(link)).toEqual([]);
    await settle(driver.disconnect());
  });

  it("the refusal closes the pipe and the next change on a clean port takes (AC-CJ3QC9.3)", async () => {
    const { demo, rec, driver, errors } = await start();
    const busy = new SharedLink(new DemoCdcTransport(demo, () => Date.now()));
    expect(
      await rejection(driver.writeSettings({ brightness: 80 }, async () => pipeOn(busy))),
    ).toEqual(IN_USE);
    expect(busy.closed).toBe(true);
    // The stream goes on.
    const polls = rec.log.length;
    await vi.advanceTimersByTimeAsync(500);
    expect(rec.log.length).toBeGreaterThan(polls);
    const clean = new SharedLink(new DemoCdcTransport(demo, () => Date.now()));
    clean.shared = false;
    const after = await settle(driver.writeSettings({ brightness: 80 }, async () => pipeOn(clean)));
    expect(valueOf(after, "brightness")).toContain("80");
    expect(pipeKinds(clean.written)).toEqual(["read", "read", "write", "read"]);
    expect(clean.closed).toBe(true);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("with nobody else on the port the change works as before (AC-CJ3QC9.4)", async () => {
    const { demo, rec, driver } = await start();
    const clean = new SharedLink(new DemoCdcTransport(demo, () => Date.now()));
    clean.shared = false;
    const from = rec.log.length;
    const after = await settle(driver.writeSettings({ brightness: 80 }, async () => pipeOn(clean)));
    expect(valueOf(after, "brightness")).toContain("80");
    expect(pipeKinds(clean.written)).toEqual(["read", "read", "write", "read"]);
    expect(noPolls(rec.names(from))).toEqual(["stop_queue", "start_queue:2"]);
    await settle(driver.disconnect());
  });
});
