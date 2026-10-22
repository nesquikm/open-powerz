/**
 * AC-KQF5GM.4: the process list the WebKit memory test reads. `ps` cuts each line to the
 * terminal width unless asked not to, and a WebKit page process names itself far past
 * column 80 on Linux, so the list is read at unlimited width (`-ww`). On Linux the
 * Playwright WebKit runs as WPE when headless (`WPEWebProcess`, as in CI) and as GTK when
 * headed (`WebKitWebProcess`), so both names count as a page process there.
 */

import { execFileSync } from "node:child_process";

/** `ps` arguments: every process, pid / RSS (KiB) / full command line, no width limit. */
export const PS_ARGS: readonly string[] = ["-ww", "-eo", "pid=,rss=,command="];

/** Command-line marker of a WebKit page process on `platform` (the GTK one on Linux). */
export function pageProcessMarker(platform: NodeJS.Platform): string {
  return platform === "darwin" ? "com.apple.WebKit.WebContent" : "WebKitWebProcess";
}

/** Every command-line marker of a WebKit page process on `platform` (WPE and GTK on Linux). */
export function pageProcessMarkers(platform: NodeJS.Platform): readonly string[] {
  return platform === "darwin"
    ? [pageProcessMarker(platform)]
    : ["WPEWebProcess", pageProcessMarker(platform)];
}

/** One marker or several, as one list. */
function asList(marker: string | readonly string[]): readonly string[] {
  return typeof marker === "string" ? [marker] : marker;
}

/** RSS (KiB) by pid of every line of `ps` output that names `marker` (or any of them). */
export function parseProcesses(
  out: string,
  marker: string | readonly string[],
): Map<number, number> {
  const markers = asList(marker);
  const found = new Map<number, number>();
  for (const line of out.split("\n")) {
    if (!markers.some((m) => line.includes(m))) continue;
    const [pid, rss] = line.trim().split(/\s+/, 2).map(Number);
    if (Number.isFinite(pid) && Number.isFinite(rss)) found.set(pid!, rss!);
  }
  return found;
}

/** RSS (KiB) by pid of every running process whose command line names `marker` (or any). */
export function listProcesses(marker: string | readonly string[]): Map<number, number> {
  const out = execFileSync("ps", [...PS_ARGS], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return parseProcesses(out, marker);
}

/** Skip reason when no page process of this page was found; none when one was. */
export function noPageProcessReason(
  pids: ReadonlySet<number>,
  marker: string | readonly string[],
): string | undefined {
  if (pids.size > 0) return undefined;
  return `no ${asList(marker).join(" or ")} page process appeared for this page, so there is no RSS to measure`;
}
