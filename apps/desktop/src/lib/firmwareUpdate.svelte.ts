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
  FlashHold,
  FlashReplug,
  FlashStage,
  FlashStatus,
  UiError,
} from "./backend";
import cancelTooLate from "./cancel-too-late.txt?raw";

/** The meter an update would go to: the picked device, with its versions while connected. */
export interface FirmwareMeter {
  id: string;
  model: string;
  mode: DeviceMode;
  /** Null while the meter is not connected (its versions are not known). */
  firmwareVersion: string | null;
  bootloaderVersion: string | null;
  /** The meter's serial, which finds it again under a new id after an update; null or
   * absent when not known. */
  serial?: string | null;
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
    /** The sessions the user agreed to end (FR AXHK2F). */
    agreed?: FlashHold[],
  ) => Promise<FlashDone>;
  /** Asks the running update to stop. */
  cancelFlash: () => Promise<void>;
  /** The update the backend is running, else the ended one it kept, which this read
   * takes; null when neither. */
  flashStatus?: () => Promise<FlashStatus | null>;
  /** Ends the recording on meter `deviceId`. Rejects with the reason, or with `null`
   * while a start or stop of that recording is in flight. */
  stopRecording?: (deviceId: string) => Promise<void>;
  /** Connects meter `deviceId` again, as a plain connect of its listing (AC-AXHK2M.15). */
  reconnect?: (deviceId: string) => Promise<void>;
}

/** Why a recording could not be stopped for the update: one is on its way in or out. */
const RECORDING_BUSY: UiError = {
  kind: "usage",
  message: "A recording is still starting or stopping. Try the update again once it has finished.",
  hint: null,
};

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
    serial: listing.serial,
    firmwareVersion: details ? details.firmware_version || null : null,
    bootloaderVersion: details ? details.bootloader_version || null : null,
  };
}

/** The listing of the updated meter `updated` (see {@link FirmwareUpdate.followTarget}). */
function updatedListing(
  updated: FirmwareMeter,
  listings: readonly DeviceListing[],
): DeviceListing | undefined {
  const demo = listings.find((l) => l.demo_scenario !== null && l.id === updated.id);
  if (demo !== undefined) return demo;
  const real = listings.filter((l) => l.demo_scenario === null);
  const serial = updated.serial ?? null;
  if (serial !== null) return real.find((l) => l.serial === serial);
  const same = real.find((l) => l.id === updated.id);
  if (same !== undefined) return same;
  const waiting = real.filter((l) => l.mode === "bootloader" && l.model === updated.model);
  return waiting.length === 1 ? waiting[0] : undefined;
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

/** Why Cancel is off once the erase has begun (AC-4PJRVJ.1). The backend reads the same
 * file for `ERASE_BEGAN`, so the two cannot drift (AC-AXHK2M.4). */
export const CANCEL_TOO_LATE = cancelTooLate.trim();

/** Why Cancel is off once it was asked for: the update stops at its next safe point. */
export const CANCEL_PENDING = "Stopping: the update ends in a moment.";

/** What a reloaded page says when the update it followed has ended and the backend kept
 * no outcome. */
export const FOLLOWED_UPDATE_ENDED =
  "The update has ended, but the app did not keep its result. Check the firmware version; a meter that stays in its bootloader needs the update run again.";

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

/** What to do after a stop (AC-AXHK2M.14): a plain replug first, which brings most
 * meters back; the button only when that does not. */
export const STOPPED_STEPS: readonly string[] = [
  "Unplug the meter and plug it back in.",
  "If that does not bring it back, unplug it again and hold Back while plugging it in.",
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

/** `stopped`: the user's stop ended the update; not a failure (AC-AXHK2M.13). */
export type FirmwareUpdatePhase =
  "idle" | "confirming" | "updating" | "done" | "failed" | "stopped";

export class FirmwareUpdate {
  #api: FirmwareUpdateApi;
  /** The chosen file, null until one is chosen. */
  path = $state<string | null>(null);
  /** The chosen file's metadata, null when none is chosen or it failed inspection. */
  info = $state.raw<FirmwareFileInfo | null>(null);
  /** Why the chosen file failed inspection, null when it passed. */
  fileError = $state<string | null>(null);
  /** The hint that goes with {@link fileError}. */
  fileErrorHint = $state<string | null>(null);
  phase = $state<FirmwareUpdatePhase>("idle");
  confirmation = $state("");
  stage = $state.raw<FlashStage | null>(null);
  result = $state.raw<FlashDone | null>(null);
  error = $state.raw<UiError | null>(null);
  failedStage = $state<FlashStage["stage"] | null>(null);
  /** The replug the update waits for, null when none (AC .4). */
  replug = $state.raw<Omit<FlashReplug, "stage"> | null>(null);
  /** The sessions the update would end, while it waits for the user to agree; null when
   * it does not wait. The backend names them when it refuses the start (FR AXHK2F). */
  releasePrompt = $state.raw<FlashHold[] | null>(null);
  /** The agreed recordings are being ended. */
  #releasing = false;
  /** The meter as it was when the update started; the update disconnects it. */
  #updating = $state.raw<FirmwareMeter | null>(null);
  /** The ended update's meter was connected again: once per update (AC-AXHK2M.15). */
  #reconnected = false;
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

  /** The meter the running update holds, null when none runs (AC-4PJRVM.4). */
  get updatingId(): string | null {
    return this.busy ? (this.#updating?.id ?? null) : null;
  }

  /** The meter to show: as it was at the start while the update runs, since the update
   * disconnects it and its versions are no longer read; once it ended (stopped or done),
   * the updated meter with the versions it reads, else the one read back by the update,
   * else those kept from the start (AC-AXHK2M.16); else `meter` as it is now. */
  meterShown(meter: FirmwareMeter | null): FirmwareMeter | null {
    if (!this.busy || this.#updating === null) return this.#withKeptVersions(meter);
    const followed = this.#updating.model === FOLLOWED_METER_MODEL;
    return followed && meter?.id === this.#updating.id ? meter : this.#updating;
  }

  /** The update ran to its end, done or stopped by the user (AC-AXHK2M.14-.16). */
  get #ended(): boolean {
    return this.phase === "done" || this.phase === "stopped";
  }

  /** `meter` with the versions an ended update knows filled in where it reads none; only
   * for the updated meter (same id, or same serial when known). */
  #withKeptVersions(meter: FirmwareMeter | null): FirmwareMeter | null {
    const updated = this.#updating;
    if (meter === null || updated === null) return meter;
    if (!this.#ended) return meter;
    const serial = updated.serial ?? null;
    const same = meter.id === updated.id || (serial !== null && meter.serial === serial);
    if (!same) return meter;
    const readBack = this.result?.outcome === "verified" ? this.result.version : null;
    return {
      ...meter,
      firmwareVersion: meter.firmwareVersion ?? readBack ?? updated.firmwareVersion,
      bootloaderVersion: meter.bootloaderVersion ?? updated.bootloaderVersion,
    };
  }

  /** The listing to keep picked once the update has ended (stopped or done): the updated
   * meter, a demo device by its id, a real one by its serial when known, else by its id,
   * else the one real meter of its model listed in bootloader mode, where a stopped update
   * leaves it (AC-AXHK2M.14); null when no update ended or the meter is not listed. Once
   * that meter is listed in application mode it is connected again, once (AC-AXHK2M.15). */
  followTarget(listings: readonly DeviceListing[]): string | null {
    const updated = this.#updating;
    if (updated === null || !this.#ended) return null;
    const target = updatedListing(updated, listings);
    if (target !== undefined && target.mode === "application" && !this.#reconnected) {
      this.#reconnected = true;
      void this.#api.reconnect?.(target.id).catch(() => {});
    }
    return target?.id ?? null;
  }

  /** The typed text is exactly the chosen file's version. */
  get confirmed(): boolean {
    return this.info !== null && this.confirmation === this.info.version;
  }

  /** The latest write progress, kept after the write stage; null before it. */
  writeProgress = $state.raw<{ done: number; total: number } | null>(null);

  /** Opens the dialog and inspects the chosen file; a cancelled dialog changes nothing
   * but the note on a followed update, which any next action clears (AC-AXHK2M.7). */
  async chooseFile(): Promise<void> {
    if (this.#locked()) return;
    this.followedEnded = false;
    const path = await this.#api.choose();
    if (path === null || this.#locked()) return;
    this.path = path;
    try {
      this.info = await this.#api.inspect(path);
      this.fileError = null;
      this.fileErrorHint = null;
    } catch (e) {
      this.info = null;
      const refused = e as UiError | null;
      this.fileError = refused?.message ?? String(e);
      this.fileErrorHint = refused?.hint ?? null;
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
    this.followedEnded = false;
    this.confirmation = "";
    this.phase = "confirming";
  }

  /** Starts the update once the target version was typed. */
  async start(meter: FirmwareMeter | null): Promise<void> {
    if (meter !== null && this.#startable(meter)) await this.#run(meter, []);
  }

  /** Back to the confirmation: every session goes on and the update does not start. */
  declineRelease(): void {
    if (!this.#releasing) this.releasePrompt = null;
  }

  /** Ends each recording of the agreed sessions, then starts the update naming all of
   * them. A failed stop leaves the update unstarted: nothing has reached the meter. */
  async confirmRelease(meter: FirmwareMeter | null): Promise<void> {
    const agreed = this.releasePrompt;
    if (agreed === null || this.#releasing || meter === null) return;
    this.#releasing = true;
    try {
      for (const session of agreed) {
        if (session.recording) await this.#api.stopRecording?.(session.id);
      }
    } catch (e) {
      this.#clearOutcome();
      this.error = (e as UiError | null) ?? RECORDING_BUSY;
      this.phase = "failed";
      return;
    } finally {
      this.#releasing = false;
      this.releasePrompt = null;
    }
    if (this.#startable(meter)) await this.#run(meter, agreed);
  }

  /** The confirmation is open, its version typed, and `meter` fits the file. */
  #startable(meter: FirmwareMeter): boolean {
    return (
      this.phase === "confirming" && this.confirmed && this.path !== null && this.canUpdate(meter)
    );
  }

  /** Runs the update to its end, unless the backend refuses it for a session `agreed`
   * does not cover: that reopens the prompt and nothing has reached the meter. */
  async #run(meter: FirmwareMeter, agreed: FlashHold[]): Promise<void> {
    const path = this.path;
    if (path === null) return;
    this.#updating = meter;
    this.#reconnected = false;
    this.phase = "updating";
    this.#clearOutcome();
    try {
      this.result = await this.#api.flash(
        meter.id,
        path,
        this.confirmation,
        (stage) => {
          if (stage.stage === "replug") {
            this.replug = { to: stage.to, reply: stage.reply };
            return;
          }
          this.replug = null;
          this.#showStage(stage);
        },
        agreed,
      );
      this.replug = null;
      this.phase = "done";
      this.#takeKeptOutcome();
    } catch (e) {
      this.replug = null;
      const refused = e as UiError | null;
      if (refused?.kind === "flash_needs_consent") {
        this.releasePrompt = refused.sessions ?? [];
        this.phase = "confirming";
        return;
      }
      this.#fail(
        refused ?? {
          kind: "internal",
          message: String(e),
          hint: null,
        },
      );
      this.#takeKeptOutcome();
    }
  }

  /** Ends the update as a failure at its latest stage, or as stopped when the user's stop
   * ended it (AC-AXHK2M.13): no failed stage, so no recovery steps. */
  #fail(error: UiError): void {
    this.error = error;
    if (error.kind === "cancelled") {
      this.failedStage = null;
      this.phase = "stopped";
      return;
    }
    this.failedStage = this.stage?.stage ?? null;
    this.phase = "failed";
  }

  /** This page saw the end, so a later reload must not be told again (AC-AXHK2M.2). */
  #takeKeptOutcome(): void {
    void this.#flashStatus().catch(() => null);
  }

  /** Shows `stage` as the update's latest. */
  #showStage(stage: FlashStage): void {
    this.stage = stage;
    if (stage.stage === "writing") {
      this.writeProgress = { done: stage.done, total: stage.total };
    }
  }

  /** Picks up an update that outlived the page (AC-4PJRVJ.3): holds the meter and shows
   * the backend's stage and replug prompt until its update ends. No answer means no
   * update to pick up. */
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
    if (!this.#showStatus(status)) void this.#followBackend();
  }

  /** Shows what the backend reports; true when that is how the update ended (AC-AXHK2M.2). */
  #showStatus(status: FlashStatus): boolean {
    if (status.stage !== null) this.#showStage(status.stage);
    this.replug = status.replug ?? null;
    const outcome = status.outcome ?? null;
    if (outcome === null) return false;
    this.replug = null;
    if (outcome.result !== null) {
      this.result = outcome.result;
      this.phase = "done";
    } else {
      this.#fail(outcome.error);
    }
    return true;
  }

  /** The update the backend reports, null when none. */
  async #flashStatus(): Promise<FlashStatus | null> {
    return (await this.#api.flashStatus?.()) ?? null;
  }

  /** Follows the backend's update to its end and shows the outcome the backend kept, or
   * says it ended when it kept none. A failed poll says nothing: the next one decides, up
   * to {@link STATUS_POLL_TRIES} in a row, and then all it can say is that it lost track. */
  async #followBackend(): Promise<void> {
    let failed = 0;
    while (failed < STATUS_POLL_TRIES) {
      await new Promise((resolve) => setTimeout(resolve, STATUS_POLL_MS));
      try {
        const status = await this.#flashStatus();
        if (status === null) break;
        failed = 0;
        if (this.#showStatus(status)) return;
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

  /** Asks the running update to stop; it then ends as stopped. A refusal is dropped:
   * it means the erase has just begun, and that stage's report turns Cancel off. */
  async cancelUpdate(): Promise<void> {
    if (this.phase !== "updating" || this.cancelBlocker !== null) return;
    try {
      await this.#api.cancelFlash();
      this.cancelling = true;
    } catch {
      // See above.
    }
  }

  /** Clears a finished, failed or stopped update's outcome; the chosen file stays. */
  dismiss(): void {
    if (this.phase !== "done" && this.phase !== "failed" && this.phase !== "stopped") return;
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
