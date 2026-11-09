/**
 * FR W8SHXA: where a record delete writes its backups.
 *
 * - AC-W8SHXA.2: with a folder picker (Chrome, Edge), every backup is a confirmed file
 *   write into `open-powerz-backups/records-<ms>` of the picked folder, finished before
 *   the meter's record table is written; a cancelled picker deletes nothing and sends
 *   no table write. Without one, each backup is a browser download.
 * - AC-W8SHXA.5: the answer names that folder.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import {
  DOWNLOADS_LABEL,
  openBackupSink,
  type DirectoryLike,
  type FileLike,
  type PickDirectory,
} from "./backupFolder";
import type { PowerzWasm } from "./driver";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import {
  FakeUsbNavigator,
  demoUsbDevice,
  downloadSink,
  sentNames,
  settle,
} from "./webBackendFakes.test";

/** An in-memory folder tree; `log` records every finished file write, in order. */
class FakeFolder implements DirectoryLike {
  folders = new Map<string, FakeFolder>();
  files = new Map<string, string>();
  constructor(
    readonly name: string,
    private readonly log: string[],
    private readonly path = name,
  ) {}
  async getDirectoryHandle(name: string): Promise<DirectoryLike> {
    let f = this.folders.get(name);
    if (!f) {
      f = new FakeFolder(name, this.log, `${this.path}/${name}`);
      this.folders.set(name, f);
    }
    return f;
  }
  async getFileHandle(name: string): Promise<FileLike> {
    return {
      createWritable: async () => {
        let text = "";
        return {
          write: async (data: string) => {
            text += data;
          },
          close: async () => {
            this.files.set(name, text);
            this.log.push(`${this.path}/${name}`);
          },
        };
      },
    };
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

describe("the backup sink", () => {
  it("without a folder picker, backups are downloads", async () => {
    const { downloads, download } = downloadSink();
    const sink = await openBackupSink(null, download);
    expect(sink.label).toBe(DOWNLOADS_LABEL);
    await sink.save("01_A.csv", "x");
    expect(downloads.map((d) => d.name)).toEqual(["01_A.csv"]);
  });

  it("a picked folder gets open-powerz-backups/records-<ms> and confirmed file writes", async () => {
    const log: string[] = [];
    const root = new FakeFolder("Backups", log);
    const pick: PickDirectory = async () => root;
    const sink = await openBackupSink(
      pick,
      () => {},
      () => 42,
    );
    expect(sink.label).toBe("Backups/open-powerz-backups/records-42");
    await sink.save("01_A.csv", "a,b\n");
    expect(log).toEqual(["Backups/open-powerz-backups/records-42/01_A.csv"]);
  });

  it("a cancelled folder picker is a usage error that deletes nothing", async () => {
    const pick: PickDirectory = () => Promise.reject(new DOMException("abort", "AbortError"));
    await expect(openBackupSink(pick, () => {})).rejects.toMatchObject({
      kind: "usage",
      message: "No backup folder was chosen, so nothing was deleted.",
    } satisfies Partial<UiError>);
  });
});

describe("a delete with a picked folder (AC-W8SHXA.2, .5)", () => {
  async function backend(pick: PickDirectory) {
    const nav = new FakeUsbNavigator();
    const dev = demoUsbDevice(w, "123456");
    nav.devices = [dev];
    const { download } = downloadSink();
    const b = await settle(
      createWebBackend({ wasm: w, usb: nav, hid: null, download, pickDirectory: pick } as never),
    );
    const id = (await settle(b.listDevices())).find((d) => d.serial === "123456")?.id;
    if (!id) throw new Error("the permitted meter is not listed");
    await settle(b.connect(id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(300);
    return { b, dev };
  }

  it("writes every backup file before the one table write and names the folder", async () => {
    const log: string[] = [];
    const root = new FakeFolder("Backups", log);
    const { b, dev } = await backend(async () => root);
    let writtenAtTableWrite = -1;
    const transfer = dev.transferOut.bind(dev);
    dev.transferOut = async (ep: number, data: BufferSource) => {
      const bytes = new Uint8Array(data as ArrayBuffer);
      if (bytes[0] === 0x48) writtenAtTableWrite = log.length;
      return transfer(ep, data);
    };
    const result = await settle(b.recordsDelete("all", "all"), 20_000);
    expect(log).toHaveLength(3);
    expect(writtenAtTableWrite).toBe(3);
    expect(result.backup_dir).toMatch(/^Backups\/open-powerz-backups\/records-\d+$/);
    expect(sentNames(w, dev).filter((n) => n === "record_table_write")).toHaveLength(1);
    await settle(b.disconnect());
  });

  it("a cancelled folder picker sends no table write", async () => {
    const { b, dev } = await backend(() => Promise.reject(new DOMException("x", "AbortError")));
    await expect(settle(b.recordsDelete(1, "PHONE_5V"))).rejects.toMatchObject({
      kind: "usage",
    });
    expect(sentNames(w, dev)).not.toContain("record_table_write");
    await settle(b.disconnect());
  });
});
