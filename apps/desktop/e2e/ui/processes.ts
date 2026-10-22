/**
 * AC-KQF5GM.4: the process list the WebKit memory test reads. `ps` cuts each line to the
 * terminal width unless asked not to, and a WebKit page process names itself far past
 * column 80 on Linux, so the list is read at unlimited width (`-ww`).
 */

import { execFileSync } from "node:child_process";

/** `ps` arguments: every process, pid / RSS (KiB) / full command line, no width limit. */
export const PS_ARGS: readonly string[] = ["-ww", "-eo", "pid=,rss=,command="];

/** Command-line marker of a WebKit page process on `platform`. */
export function pageProcessMarker(platform: NodeJS.Platform): string {
  return platform === "darwin" ? "com.apple.WebKit.WebContent" : "WebKitWebProcess";
}

/** RSS (KiB) by pid of every line of `ps` output that names `marker`. */
export function parseProcesses(out: string, marker: string): Map<number, number> {
  const found = new Map<number, number>();
  for (const line of out.split("\n")) {
    if (!line.includes(marker)) continue;
    const [pid, rss] = line.trim().split(/\s+/, 2).map(Number);
    if (Number.isFinite(pid) && Number.isFinite(rss)) found.set(pid!, rss!);
  }
  return found;
}

/** RSS (KiB) by pid of every running process whose command line names `marker`. */
export function listProcesses(marker: string): Map<number, number> {
  const out = execFileSync("ps", [...PS_ARGS], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return parseProcesses(out, marker);
}

/** Skip reason when no page process of this page was found; none when one was. */
export function noPageProcessReason(pids: ReadonlySet<number>, marker: string): string | undefined {
  if (pids.size > 0) return undefined;
  return `no ${marker} page process appeared for this page, so there is no RSS to measure`;
}
