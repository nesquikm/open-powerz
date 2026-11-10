/**
 * FR W8SHXB, AC-W8SHXB.1: the web edition's startup logo file picker. The browser
 * gives no file paths, so a chosen file's bytes stay in the page under an opaque token
 * (`web-file:<n>:<name>`) that the UI passes where the desktop app passes a path.
 */

import type { UiError } from "../backend";

/** The file types the picker offers: the desktop dialog's PNG, JPEG, BMP and raw
 * `.bin` logo files. */
export const LOGO_ACCEPT = "image/png,image/jpeg,image/bmp,.bin";

/** A file the user chose. */
export interface PickedLogoFile {
  name: string;
  bytes: Uint8Array;
}

/** Opens the browser's file picker; `null` when it was cancelled. */
export type PickLogoFile = () => Promise<PickedLogoFile | null>;

/** A kept file: its name, bytes and whether it is a raw `.bin` logo file. */
export interface LogoFile extends PickedLogoFile {
  isBin: boolean;
}

interface FileLike {
  name: string;
  /** Bytes, when the browser reports it (a `File` does). */
  size?: number;
  arrayBuffer(): Promise<ArrayBuffer>;
}

interface FileInputLike {
  type: string;
  accept: string;
  files: ArrayLike<FileLike> | null;
  click(): void;
  addEventListener(type: string, fn: () => void): void;
  removeEventListener(type: string, fn: () => void): void;
}

/** The picker over `doc`: an `<input type="file">` clicked synchronously, inside the
 * user's click, resolving to the chosen file or `null` when cancelled. */
export function browserPickLogoFile(doc?: Document): PickLogoFile {
  return () => {
    const input = (doc ?? document).createElement("input") as unknown as FileInputLike;
    input.type = "file";
    input.accept = LOGO_ACCEPT;
    const chosen = new Promise<PickedLogoFile | null>((resolve, reject) => {
      const done = () => {
        input.removeEventListener("change", onChange);
        input.removeEventListener("cancel", onCancel);
      };
      const onChange = () => {
        done();
        const file = input.files?.[0];
        if (!file) {
          resolve(null);
          return;
        }
        // Refuse an oversized file before reading it into memory.
        if ((file.size ?? 0) > MAX_LOGO_FILE_BYTES) {
          reject(tooLarge(file.name));
          return;
        }
        file
          .arrayBuffer()
          .then((buffer) => resolve({ name: file.name, bytes: new Uint8Array(buffer) }), reject);
      };
      const onCancel = () => {
        done();
        resolve(null);
      };
      input.addEventListener("change", onChange);
      input.addEventListener("cancel", onCancel);
    });
    input.click();
    return chosen;
  };
}

const TOKEN_PREFIX = "web-file:";
/** Largest picture file the page reads: a startup logo is at most 320x240, so this is
 * far above any real one and keeps a hostile file from filling the page's memory. */
export const MAX_LOGO_FILE_BYTES = 16 * 1024 * 1024;

function tooLarge(name: string): UiError {
  return {
    kind: "usage",
    message: `${name} is larger than 16 MB, which no startup logo needs.`,
    hint: "Pick a smaller picture.",
  };
}

function unknownFile(): UiError {
  return {
    kind: "usage",
    message: "That logo file is no longer open in this page. Choose it again.",
    hint: null,
  };
}

/** The files chosen in this page, each under its own token. */
export class LogoFiles {
  private files = new Map<string, LogoFile>();
  private next = 1;

  constructor(private readonly pick: PickLogoFile) {}

  /** Opens the picker; resolves to the chosen file's token, or `null` when cancelled. */
  async choose(): Promise<string | null> {
    const file = await this.pick();
    if (!file) return null;
    if (file.bytes.length > MAX_LOGO_FILE_BYTES) throw tooLarge(file.name);
    const token = `${TOKEN_PREFIX}${this.next++}:${file.name}`;
    this.files.set(token, {
      name: file.name,
      bytes: file.bytes,
      isBin: /\.bin$/i.test(file.name),
    });
    return token;
  }

  /** The file under `token`; an unknown token is a usage error. */
  get(token: string): LogoFile {
    const file = this.files.get(token);
    if (!file) throw unknownFile();
    return file;
  }
}
