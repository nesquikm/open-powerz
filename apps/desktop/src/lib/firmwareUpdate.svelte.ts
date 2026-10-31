/**
 * The firmware view's state (FR V9SK55). The meter's mode and versions, the chosen
 * file's metadata, and whether Update can start: only for a file that passed inspection
 * and fits the meter's model (AC .2).
 */

import type {
  ConnectedInfo,
  DeviceListing,
  DeviceMode,
  FirmwareFileInfo,
  FlashDone,
  FlashEvent,
  FlashReplug,
  FlashStage,
  FlashStatus,
  UiError,
} from "./backend";

/** The meter an update would go to: the picked device, with its versions while connected. */
export interface FirmwareMeter {
  id: string;
  model: string;
  mode: DeviceMode;
  /** Null while the meter is not connected (its versions are not known). */
  firmwareVersion: string | null;
  bootloaderVersion: string | null;
}

/** What the view needs from the backend. */
export interface FirmwareUpdateApi {
  /** The open dialog; null when it was cancelled. */
  choose: () => Promise<string | null>;
  inspect: (path: string) => Promise<FirmwareFileInfo>;
  flash: (
    deviceId: string,
    path: string,
    confirmation: string,
    onStage: (stage: FlashEvent) => void,
  ) => Promise<FlashDone>;
  /** Asks the running update to stop. */
  cancelFlash?: () => Promise<void>;
  /** The update the backend is running, null when none. */
  flashStatus?: () => Promise<FlashStatus | null>;
  /** A recording is running on meter `deviceId`. */
  isRecording?: (deviceId: string) => boolean;
  /** Ends the recording on meter `deviceId`. */
  stopRecording?: (deviceId: string) => Promise<void>;
}

/** The picked device (`listing`) as the update's meter, with the connected meter's
 * versions from `details` (null while none is connected); null when nothing is picked. */
export function flashMeter(
  listing: DeviceListing | undefined,
  details: ConnectedInfo | null,
): FirmwareMeter | null {
  if (listing === undefined) return null;
  return {
    id: listing.id,
    model: listing.model,
    mode: listing.mode,
    firmwareVersion: details ? details.firmware_version || null : null,
    bootloaderVersion: details ? details.bootloader_version || null : null,
  };
}

const MODE_LABELS: Record<DeviceMode, string> = {
  application: "Application",
  bootloader: "Bootloader",
  unknown: "Unknown",
};

/** The meter's model, mode and versions for display; a dash while a version is unknown. */
export function firmwareMeterRows(meter: FirmwareMeter): { label: string; value: string }[] {
  return [
    { label: "Model", value: meter.model },
    { label: "Mode", value: MODE_LABELS[meter.mode] },
    { label: "Firmware", value: meter.firmwareVersion ?? "—" },
    { label: "Bootloader", value: meter.bootloaderVersion ?? "—" },
  ];
}

/** The update's stages in flash order (AC .4). */
export const FLASH_STAGES: readonly FlashStage["stage"][] = [
  "switching",
  "erasing",
  "writing",
  "verifying",
  "finished",
  "returning",
];

/** The warning shown with the typed confirmation (AC .3). */
export const UPDATE_WARNING =
  "Keep the meter connected and the laptop on power until the update finishes, and unplug the meter only when the screen asks you to replug it: an interrupted update leaves the meter in its bootloader.";

/** Why Cancel is off once the erase has begun (AC-4PJRVJ.1). */
export const CANCEL_TOO_LATE = "The erase has started, so the update can no longer be cancelled.";

/** Why Cancel is off once it was asked for: the meter is on its way to the bootloader. */
export const CANCEL_PENDING = "Stopping: the update ends once the meter has switched.";

/** What a reloaded page says when the update it followed has ended: the outcome went to
 * the page that started it. */
export const FOLLOWED_UPDATE_ENDED =
  "The update has ended, but its result went to the page before the reload. Check the firmware version; a meter that stays in its bootloader needs the update run again.";

/** What a reloaded page says when the backend stops answering: the update may still run. */
export const FOLLOWED_UPDATE_LOST =
  "The app lost track of the update, and it may still be running. Leave the meter plugged in for a few minutes, then check the firmware version.";

/** The model a reloaded page shows for the meter being updated: only its id is known. */
const FOLLOWED_METER_MODEL = "Meter being updated";

/** How to recover the meter after a failed update (AC .5). */
export const RECOVERY_STEPS: readonly string[] = [
  "Unplug the meter, then hold Back while plugging it in again to start it in its bootloader.",
  "Retry the update with the same file.",
];

/** Byte counts with thousands separators, whatever the page's locale. */
const BYTES = new Intl.NumberFormat("en-US");

/** A stage's name for the progress view (AC .4); the write stage with bytes done / total. */
export function stageLabel(stage: FlashStage): string {
  switch (stage.stage) {
    case "switching":
      return "Switching to the bootloader";
    case "erasing":
      return "Erasing the application area";
    case "writing":
      return `Writing ${BYTES.format(stage.done)} / ${BYTES.format(stage.total)} bytes`;
    case "verifying":
      return "Verifying the image";
    case "finished":
      return "Image accepted";
    case "returning":
      return "Returning to the application";
  }
}

/** The replug prompt: if the meter does not restart by itself after a jump, the user
 * unplugs it and plugs it back in (AC .4). */
export function replugPrompt(replug: Omit<FlashReplug, "stage">): string {
  return `Unplug the meter and plug it back in (no buttons) if it does not restart by itself. Waiting for ${replug.to} mode.`;
}

/** The label of the stage `name`; the write stage with `progress`, without counts while none. */
export function stageNameLabel(
  name: FlashStage["stage"],
  progress: { done: number; total: number } | null,
): string {
  if (name !== "writing") return stageLabel({ stage: name });
  return progress === null ? "Writing the image" : stageLabel({ stage: "writing", ...progress });
}

/** How often a reloaded page asks the backend about the running update (AC-4PJRVJ.3). */
const STATUS_POLL_MS = 500;
/** Failed polls in a row after which a reloaded page stops following the update. */
const STATUS_POLL_TRIES = 20;

export type FirmwareUpdatePhase = "idle" | "confirming" | "updating" | "done" | "failed";

export class FirmwareUpdate {
  #api: FirmwareUpdateApi;
  /** The chosen file, null until one is chosen. */
  path = $state<string | null>(null);
  /** The chosen file's metadata, null when none is chosen or it failed inspection. */
  info = $state.raw<FirmwareFileInfo | null>(null);
  /** Why the chosen file failed inspection, null when it passed. */
  fileError = $state<string | null>(null);
  phase = $state<FirmwareUpdatePhase>("idle");
  confirmation = $state("");
  stage = $state.raw<FlashStage | null>(null);
  result = $state.raw<FlashDone | null>(null);
  error = $state.raw<UiError | null>(null);
  failedStage = $state<FlashStage["stage"] | null>(null);
  /** The replug the update waits for, null when none (AC .4). */
  replug = $state.raw<Omit<FlashReplug, "stage"> | null>(null);
  /** The update waits for the user: its meter is recording (AC-4PJRVJ.2). */
  recordingPrompt = $state(false);
  /** The agreed recording is being ended. */
  #endingRecording = false;
  /** The meter as it was when the update started; the update disconnects it. */
  #updating = $state.raw<FirmwareMeter | null>(null);
  /** Cancel was accepted: the update stops at its next safe point. */
  cancelling = $state(false);
  /** The update a reloaded page followed has ended (AC-4PJRVJ.3). */
  followedEnded = $state(false);
  /** A reloaded page stopped hearing about the update it followed (AC-4PJRVJ.3). */
  followedLost = $state(false);

  constructor(api: FirmwareUpdateApi) {
    this.#api = api;
  }

  /** An update is running: the meter is held for it (AC .4). */
  get busy(): boolean {
    return this.phase === "updating";
  }

  /** The meter to show: as it was at the start while the update runs, since the update
   * disconnects it and its versions are no longer read; else `meter` as it is now. */
  meterShown(meter: FirmwareMeter | null): FirmwareMeter | null {
    if (!this.busy || this.#updating === null) return meter;
    const followed = this.#updating.model === FOLLOWED_METER_MODEL;
    return followed && meter?.id === this.#updating.id ? meter : this.#updating;
  }

  /** The typed text is exactly the chosen file's version. */
  get confirmed(): boolean {
    return this.info !== null && this.confirmation === this.info.version;
  }

  /** The latest write progress, kept after the write stage; null before it. */
  writeProgress = $state.raw<{ done: number; total: number } | null>(null);

  /** Opens the dialog and inspects the chosen file; a cancelled dialog changes nothing. */
  async chooseFile(): Promise<void> {
    if (this.#locked()) return;
    const path = await this.#api.choose();
    if (path === null || this.#locked()) return;
    this.path = path;
    try {
      this.info = await this.#api.inspect(path);
      this.fileError = null;
    } catch (e) {
      this.info = null;
      this.fileError = (e as UiError | null)?.message ?? String(e);
    }
  }

  /** The file is fixed: the confirmation is open or the update runs. */
  #locked(): boolean {
    return this.phase === "updating" || this.phase === "confirming";
  }

  /** The failure came after the meter was touched: show the recovery steps. A refusal
   * (usage error) or a failure before any stage sent nothing to the meter. */
  get needsRecovery(): boolean {
    return this.error !== null && this.error.kind !== "usage" && this.failedStage !== null;
  }

  /** Why Update cannot start for `meter`, or null when it can. */
  updateBlocker(meter: FirmwareMeter | null): string | null {
    const info = this.info;
    if (info === null) return "Choose a firmware file to update with.";
    if (meter === null) return "Pick a meter to update.";
    if (info.model.toUpperCase() !== meter.model.toUpperCase()) {
      return `This file is for the ${info.model}, but the meter is a ${meter.model}.`;
    }
    return null;
  }

  /** Update is enabled: an inspected file whose model matches the meter's. */
  canUpdate(meter: FirmwareMeter | null): boolean {
    return this.updateBlocker(meter) === null;
  }

  /** Opens the typed confirmation (AC .3); a blocked Update does nothing. */
  requestUpdate(meter: FirmwareMeter | null): void {
    if (this.phase !== "idle" || !this.canUpdate(meter)) return;
    this.confirmation = "";
    this.phase = "confirming";
  }

  /** Starts the update once the target version was typed. */
  async start(meter: FirmwareMeter | null): Promise<void> {
    if (meter === null || !this.#startable(meter)) return;
    if (this.#api.isRecording?.(meter.id)) {
      this.recordingPrompt = true;
      return;
    }
    await this.#run(meter);
  }

  /** Back to the confirmation: the recording goes on and the update does not start. */
  declineEndRecording(): void {
    if (!this.#endingRecording) this.recordingPrompt = false;
  }

  /** Ends the recording on `meter`, then starts the update. A failed stop leaves the
   * update unstarted: nothing has reached the meter. */
  async confirmEndRecording(meter: FirmwareMeter | null): Promise<void> {
    if (!this.recordingPrompt || this.#endingRecording || meter === null) return;
    this.#endingRecording = true;
    try {
      await this.#api.stopRecording?.(meter.id);
    } catch (e) {
      this.#clearOutcome();
      this.error = (e as UiError | null) ?? { kind: "internal", message: String(e), hint: null };
      this.phase = "failed";
      return;
    } finally {
      this.#endingRecording = false;
      this.recordingPrompt = false;
    }
    if (this.#startable(meter)) await this.#run(meter);
  }

  /** The confirmation is open, its version typed, and `meter` fits the file. */
  #startable(meter: FirmwareMeter): boolean {
    return (
      this.phase === "confirming" && this.confirmed && this.path !== null && this.canUpdate(meter)
    );
  }

  /** Runs the update to its end. */
  async #run(meter: FirmwareMeter): Promise<void> {
    const path = this.path;
    if (path === null) return;
    this.#updating = meter;
    this.phase = "updating";
    this.#clearOutcome();
    try {
      this.result = await this.#api.flash(meter.id, path, this.confirmation, (stage) => {
        if (stage.stage === "replug") {
          this.replug = { to: stage.to, reply: stage.reply };
          return;
        }
        this.replug = null;
        this.#showStage(stage);
      });
      this.replug = null;
      this.phase = "done";
    } catch (e) {
      this.replug = null;
      this.error = (e as UiError | null) ?? {
        kind: "internal",
        message: String(e),
        hint: null,
      };
      // Set by the stage callback, which the narrowing above does not see.
      const last = this.stage as FlashStage | null;
      this.failedStage = last?.stage ?? null;
      this.phase = "failed";
    }
  }

  /** Shows `stage` as the update's latest. */
  #showStage(stage: FlashStage): void {
    this.stage = stage;
    if (stage.stage === "writing") {
      this.writeProgress = { done: stage.done, total: stage.total };
    }
  }

  /** Picks up an update that outlived the page (AC-4PJRVJ.3): holds the meter and shows
   * the backend's stage until its update ends. No answer means no update to pick up. */
  async reattach(): Promise<void> {
    const status = await this.#flashStatus().catch(() => null);
    if (status === null || this.phase !== "idle") return;
    this.phase = "updating";
    this.#clearOutcome();
    this.#updating = {
      id: status.device_id,
      model: FOLLOWED_METER_MODEL,
      mode: "unknown",
      firmwareVersion: null,
      bootloaderVersion: null,
    };
    if (status.stage !== null) this.#showStage(status.stage);
    void this.#followBackend();
  }

  /** The update the backend is running, null when none. */
  async #flashStatus(): Promise<FlashStatus | null> {
    return (await this.#api.flashStatus?.()) ?? null;
  }

  /** Follows the backend's update to its end, then says so: its outcome went to the page
   * that started it. A failed poll says nothing: the next one decides, up to
   * {@link STATUS_POLL_TRIES} in a row, and then all it can say is that it lost track. */
  async #followBackend(): Promise<void> {
    let failed = 0;
    while (failed < STATUS_POLL_TRIES) {
      await new Promise((resolve) => setTimeout(resolve, STATUS_POLL_MS));
      try {
        const status = await this.#flashStatus();
        if (status === null) break;
        failed = 0;
        if (status.stage !== null) this.#showStage(status.stage);
      } catch {
        failed += 1;
      }
    }
    this.phase = "idle";
    this.#clearOutcome();
    if (failed < STATUS_POLL_TRIES) this.followedEnded = true;
    else this.followedLost = true;
  }

  /** Closes the confirmation without sending anything. */
  cancel(): void {
    if (this.phase !== "confirming") return;
    this.phase = "idle";
    this.confirmation = "";
  }

  /** Why the running update can no longer be cancelled, null while it can. */
  get cancelBlocker(): string | null {
    const stage = this.stage?.stage;
    if (stage !== undefined && stage !== "switching") return CANCEL_TOO_LATE;
    return this.cancelling ? CANCEL_PENDING : null;
  }

  /** Asks the running update to stop; it then ends as a failure. A refusal is dropped:
   * it means the erase has just begun, and that stage's report turns Cancel off. */
  async cancelUpdate(): Promise<void> {
    if (this.phase !== "updating" || this.cancelBlocker !== null) return;
    try {
      await this.#api.cancelFlash?.();
      this.cancelling = true;
    } catch {
      // See above.
    }
  }

  /** Clears a finished or failed update's outcome; the chosen file stays for a retry. */
  dismiss(): void {
    if (this.phase !== "done" && this.phase !== "failed") return;
    this.phase = "idle";
    this.confirmation = "";
    this.#clearOutcome();
  }

  /** Forgets the last update's stages, progress and outcome. */
  #clearOutcome(): void {
    this.stage = null;
    this.writeProgress = null;
    this.result = null;
    this.error = null;
    this.failedStage = null;
    this.replug = null;
    this.cancelling = false;
    this.followedEnded = false;
    this.followedLost = false;
  }
}
