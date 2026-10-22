/**
 * AC-KQF5GM.4: the WebKit memory test finds this page's content process on Linux and on
 * macOS because the process list it reads is not cut to a terminal width, and it skips
 * with a reason when no page process can be found.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { afterEach, describe, expect, it } from "vitest";
import {
  PS_ARGS,
  listProcesses,
  noPageProcessReason,
  pageProcessMarker,
  parseProcesses,
} from "./processes";

describe("pageProcessMarker", () => {
  it("names the WebKit page process of each OS", () => {
    expect(pageProcessMarker("darwin")).toBe("com.apple.WebKit.WebContent");
    expect(pageProcessMarker("linux")).toBe("WebKitWebProcess");
  });
});

describe("parseProcesses", () => {
  it("keeps the pid and RSS of every line that names the marker", () => {
    const out = [
      "    1   1024 /sbin/init",
      ` 4242 204800 /usr/lib/webkit/${"x".repeat(200)} WebKitWebProcess 12 13`,
      "4243  8192 /usr/lib/webkit/WebKitNetworkProcess",
      "  77 51200 /usr/lib/webkit/WebKitWebProcess 7 8",
      "",
    ].join("\n");
    expect(parseProcesses(out, "WebKitWebProcess")).toEqual(
      new Map([
        [4242, 204800],
        [77, 51200],
      ]),
    );
  });

  it("finds nothing in an empty list", () => {
    expect(parseProcesses("", "WebKitWebProcess").size).toBe(0);
  });
});

describe("PS_ARGS", () => {
  it("asks ps for unlimited width", () => {
    expect(PS_ARGS).toContain("-ww");
    expect(PS_ARGS.join(" ")).toMatch(/pid=.*rss=.*command=/);
  });
});

describe("listProcesses", () => {
  let child: ChildProcess | undefined;
  afterEach(() => {
    child?.kill();
    child = undefined;
  });

  it.skipIf(process.platform === "win32")(
    "finds a process whose marker sits far past a terminal width",
    async () => {
      const marker = `KQF5GM-wide-marker-${process.pid}-${Date.now()}`;
      // The marker starts after column 300 of the command line.
      child = spawn(
        process.execPath,
        ["-e", "setTimeout(() => {}, 30000)", "x".repeat(300), marker],
        {
          stdio: "ignore",
        },
      );
      await new Promise((resolve) => child!.once("spawn", resolve));
      const found = listProcesses(marker);
      expect([...found.keys()]).toContain(child.pid);
      expect(found.get(child.pid!)!).toBeGreaterThan(0);
    },
  );
});

describe("noPageProcessReason", () => {
  it("gives a skip reason when no page process appeared", () => {
    const reason = noPageProcessReason(new Set(), "WebKitWebProcess");
    expect(reason).toMatch(/no WebKitWebProcess/);
    expect(reason).toMatch(/page process/);
  });

  it("gives none when this page's process was found", () => {
    expect(noPageProcessReason(new Set([4242]), "WebKitWebProcess")).toBeUndefined();
  });
});
