import type {
  AttachedSession,
  BackendEvent,
  ConnectedInfo,
  DeviceListing,
  Sample,
  StatsSnapshot,
  UiError,
} from "./backend";
import { canConnectTo } from "./deviceDetails";
import { sortDevices, type ConnectionState } from "./format";

/** Reactive UI state of the live session. */
export class Session {
  /** Connection state for the top-bar badge. */
  state = $state<ConnectionState>("disconnected");
  /** Last error, null when none. */
  error = $state<UiError | null>(null);
  /** Devices in the picker (real meters first), kept current by hotplug. */
  devices = $state<DeviceListing[]>([]);
  /** Id of the picked device, null when the list is empty. */
  selectedId = $state<string | null>(null);
  /** Latest sample, null while disconnected. */
  latest = $state<Sample | null>(null);
  /** Latest temperature, null when not reported or disconnected. */
  tempC = $state<number | null>(null);
  /** Measured queue rate. */
  rateSps = $state<number | null>(null);
  /** Statistics of the latest batch, null until the first one. They stay after a
   * disconnect, so the final totals remain readable. */
  stats = $state<StatsSnapshot | null>(null);
  /** Id of the real meter picked by hand or connected (FR NWGY72): when it leaves the
   * list while no session runs, it stays picked as not connected. */
  #chosenMeterId: string | null = null;
  /** The pick was made by the list fallback, not by the user (FR NWGY72, AC .2). */
  #autoPicked = false;
  /** Device whose session the statistics belong to (FR R13Y5W). */
  #statsDeviceId: string | null = null;
  /** The connected meter's details (mode, versions, firmware warnings), null while
   * no meter is connected. */
  details = $state.raw<ConnectedInfo | null>(null);
  /** Serial of the unplugged meter the backend waits for, null when none. */
  waitingSerial = $state<string | null>(null);
  /** The meter stopped filling the queue (FR NVN6M8): the view shows `STALL_NOTICE`. */
  stalled = $state(false);
  /** A firmware update holds the meter (FR V9SK55, AC .4): no switching, no connect. */
  flashing = $state(false);

  /** The backend zeroed the statistics: show none until the next batch. */
  statsReset(): void {
    this.stats = null;
  }

  /** A meter streams, so its settings and records can be read: also a session this page
   * adopted after a reload, which has no connect details. */
  get meterConnected(): boolean {
    return this.state === "streaming" || this.state === "rate_locked";
  }

  /** A session is open or opening (the picker is locked, the button reads Disconnect). */
  get active(): boolean {
    return this.state !== "disconnected" && this.state !== "error" && this.state !== "waiting";
  }

  /** Connect / Disconnect is available: not while a connect is still pending, and
   * connecting needs a picked device. */
  get canToggleConnect(): boolean {
    if (this.flashing || this.state === "connecting") return false;
    if (this.active) return true;
    // A meter in bootloader mode is listed but never connected (FR V9SK52, AC .5).
    const picked = this.devices.find((d) => d.id === this.selectedId);
    return picked === undefined ? this.selectedId !== null : canConnectTo(picked);
  }

  /** Replaces the device list; keeps the selection while it is still present. While the
   * backend waits for an unplugged meter, that meter stays listed and picked, and follows
   * it to its new listing when the OS lists it again. Once its session has ended, a picked
   * meter that vanishes follows a new listing with the same non-empty serial (a browser
   * replug) and the dead entry goes (FR R13Y5V). With no such listing, a picked meter that
   * vanishes while its session runs or has just ended with an error stays: the hotplug
   * list can arrive before the waiting status, and the fallback must never pick a demo.
   * A meter the user picked or connected that vanishes with no other meter listed stays
   * picked, marked `absent` (shown as not connected), so the pick never moves to a demo
   * on its own; a demo the fallback picked yields to a meter that appears while no
   * session runs (FR NWGY72). */
  setDevices(devices: readonly DeviceListing[]): void {
    let list = sortDevices(devices);
    const meterWithSerial = (serial: string): DeviceListing | undefined =>
      list.find((d) => d.demo_scenario === null && d.serial === serial);
    const away = this.waitingSerial;
    const kept = this.devices.find((d) => d.id === this.selectedId);
    const vanished = kept !== undefined && !list.some((d) => d.id === kept.id);
    const replugged =
      kept?.demo_scenario === null && kept.serial && !this.active && vanished
        ? meterWithSerial(kept.serial)
        : undefined;
    if (away !== null) {
      const back = meterWithSerial(away);
      if (back) this.selectedId = back.id;
      else if (kept && vanished) list = sortDevices([kept, ...list]);
    } else if (replugged) {
      this.selectedId = replugged.id;
    } else if (kept?.demo_scenario === null && this.state !== "disconnected" && vanished) {
      list = sortDevices([kept, ...list]);
    } else if (
      kept?.demo_scenario === null &&
      vanished &&
      kept.id === this.#chosenMeterId &&
      !list.some((d) => d.demo_scenario === null)
    ) {
      // The chosen meter left with no other meter listed: it stays picked as not
      // connected, so the pick never moves to a demo on its own (FR NWGY72, AC .1).
      list = sortDevices([{ ...kept, absent: true }, ...list]);
    } else if (kept?.demo_scenario !== null && this.#autoPicked && !this.active) {
      // A demo picked automatically yields to a meter that appears (FR NWGY72, AC .2).
      const real = list.find((d) => d.demo_scenario === null && d.absent !== true);
      if (real) this.selectedId = real.id;
    }
    this.devices = list;
    if (!this.devices.some((d) => d.id === this.selectedId)) {
      this.selectedId = this.devices[0]?.id ?? null;
      this.#autoPicked = true;
    }
  }

  /** Picks a device. Picking another device while waiting for an unplugged meter
   * cancels the wait for it; returns true then, so the caller tells the backend. */
  select(id: string): boolean {
    this.selectedId = id;
    this.#autoPicked = false;
    this.#rememberMeter();
    if (this.waitingSerial === null) return false;
    const picked = this.devices.find((d) => d.id === id);
    if (picked?.demo_scenario === null && picked.serial === this.waitingSerial) return false;
    this.waitingSerial = null;
    if (this.state === "waiting") this.state = "disconnected";
    return true;
  }

  /** Takes over a session that was already running when this page loaded (a reload):
   * its device stays picked, even when the device list arrives later. */
  adopt(snapshot: AttachedSession): void {
    this.error = null;
    this.selectedId = snapshot.device_id;
    this.#statsDeviceId = snapshot.device_id;
    this.rateSps = snapshot.rate_sps;
    this.state = snapshot.rate_locked ? "rate_locked" : "streaming";
  }

  beginConnect(): void {
    this.error = null;
    this.stalled = false;
    // Another device's numbers must not sit next to this connect (FR R13Y5W, AC .2).
    if (this.#statsDeviceId !== this.selectedId) this.stats = null;
    this.#statsDeviceId = this.selectedId;
    this.#autoPicked = false;
    this.#rememberMeter();
    this.state = "connecting";
  }

  /** Remembers the picked device when it is a real meter (FR NWGY72). */
  #rememberMeter(): void {
    const picked = this.devices.find((d) => d.id === this.selectedId);
    if (picked?.demo_scenario === null) this.#chosenMeterId = picked.id;
  }

  failConnect(error: UiError): void {
    this.error = error;
    this.state = "error";
  }

  /** Shows an error that leaves the session as it is (a refused rate change or disconnect). */
  reportError(error: UiError): void {
    this.error = error;
  }

  /** Hides the error banner. */
  dismissError(): void {
    this.error = null;
  }

  handleEvent(event: BackendEvent): void {
    switch (event.type) {
      case "batch": {
        // A batch still in flight when the session ended must not bring values back.
        if (!this.active) break;
        const last = event.data.samples.at(-1);
        if (last) this.latest = last;
        this.tempC = event.data.temp_c;
        this.stats = event.data.stats;
        break;
      }
      case "status":
        if (event.data.state === "disconnected") {
          this.latest = null;
          this.tempC = null;
          this.rateSps = null;
          this.details = null;
          this.stalled = false;
          this.error = event.data.error;
          this.state = event.data.error ? "error" : "disconnected";
        } else if (event.data.state === "waiting") {
          this.waitingSerial = event.data.serial;
          this.state = "waiting";
        } else {
          // A meter that came back takes its unplug message with it, whatever the unplug
          // looked like: on macOS a pulled cable can end the session with an I/O error.
          if (this.waitingSerial !== null) this.error = null;
          this.waitingSerial = null;
          if (event.data.state === "connected") this.details = event.data;
          this.rateSps = event.data.rate_sps;
          this.state = event.data.rate_locked ? "rate_locked" : "streaming";
        }
        break;
      case "error":
        this.error = event.data;
        break;
      case "stall":
        this.stalled = event.data.stalled;
        break;
      default:
        break;
    }
  }
}
