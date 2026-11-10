/**
 * FR W8SHXB, AC-W8SHXB.1: the web logo file picker. Choosing a picture opens the
 * browser's file picker for images (PNG, JPEG, BMP and raw `.bin` logos) from the
 * user's click, and keeps the chosen file's bytes under an opaque token
 * (`web-file:<n>:<name>`) that stands in for the desktop path; a cancelled picker
 * answers `null`, and an unknown token is a usage error.
 */

import { describe, expect, it } from "vitest";
import {
  LOGO_ACCEPT,
  LogoFiles,
  browserPickLogoFile,
  type PickLogoFile,
  MAX_LOGO_FILE_BYTES,
} from "./logoFiles";

type Listener = (event: unknown) => void;

/** A fake `<input type="file">`: the test picks a file or cancels after `click()`. */
class FakeInput {
  type = "";
  accept = "";
  files: { name: string; arrayBuffer(): Promise<ArrayBuffer> }[] | null = null;
  clicks = 0;
  private listeners: Record<string, Listener[]> = {};
  addEventListener(type: string, fn: Listener): void {
    (this.listeners[type] ??= []).push(fn);
  }
  removeEventListener(type: string, fn: Listener): void {
    this.listeners[type] = (this.listeners[type] ?? []).filter((l) => l !== fn);
  }
  click(): void {
    this.clicks++;
  }
  fire(type: string): void {
    for (const l of [...(this.listeners[type] ?? [])]) l({ type, target: this });
  }
  pick(name: string, bytes: Uint8Array): void {
    this.files = [{ name, arrayBuffer: async () => bytes.slice().buffer }];
    this.fire("input");
    this.fire("change");
  }
}

function fakeDocument() {
  const inputs: FakeInput[] = [];
  const doc = {
    createElement(tag: string) {
      expect(tag).toBe("input");
      const input = new FakeInput();
      inputs.push(input);
      return input;
    },
  };
  return { doc, inputs };
}

describe("the browser's file picker (AC-W8SHXB.1)", () => {
  it("accepts the desktop's logo formats", () => {
    expect(LOGO_ACCEPT).toBe("image/png,image/jpeg,image/bmp,.bin");
  });

  it("opens an image file input from the click and resolves to the chosen file", async () => {
    const { doc, inputs } = fakeDocument();
    const pick = browserPickLogoFile(doc as unknown as Document);
    const chosen = pick();
    // Synchronously, inside the user's click.
    expect(inputs).toHaveLength(1);
    const input = inputs[0] as FakeInput;
    expect(input.type).toBe("file");
    expect(input.accept).toBe(LOGO_ACCEPT);
    expect(input.clicks).toBe(1);
    input.pick("logo.png", new Uint8Array([1, 2, 3]));
    const file = await chosen;
    expect(file?.name).toBe("logo.png");
    expect(Array.from(file?.bytes ?? [])).toEqual([1, 2, 3]);
  });

  it("resolves null when the picker is cancelled", async () => {
    const { doc, inputs } = fakeDocument();
    const chosen = browserPickLogoFile(doc as unknown as Document)();
    (inputs[0] as FakeInput).fire("cancel");
    expect(await chosen).toBeNull();
  });
});

describe("file tokens (AC-W8SHXB.1)", () => {
  const picker = (files: ({ name: string; bytes: Uint8Array } | null)[]): PickLogoFile => {
    let i = 0;
    return async () => files[i++] ?? null;
  };

  it("keeps each chosen file under its own token", async () => {
    const files = new LogoFiles(
      picker([
        { name: "a.png", bytes: new Uint8Array([1]) },
        { name: "b.bin", bytes: new Uint8Array([2, 3]) },
      ]),
    );
    const a = await files.choose();
    const b = await files.choose();
    expect(a).toMatch(/^web-file:\d+:a\.png$/);
    expect(b).toMatch(/^web-file:\d+:b\.bin$/);
    expect(a).not.toBe(b);
    expect(files.get(a as string)).toMatchObject({ name: "a.png", isBin: false });
    expect(Array.from(files.get(a as string).bytes)).toEqual([1]);
    expect(files.get(b as string)).toMatchObject({ name: "b.bin", isBin: true });
    expect(Array.from(files.get(b as string).bytes)).toEqual([2, 3]);
  });

  it("a .bin file name in any case is a raw logo file", async () => {
    const files = new LogoFiles(picker([{ name: "LOGO.BIN", bytes: new Uint8Array([9]) }]));
    const t = await files.choose();
    expect(files.get(t as string).isBin).toBe(true);
  });

  it("a cancelled picker gives no token", async () => {
    const files = new LogoFiles(picker([null]));
    expect(await files.choose()).toBeNull();
  });

  it("an unknown token is a usage error", () => {
    const files = new LogoFiles(picker([]));
    for (const token of ["/tmp/logo.png", "web-file:99:x.png", ""]) {
      let error: unknown = null;
      try {
        files.get(token);
      } catch (e) {
        error = e;
      }
      expect(error, token).toMatchObject({ kind: "usage" });
    }
  });
});

describe("file size (FR W8SHXB review)", () => {
  it("refuses a picture larger than the cap before keeping it", async () => {
    const big = { name: "huge.png", bytes: new Uint8Array(MAX_LOGO_FILE_BYTES + 1) };
    const files = new LogoFiles(async () => big);
    await expect(files.choose()).rejects.toMatchObject({ kind: "usage" });
  });
});
