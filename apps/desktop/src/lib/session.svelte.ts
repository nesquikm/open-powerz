import type {
  AttachedSession,
  BackendEvent,
  DeviceListing,
  Sample,
  StatsSnapshot,
  UiError,
} from "./backend";
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

  /** The backend zeroed the statistics: show none until the next batch. */
  statsReset(): void {
    this.stats = null;
  }

  /** A session is open or opening (the picker is locked, the button reads Disconnect). */
  get active(): boolean {
    return this.state !== "disconnected" && this.state !== "error";
  }

  /** Connect / Disconnect is available: not while a connect is still pending, and
   * connecting needs a picked device. */
  get canToggleConnect(): boolean {
    if (this.state === "connecting") return false;
    return this.active || this.selectedId !== null;
  }

  /** Replaces the device list; keeps the selection while it is still present. */
  setDevices(devices: readonly DeviceListing[]): void {
    this.devices = sortDevices(devices);
    if (!this.devices.some((d) => d.id === this.selectedId)) {
      this.selectedId = this.devices[0]?.id ?? null;
    }
  }

  select(id: string): void {
    this.selectedId = id;
  }

  /** Takes over a session that was already running when this page loaded (a reload):
   * its device stays picked, even when the device list arrives later. */
  adopt(snapshot: AttachedSession): void {
    this.error = null;
    this.selectedId = snapshot.device_id;
    this.rateSps = snapshot.rate_sps;
    this.state = snapshot.rate_locked ? "rate_locked" : "streaming";
  }

  beginConnect(): void {
    this.error = null;
    this.state = "connecting";
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
          this.error = event.data.error;
          this.state = event.data.error ? "error" : "disconnected";
        } else {
          this.rateSps = event.data.rate_sps;
          this.state = event.data.rate_locked ? "rate_locked" : "streaming";
        }
        break;
      case "error":
        this.error = event.data;
        break;
      default:
        break;
    }
  }
}
