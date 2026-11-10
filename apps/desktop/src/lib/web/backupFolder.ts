/**
 * FR W8SHXA: where a record delete writes its backups before anything is deleted.
 *
 * Chrome and Edge let the page write into a folder the user picks (the File System
 * Access API): each backup is a confirmed file write, like the desktop app's backup
 * folder, under `open-powerz-backups/records-<unix ms>` there. A browser without it
 * falls back to one download per backup file.
 */

import type { UiError } from "../backend";

/** The parts of a `FileSystemDirectoryHandle` the backup uses. */
export interface DirectoryLike {
  readonly name: string;
  getDirectoryHandle(name: string, options: { create: boolean }): Promise<DirectoryLike>;
  getFileHandle(name: string, options: { create: boolean }): Promise<FileLike>;
}

/** The parts of a `FileSystemFileHandle` the backup uses. */
export interface FileLike {
  createWritable(): Promise<{ write(data: string): Promise<void>; close(): Promise<void> }>;
}

/** `window.showDirectoryPicker`, as the backup calls it. */
export type PickDirectory = (options: { mode: "readwrite"; id: string }) => Promise<DirectoryLike>;

/** Where the backups of one delete go: `save` settles once a file is written. */
export interface BackupSink {
  /** The folder as the answer names it. */
  readonly label: string;
  save(name: string, csv: string): Promise<void>;
}

/** The downloads fallback's folder label (the browser picks the folder). */
export const DOWNLOADS_LABEL = "your browser's downloads";

const noFolder = (): UiError => ({
  kind: "usage",
  message: "No backup folder was chosen, so nothing was deleted.",
  hint: "Pick a folder for the backup, then delete again.",
});

/** `window.showDirectoryPicker`, or `null` without the File System Access API. */
export function browserPickDirectory(): PickDirectory | null {
  if (typeof window === "undefined") return null;
  const pick = (window as unknown as { showDirectoryPicker?: PickDirectory }).showDirectoryPicker;
  return typeof pick === "function" ? pick.bind(window) : null;
}

/** The backup sink of one delete: a picked folder when `pick` is given (call it from
 * the user's click: the picker needs it), else browser downloads through `download`. */
export async function openBackupSink(
  pick: PickDirectory | null,
  download: (name: string, data: Blob) => void,
  now: () => number = Date.now,
): Promise<BackupSink> {
  if (!pick) {
    return {
      label: DOWNLOADS_LABEL,
      save: async (name, csv) => download(name, new Blob([csv], { type: "text/csv" })),
    };
  }
  let root: DirectoryLike;
  try {
    root = await pick({ mode: "readwrite", id: "open-powerz-backups" });
  } catch {
    throw noFolder();
  }
  const sub = `records-${now()}`;
  const folder = await (
    await root.getDirectoryHandle("open-powerz-backups", { create: true })
  ).getDirectoryHandle(sub, { create: true });
  return {
    label: `${root.name}/open-powerz-backups/${sub}`,
    save: async (name, csv) => {
      const file = await folder.getFileHandle(name, { create: true });
      const out = await file.createWritable();
      await out.write(csv);
      await out.close();
    },
  };
}
