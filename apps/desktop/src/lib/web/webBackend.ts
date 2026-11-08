/**
 * FR QFTCX0: the web backend `backend.ts` loads lazily when the build was made with
 * `VITE_BACKEND=web`. It answers the UI's backend calls in the browser.
 */

import type {
  AttachedSession,
  BackendEvent,
  DeviceListing,
  MeterCommands,
  UiError,
  WebBackendApi,
} from "../backend";
import { DemoTransport } from "./demoTransport";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { WEB_FEATURES, desktopOnlyError, type Features } from "./features";
import { SESSION_COMMANDS, WebSession, rateCodeOf, usage } from "./session";
import type { WebTransport } from "./transport";
import { POWERZ_VENDOR_ID, type UsbDeviceLike, type UsbLike } from "./webusb";

/** What the web backend runs on; each part defaults to the browser's own. */
export interface WebBackendOptions {
  /** The wasm protocol module (`src/wasm/powerz_wasm.js`). */
  wasm?: unknown;
  /** `navigator.usb`, or `null` without WebUSB. */
  usb?: unknown;
  /** Saves a file for the user (a Blob behind an anchor click). */
  download?: (name: string, data: Blob) => void;
}

const notReady = (name: string): UiError => ({
  kind: "internal",
  message: `${name} is not available in the browser yet.`,
  hint: null,
});

/** The commands each switchable feature covers (AC-QFTCX0.6, .7). */
const FEATURE_COMMANDS: Readonly<Record<keyof Features, readonly string[]>> = {
  firmwareUpdate: ["firmwareFlash"],
  logoUpload: ["logoUpload", "logoUploadBuiltin", "logoRemove"],
  settingsWrite: ["meterSettingsWrite"],
  recordDelete: ["recordsDelete"],
  trigger: [
    "triggerOpen",
    "triggerDetect",
    "triggerList",
    "triggerRisk",
    "triggerRequest",
    "triggerArmed",
    "triggerDisarm",
    "triggerChargerGone",
    "triggerPdEntered",
    "triggerBackTo5v",
    "triggerClose",
    "triggerMessageRisk",
    "triggerMessageSend",
    "triggerCableRead",
    "triggerConsoleRisk",
    "triggerConsoleSend",
  ],
  settingsRead: ["meterSettings"],
  recordsList: ["recordsList"],
  recordDownload: ["recordDownload"],
};

/** The commands of the features the web build leaves out: each rejects with a usage
 * error before anything is sent. */
const DESKTOP_ONLY_COMMANDS: readonly string[] = (
  Object.keys(FEATURE_COMMANDS) as (keyof Features)[]
)
  .filter((feature) => !WEB_FEATURES[feature])
  .flatMap((feature) => FEATURE_COMMANDS[feature]);

/** Commands the browser backend does not answer yet: each rejects. */
function pending(): MeterCommands {
  return new Proxy({} as MeterCommands, {
    // Not a thenable: `await`ing the backend must not call `then`.
    get: (_target, name) =>
      name === "then" || typeof name === "symbol"
        ? undefined
        : () => Promise.reject(notReady(String(name))),
  });
}

/** The parts of a WebUSB `USBDevice` the device list reads. */
interface UsbDeviceInfo {
  serialNumber?: string | null;
  productName?: string | null;
}

type UsbEventListener = (event: { device: unknown }) => void;

/** The parts of `navigator.usb` the device list uses. */
interface UsbNavigatorLike {
  getDevices(): Promise<UsbDeviceInfo[]>;
  requestDevice(options: { filters: { vendorId: number }[] }): Promise<UsbDeviceInfo>;
  addEventListener(type: "connect" | "disconnect", fn: UsbEventListener): void;
  removeEventListener(type: "connect" | "disconnect", fn: UsbEventListener): void;
}

/** The demo meter's scenario, listed first like the desktop app's demo entry. */
const DEMO_SCENARIO = "load-5v-1a";

const demoListing = (): DeviceListing => ({
  id: `demo:${DEMO_SCENARIO}`,
  label: `Demo device — ${DEMO_SCENARIO}`,
  model: "KM003C",
  serial: null,
  bus: null,
  address: null,
  demo_scenario: DEMO_SCENARIO,
  mode: "application",
});

/** Permitted WebUSB meters, each under a stable id for as long as the page lives. */
class UsbDeviceList {
  private ids = new Map<UsbDeviceInfo, string>();
  private next = 1;

  constructor(private readonly usb: UsbNavigatorLike | null) {}

  /** The id of `device`, assigned on first sight. */
  idOf(device: UsbDeviceInfo): string {
    let id = this.ids.get(device);
    if (!id) {
      id = `webusb:${this.next++}`;
      this.ids.set(device, id);
    }
    return id;
  }

  /** The device listed as `id`, if the page has seen one. */
  deviceOf(id: string): UsbDeviceInfo | null {
    for (const [device, known] of this.ids) if (known === id) return device;
    return null;
  }

  listing(device: UsbDeviceInfo): DeviceListing {
    const model = device.productName || "KM003C";
    const serial = device.serialNumber || null;
    return {
      id: this.idOf(device),
      label: serial ? `${model} — ${serial}` : model,
      model,
      serial,
      bus: null,
      address: null,
      demo_scenario: null,
      mode: "application",
    };
  }

  async list(): Promise<DeviceListing[]> {
    const meters = this.usb ? await this.usb.getDevices() : [];
    return [...meters.map((d) => this.listing(d)), demoListing()];
  }

  /** Opens the picker. `requestDevice` runs before any await, so the click's user
   * activation still holds. */
  request(): Promise<DeviceListing | null> {
    if (!this.usb) return Promise.resolve(null);
    return this.usb.requestDevice({ filters: [{ vendorId: POWERZ_VENDOR_ID }] }).then(
      (device) => this.listing(device),
      () => null,
    );
  }

  async watch(onChange: (devices: DeviceListing[]) => void): Promise<() => void> {
    const usb = this.usb;
    if (!usb) return () => {};
    let live = true;
    const refresh = () => {
      void this.list().then(
        (list) => {
          if (live) onChange(list);
        },
        () => {},
      );
    };
    usb.addEventListener("connect", refresh);
    usb.addEventListener("disconnect", refresh);
    return () => {
      live = false;
      usb.removeEventListener("connect", refresh);
      usb.removeEventListener("disconnect", refresh);
    };
  }
}

/** The generated wasm module, fetched and initialised on first use. */
async function loadBrowserWasm(): Promise<PowerzWasm> {
  const [mod, wasmUrl] = await Promise.all([
    import("../../wasm/powerz_wasm.js") as unknown as Promise<{
      default: (init: { module_or_path: string }) => Promise<unknown>;
    }>,
    // `?url` makes Vite emit the .wasm as a hashed asset under the build's base path.
    import("../../wasm/powerz_wasm_bg.wasm?url").then((m: { default: string }) => m.default),
  ]);
  await mod.default({ module_or_path: wasmUrl });
  return mod as unknown as PowerzWasm;
}

/** Saves `data` as `name` through an anchor click on a Blob URL. */
function browserDownload(name: string, data: Blob): void {
  const url = URL.createObjectURL(data);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/** The live sessions: one per meter, the newest being the current one. */
class Sessions {
  private list: WebSession[] = [];
  private nextSessionId = 1;
  private wasm: PowerzWasm | null;

  constructor(
    private readonly devices: UsbDeviceList,
    private readonly usb: UsbNavigatorLike | null,
    wasm: unknown,
    private readonly download: (name: string, data: Blob) => void,
  ) {
    this.wasm = (wasm as PowerzWasm | undefined) ?? null;
  }

  get current(): WebSession | null {
    return this.list.at(-1) ?? null;
  }

  /** The connected meters' ids, oldest first. */
  ids(): string[] {
    return this.list.map((s) => s.deviceId);
  }

  of(deviceId: string): WebSession | null {
    return this.list.find((s) => s.deviceId === deviceId) ?? null;
  }

  private async module(): Promise<PowerzWasm> {
    this.wasm ??= await loadBrowserWasm();
    return this.wasm;
  }

  /** The transport to `deviceId`: the demo meter, or a permitted WebUSB meter. */
  private async open(w: PowerzWasm, deviceId: string): Promise<WebTransport> {
    if (deviceId.startsWith("demo:")) {
      return new DemoTransport(w, deviceId.slice("demo:".length), () => Date.now());
    }
    const device = this.devices.deviceOf(deviceId);
    if (!device) throw usage("This meter is no longer available. Pick it again.");
    const options = this.usb ? { usb: this.usb as unknown as UsbLike } : {};
    const opened = await openUsbOrOfferHid(device as unknown as UsbDeviceLike, options);
    if ("offerHid" in opened) throw opened.error;
    return opened.transport;
  }

  async connect(
    deviceId: string,
    rateSps: number,
    onEvent: (event: BackendEvent) => void,
  ): Promise<void> {
    const rateCode = rateCodeOf(rateSps);
    await this.of(deviceId)?.disconnect();
    const w = await this.module();
    const transport = await this.open(w, deviceId);
    const session = new WebSession({
      deviceId,
      sessionId: this.nextSessionId++,
      onEvent,
      wasm: w,
      download: this.download,
      onEnd: (s) => {
        this.list = this.list.filter((x) => x !== s);
      },
    });
    session.driver = await MeterDriver.connect({
      wasm: w,
      transport,
      rateCode,
      onBatch: (samples) => session.onBatch(samples),
      onTemp: (t) => session.onTemp(t),
      onError: (e) => session.onError(e),
    });
    this.list.push(session);
    session.announce();
  }

  attach(onEvent: (event: BackendEvent) => void, deviceId?: string): AttachedSession | null {
    const s = deviceId === undefined ? this.current : this.of(deviceId);
    return s ? s.attach(onEvent) : null;
  }

  /** The session commands of `session()`, or the pending ones for anything else. */
  commands(session: () => WebSession | null): MeterCommands {
    const open = (): WebSession => {
      const s = session();
      if (!s) throw usage("No meter is connected.");
      return s;
    };
    const live = Object.fromEntries(
      SESSION_COMMANDS.map((name) => [
        name,
        (...args: unknown[]) => {
          try {
            const command = open().commands()[name] as (...a: unknown[]) => Promise<unknown>;
            return command(...args);
          } catch (e) {
            return Promise.reject(e as UiError);
          }
        },
      ]),
    );
    const refused = Object.fromEntries(
      DESKTOP_ONLY_COMMANDS.map((name) => [name, () => Promise.reject(desktopOnlyError())]),
    );
    return Object.assign(Object.create(pending()) as MeterCommands, live, refused);
  }
}

/** Creates the browser backend. */
export async function createWebBackend(options: WebBackendOptions = {}): Promise<WebBackendApi> {
  const browserUsb =
    typeof navigator !== "undefined" ? ((navigator as { usb?: unknown }).usb ?? null) : null;
  const usb = (options.usb === undefined ? browserUsb : options.usb) as UsbNavigatorLike | null;
  const devices = new UsbDeviceList(usb);
  const download = options.download ?? browserDownload;
  const sessions = new Sessions(devices, usb, options.wasm, download);
  const commands = sessions.commands(() => sessions.current);
  // The desktop-only writes are among `commands`' refused ones (AC-QFTCX0.6).
  type Writes = Pick<
    WebBackendApi,
    "firmwareFlash" | "logoUpload" | "logoUploadBuiltin" | "logoRemove"
  >;
  return Object.assign(Object.create(commands) as MeterCommands & Writes, {
    features: WEB_FEATURES,
    listDevices: () => devices.list(),
    watchDevices: (onChange: (devices: DeviceListing[]) => void) => devices.watch(onChange),
    requestMeter: () => devices.request(),
    connect: (deviceId: string, rateSps: number, onEvent: (event: BackendEvent) => void) =>
      sessions.connect(deviceId, rateSps, onEvent),
    attach: async (onEvent: (event: BackendEvent) => void, deviceId?: string) =>
      sessions.attach(onEvent, deviceId),
    meter: (deviceId: string) => sessions.commands(() => sessions.of(deviceId)),
    connectedMeters: async () => sessions.ids(),
    saveChartImage: async (
      suggestedName: string,
      format: "png" | "svg" | "pdf",
      data: Uint8Array,
    ) => {
      // The desktop backend wraps the PNG in a PDF page; the browser has no such writer.
      if (format === "pdf") throw desktopOnlyError();
      const name = suggestedName.endsWith(`.${format}`)
        ? suggestedName
        : `${suggestedName}.${format}`;
      const type = format === "png" ? "image/png" : "image/svg+xml";
      download(name, new Blob([data as BlobPart], { type }));
      return name;
    },
  });
}
