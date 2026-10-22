import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// A JSON merge patch replaces arrays whole, so the e2e config repeats the app window.
const read = (path: string) => JSON.parse(readFileSync(new URL(path, import.meta.url), "utf8"));

describe("e2e Tauri config (AC-AH2AEN.4)", () => {
  it("repeats the app window exactly, adding only the WebView2 arguments WebDriver needs", () => {
    const base = read("../../src-tauri/tauri.conf.json").app.windows;
    const e2e = read("../../e2e/app/tauri.e2e.conf.json").app.windows;
    expect(e2e).toHaveLength(base.length);
    const { additionalBrowserArgs, ...rest } = e2e[0];
    expect(rest).toEqual(base[0]);
    expect(additionalBrowserArgs).toContain("--remote-debugging-port=0");
  });
});
