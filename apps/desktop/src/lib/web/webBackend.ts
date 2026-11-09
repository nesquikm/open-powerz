/**
 * FR QFTCX0: the web backend `backend.ts` loads lazily when the build was made with
 * `VITE_BACKEND=web`. It answers the UI's backend calls in the browser.
 */

import type {
  AttachedSession,
  BackendEvent,
  DeviceListing,
  LogoPreview,
  LogoProgress,
  MeterCommands,
  UiError,
  WebBackendApi,
} from "../backend";
import { browserPickDirectory, type PickDirectory } from "./backupFolder";
import { DemoCdcTransport, serialPipeOpener } from "./cdcTransport";
import { DemoTransport } from "./demoTransport";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { WEB_FEATURES, desktopOnlyError, type Features } from "./features";
import { LogoFiles, browserPickLogoFile, type PickLogoFile } from "./logoFiles";
import { SESSION_COMMANDS, WebSession, rateCodeOf, usage } from "./session";
import type { SerialLike } from "./serial";
import type { WebTransport } from "./transport";
import { openWebHid, type HidDeviceLike } from "./webhid";
import { POWERZ_VENDOR_ID, type UsbDeviceLike, type UsbLike } from "./webusb";

/** What the web backend runs on; each part defaults to the browser's own. */
export interface WebBackendOptions {
  /** The wasm protocol module (`src/wasm/powerz_wasm.js`). */
  wasm?: unknown;
  /** `navigator.usb`, or `null` without WebUSB. */
  usb?: unknown;
  /** `navigator.hid`, or `null` without WebHID. */
  hid?: unknown;
  /** The browser's Web Serial API (`navigator` serial), or `null` without it (FR W8SHX9: settings writes). */
  serial?: unknown;
  /** Saves a file for the user (a Blob behind an anchor click). */
  download?: (name: string, data: Blob) => void;
  /** Picks a record delete's backup folder (FR W8SHXA); `null`: browser downloads.
   * Defaults to the browser's `showDirectoryPicker` where it has one. */
  pickDirectory?: PickDirectory | null;
  /** Opens the startup logo's file picker (FR W8SHXB); defaults to the browser's file
   * input. */
  pickLogoFile?: PickLogoFile;
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
  // No meter command: `revealPath` refuses on the web in backend.ts (AC-R13Y5W.1).
  fileReveal: [],
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

/** The parts of a WebHID `HIDDevice` the device list reads. */
interface HidDeviceInfo {
  vendorId: number;
  productName?: string | null;
}

/** The parts of `navigator.hid` the device list uses. */
interface HidNavigatorLike {
  getDevices(): Promise<HidDeviceInfo[]>;
  requestDevice(options: { filters: { vendorId: number }[] }): Promise<HidDeviceInfo[]>;
  addEventListener(type: "connect" | "disconnect", fn: UsbEventListener): void;
  removeEventListener(type: "connect" | "disconnect", fn: UsbEventListener): void;
}

/** The demo meter's scenario, listed after the permitted meters (the picker sorts real
 * meters first, like the desktop app's list). */
const DEMO_SCENARIO = "load-5v-1a";

/** The model a meter lists as when the browser gives no product name. */
const DEFAULT_MODEL = "KM003C";

/** The picker filter, for WebUSB and WebHID alike: POWER-Z meters only. */
const METER_FILTERS = { filters: [{ vendorId: POWERZ_VENDOR_ID }] };

/** A permitted device's model: its product name, else the default. */
const modelOf = (device: { productName?: string | null }): string =>
  device.productName || DEFAULT_MODEL;

/** `navigator[name]`, or `null` outside a browser or without that API. */
function browserApi(name: "usb" | "hid" | "serial"): unknown {
  if (typeof navigator === "undefined") return null;
  return (navigator as { usb?: unknown; hid?: unknown; serial?: unknown })[name] ?? null;
}

const demoListing = (): DeviceListing => ({
  id: `demo:${DEMO_SCENARIO}`,
  label: `Demo device — ${DEMO_SCENARIO}`,
  model: DEFAULT_MODEL,
  serial: null,
  bus: null,
  address: null,
  demo_scenario: DEMO_SCENARIO,
  mode: "application",
});

/** Permitted WebUSB (`webusb:N`) and WebHID (`webhid:N`) meters, each under a stable id
 * for as long as the page lives. */
class DeviceList {
  private ids = new Map<object, string>();
  private next = { webusb: 1, webhid: 1 };

  constructor(
    private readonly usb: UsbNavigatorLike | null,
    readonly hid: HidNavigatorLike | null,
  ) {}

  /** The id of `device`, assigned on first sight. */
  private idOf(device: object, prefix: "webusb" | "webhid"): string {
    let id = this.ids.get(device);
    if (!id) {
      id = `${prefix}:${this.next[prefix]++}`;
      this.ids.set(device, id);
    }
    return id;
  }

  /** The device listed as `id`, if the page has seen one. */
  deviceOf(id: string): object | null {
    for (const [device, known] of this.ids) if (known === id) return device;
    return null;
  }

  /** The permitted WebHID meters (vendor 0x5FC9). */
  async hidMeters(): Promise<HidDeviceInfo[]> {
    const devices = this.hid ? await this.hid.getDevices() : [];
    return devices.filter((d) => d.vendorId === POWERZ_VENDOR_ID);
  }

  hidListing(device: HidDeviceInfo): DeviceListing {
    const model = modelOf(device);
    return {
      id: this.idOf(device, "webhid"),
      label: `${model} (HID)`,
      model,
      serial: null,
      bus: null,
      address: null,
      demo_scenario: null,
      mode: "application",
    };
  }

  listing(device: UsbDeviceInfo): DeviceListing {
    const model = modelOf(device);
    const serial = device.serialNumber || null;
    return {
      id: this.idOf(device, "webusb"),
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
    const hidMeters = await this.hidMeters();
    return [
      ...meters.map((d) => this.listing(d)),
      ...hidMeters.map((d) => this.hidListing(d)),
      demoListing(),
    ];
  }

  /** Opens the picker: WebUSB's, or WebHID's in a browser without WebUSB.
   * `requestDevice` runs before any await, so the click's user activation still holds. */
  request(): Promise<DeviceListing | null> {
    if (!this.usb) return this.requestHid();
    return this.usb.requestDevice(METER_FILTERS).then(
      (device) => this.listing(device),
      () => null,
    );
  }

  /** Opens the WebHID picker, before any await (AC-QFTCWZ.3). */
  requestHid(): Promise<DeviceListing | null> {
    if (!this.hid) return Promise.resolve(null);
    return this.hid.requestDevice(METER_FILTERS).then(
      (devices) => {
        const meter = devices.find((d) => d.vendorId === POWERZ_VENDOR_ID);
        return meter ? this.hidListing(meter) : null;
      },
      () => null,
    );
  }

  async watch(onChange: (devices: DeviceListing[]) => void): Promise<() => void> {
    const sources = [this.usb, this.hid].filter((n) => n !== null);
    if (sources.length === 0) return () => {};
    let live = true;
    const refresh = () => {
      void this.list().then(
        (list) => {
          if (live) onChange(list);
        },
        () => {},
      );
    };
    for (const nav of sources) {
      nav.addEventListener("connect", refresh);
      nav.addEventListener("disconnect", refresh);
    }
    return () => {
      live = false;
      for (const nav of sources) {
        nav.removeEventListener("connect", refresh);
        nav.removeEventListener("disconnect", refresh);
      }
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

/** An opened meter: its transport, and whether it runs over WebHID. */
interface OpenedMeter {
  transport: WebTransport;
  /** The demo meter's transport, whose CDC twin carries its settings writes. */
  demo?: DemoTransport;
  hid?: boolean;
  model?: string;
}

/** The live sessions: one per meter, the newest being the current one. */
class Sessions {
  private list: WebSession[] = [];
  private nextSessionId = 1;
  private wasm: PowerzWasm | null;

  constructor(
    private readonly devices: DeviceList,
    private readonly usb: UsbNavigatorLike | null,
    wasm: unknown,
    private readonly download: (name: string, data: Blob) => void,
    private readonly webSerial: SerialLike | null,
    private readonly pickDirectory: PickDirectory | null = null,
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

  async module(): Promise<PowerzWasm> {
    this.wasm ??= await loadBrowserWasm();
    return this.wasm;
  }

  /** The transport to `deviceId`: the demo meter, or a permitted WebUSB or WebHID
   * meter. A WebUSB meter whose interface another driver holds falls back to an
   * already-permitted HID meter (AC-QFTCWZ.3). */
  private async open(w: PowerzWasm, deviceId: string): Promise<OpenedMeter> {
    if (deviceId.startsWith("demo:")) {
      const demo = new DemoTransport(w, deviceId.slice("demo:".length), () => Date.now());
      return { transport: demo, demo };
    }
    const device = this.devices.deviceOf(deviceId);
    if (!device) throw usage("This meter is no longer available. Pick it again.");
    if (deviceId.startsWith("webhid:")) return this.openHid(device as HidDeviceInfo);
    const options = this.usb ? { usb: this.usb as unknown as UsbLike } : {};
    const opened = await openUsbOrOfferHid(device as unknown as UsbDeviceLike, options);
    if (!("offerHid" in opened)) return { transport: opened.transport };
    if (!this.devices.hid) throw opened.error;
    const [hidMeter] = await this.devices.hidMeters();
    if (!hidMeter) {
      throw {
        ...opened.error,
        hint: "Pick “Connect over HID…” in the device menu for basic readings, or close the other app and connect again.",
      } satisfies UiError;
    }
    try {
      return await this.openHid(hidMeter);
    } catch {
      throw opened.error;
    }
  }

  private async openHid(device: HidDeviceInfo): Promise<OpenedMeter> {
    const transport = await openWebHid(device as unknown as HidDeviceLike);
    return { transport, hid: true, model: modelOf(device) };
  }

  async connect(
    deviceId: string,
    rateSps: number,
    onEvent: (event: BackendEvent) => void,
  ): Promise<void> {
    const rateCode = rateCodeOf(rateSps);
    await this.of(deviceId)?.disconnect();
    const w = await this.module();
    const opened = await this.open(w, deviceId);
    const demo = opened.demo;
    const session = new WebSession({
      deviceId,
      sessionId: this.nextSessionId++,
      onEvent,
      wasm: w,
      download: this.download,
      pickDirectory: this.pickDirectory,
      openSettingsPipe: demo
        ? async () => new DemoCdcTransport(demo, () => Date.now())
        : serialPipeOpener(this.webSerial, w),
      onEnd: (s) => {
        this.list = this.list.filter((x) => x !== s);
      },
    });
    session.driver = await MeterDriver.connect({
      wasm: w,
      transport: opened.transport,
      rateCode,
      hid: opened.hid === true,
      ...(opened.model ? { model: opened.model } : {}),
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

/** A wasm `BindError` (or any failure) as a `UiError` the UI shows. */
function bindError(e: unknown): UiError {
  if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
    const { kind, message, hint } = e as { kind: unknown; message: unknown; hint?: unknown };
    return {
      kind: kind === "usage" ? "usage" : "protocol",
      message: String(message),
      hint: typeof hint === "string" ? hint : null,
    };
  }
  return { kind: "internal", message: "Something went wrong inside the app.", hint: null };
}

/** The startup logo commands (FR W8SHXB): the file picker and previews in the page, the
 * reads and writes on meter `deviceId`'s driver. */
function logoCommands(sessions: Sessions, files: LogoFiles) {
  const driver = (deviceId: string) => {
    const s = sessions.of(deviceId);
    if (!s) throw usage("No meter is connected.");
    return s.driver;
  };
  const preview = async (make: (w: PowerzWasm) => LogoPreview): Promise<LogoPreview> => {
    const w = await sessions.module();
    try {
      return make(w);
    } catch (e) {
      throw bindError(e);
    }
  };
  return {
    logoChoose: () => files.choose(),
    logoPreview: async (path: string, alpha: boolean) => {
      const file = files.get(path);
      return preview((w) => w.logoPreview(file.bytes, file.isBin, alpha));
    },
    logoPreviewBuiltin: async (name: string) => preview((w) => w.logoPreviewBuiltin(name)),
    logoRead: async (deviceId: string, full: boolean) => driver(deviceId).logoRead(full),
    logoUpload: async (
      deviceId: string,
      path: string,
      alpha: boolean,
      onProgress: (progress: LogoProgress) => void,
    ) => {
      const file = files.get(path);
      return driver(deviceId).logoUpload(
        { bytes: file.bytes, isBin: file.isBin, alpha },
        onProgress,
      );
    },
    logoUploadBuiltin: async (
      deviceId: string,
      name: string,
      onProgress: (progress: LogoProgress) => void,
    ) => driver(deviceId).logoUpload({ builtin: name }, onProgress),
    logoRemove: async (deviceId: string, onProgress: (progress: LogoProgress) => void) =>
      driver(deviceId).logoRemove(onProgress),
    logoSettingsCheck: async (deviceId: string) => driver(deviceId).logoSettingsCheck(),
  };
}

/** Creates the browser backend. */
export async function createWebBackend(options: WebBackendOptions = {}): Promise<WebBackendApi> {
  const usb = (
    options.usb === undefined ? browserApi("usb") : options.usb
  ) as UsbNavigatorLike | null;
  const hid = (
    options.hid === undefined ? browserApi("hid") : options.hid
  ) as HidNavigatorLike | null;
  const devices = new DeviceList(usb, hid);
  const download = options.download ?? browserDownload;
  // Destructured: the Web Serial API is no meter serial number (hideSerial lint).
  const { serial: serialOption } = options;
  const serial = (
    serialOption === undefined ? browserApi("serial") : serialOption
  ) as SerialLike | null;
  const pickDirectory =
    options.pickDirectory === undefined ? browserPickDirectory() : options.pickDirectory;
  const sessions = new Sessions(devices, usb, options.wasm, download, serial, pickDirectory);
  const commands = sessions.commands(() => sessions.current);
  const logoFiles = new LogoFiles(options.pickLogoFile ?? browserPickLogoFile());
  // The desktop-only writes are among `commands`' refused ones (AC-QFTCX0.6).
  type Writes = Pick<WebBackendApi, "firmwareFlash">;
  return Object.assign(Object.create(commands) as MeterCommands & Writes, {
    ...logoCommands(sessions, logoFiles),
    features: WEB_FEATURES,
    listDevices: () => devices.list(),
    watchDevices: (onChange: (devices: DeviceListing[]) => void) => devices.watch(onChange),
    requestMeter: () => devices.request(),
    requestHidMeter: () => devices.requestHid(),
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
