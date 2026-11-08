/**
 * FR R13Y5W, AC-R13Y5W.1: in the web edition the saved-recording toast shows no
 * "Reveal in Finder" / "Show in folder" action and no copy-path action; the desktop
 * toast is unchanged.
 *
 * The toast's file actions are the `fileActions` snippet of App.svelte (rendered by
 * every saved-file notice: recordings, PD captures). They are gated on a `fileReveal`
 * capability: on in the desktop (Tauri and mock) builds, off in the web build, where a
 * page can neither open the file manager nor name a path on disk.
 *
 * Evidence: web HIL run WEB.hil2-20261108-2223 (W5): the browser toast offered
 * "Reveal in Finder".
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { DESKTOP_FEATURES, WEB_FEATURES } from "./features";

const APP = readFileSync(new URL("../../App.svelte", import.meta.url), "utf8");

/** The body of App.svelte's `{#snippet fileActions(...)}` block. */
function fileActionsSnippet(): string {
  const start = APP.indexOf("{#snippet fileActions(");
  expect(start, "App.svelte defines the fileActions snippet").toBeGreaterThanOrEqual(0);
  const end = APP.indexOf("{/snippet}", start);
  expect(end).toBeGreaterThan(start);
  return APP.slice(start, end);
}

describe("fileReveal capability (AC-R13Y5W.1)", () => {
  it("is off in the web build", () => {
    expect((WEB_FEATURES as Record<string, unknown>)["fileReveal"]).toBe(false);
  });

  it("is on in the desktop build", () => {
    expect((DESKTOP_FEATURES as Record<string, unknown>)["fileReveal"]).toBe(true);
  });
});

describe("the saved-file toast actions (AC-R13Y5W.1)", () => {
  it("gates the reveal and copy-path buttons on features.fileReveal", () => {
    const body = fileActionsSnippet();
    const gate = body.search(/\{#if\s+features\.fileReveal\s*\}/);
    expect(gate, "the snippet opens an {#if features.fileReveal} block").toBeGreaterThanOrEqual(0);
    const close = body.indexOf("{/if}", gate);
    expect(close).toBeGreaterThan(gate);
    const gated = body.slice(gate, close);
    // Both actions sit inside the gate, none outside it.
    expect(gated).toContain("revealFile(path)");
    expect(gated).toContain("copyPath(path)");
    const outside = body.slice(0, gate) + body.slice(close);
    expect(outside).not.toContain("revealFile(");
    expect(outside).not.toContain("copyPath(");
    expect(outside).not.toContain("<button");
  });

  it("keeps the desktop toast's buttons and labels", () => {
    const body = fileActionsSnippet();
    expect(body).toContain("{revealLabel}");
    expect(body).toContain("Copy path");
    expect(body).toMatch(/class="file-actions"/);
  });
});
