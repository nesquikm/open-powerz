// The pinned Rust toolchain (FR E1ZJ2M).
//
// rust-toolchain.toml at the repository root names one exact stable Rust
// version; this module reads that pin and checks its shape.

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The `[toolchain]` table of a rust-toolchain.toml. */
export interface ToolchainPin {
  channel: string;
  profile?: string;
  components: string[];
}

/** One workflow line that names its own Rust version or channel. */
export interface Finding {
  file: string;
  line: number;
  reason: string;
}

/** True for an exact stable release such as `1.99.1`; false for channels, pre-releases and dates. */
export function isExactStableVersion(v: string): boolean {
  return /^\d+\.\d+\.\d+$/.test(v);
}

/**
 * Parses the `[toolchain]` table (channel, profile, components) of a rust-toolchain.toml.
 * Accepts basic and literal strings and a components array split over several lines.
 */
export function readToolchainPin(text: string): ToolchainPin {
  const pin: ToolchainPin = { channel: "", components: [] };
  let inTable = false;
  let pending: { key: string; value: string } | null = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/#.*$/, "").trim();
    if (pending) {
      pending.value += ` ${line}`;
      if (!line.includes("]")) continue;
      assign(pin, pending.key, pending.value);
      pending = null;
      continue;
    }
    if (line === "") continue;
    const header = /^\[([^\]]+)\]$/.exec(line);
    if (header) {
      inTable = header[1]?.trim() === "toolchain";
      continue;
    }
    if (!inTable) continue;
    const kv = /^([A-Za-z_-]+)\s*=\s*(.+)$/.exec(line);
    if (!kv) continue;
    const [, key = "", value = ""] = kv;
    if (value.startsWith("[") && !value.includes("]")) pending = { key, value };
    else assign(pin, key, value);
  }
  return pin;
}

function assign(pin: ToolchainPin, key: string, value: string): void {
  const strings = [...value.matchAll(/"([^"]*)"|'([^']*)'/g)].map((m) => m[1] ?? m[2] ?? "");
  if (key === "channel") pin.channel = strings[0] ?? "";
  else if (key === "profile") pin.profile = strings[0];
  else if (key === "components") pin.components = strings;
}

// Workflow scan (AC-E1ZJ2M.2): every workflow installs Rust from
// rust-toolchain.toml, so no line may name its own version or channel.

const RUSTUP_SELECT =
  /\brustup\s+(?:toolchain\s+install|install|update|default|override\s+set|run)\b(.*)$/;
// rustup flags that take a value, so the value is not mistaken for a toolchain.
const VALUE_FLAGS = new Set(["--profile", "-c", "--component", "-t", "--target"]);

/**
 * Drops a full-line comment or a trailing ` #` comment, leaving a `#` inside quotes alone.
 * A quote that never closes on the line (`Rust's`, `don\'t`) is read as a plain character.
 */
function stripComment(line: string): string {
  const literal = new Set<number>();
  for (;;) {
    let quote = "";
    let quoteAt = -1;
    for (let i = 0; i < line.length; i++) {
      const c = line[i];
      if (quote) {
        if (c === quote) quote = "";
      } else if ((c === '"' || c === "'") && !literal.has(i)) {
        quote = c;
        quoteAt = i;
      } else if (c === "#" && (i === 0 || /\s/.test(line[i - 1] ?? ""))) {
        return line.slice(0, i);
      }
    }
    if (!quote) return line;
    literal.add(quoteAt);
  }
}

/** True when the rest of a rustup command names an explicit toolchain argument. */
function namesToolchain(rest: string): boolean {
  const tokens = rest
    .replace(/["'\\]/g, " ")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i] ?? "";
    // A shell operator or redirection ends the rustup command.
    if (/^(?:&&|\|\||;|\||\d*[<>])/.test(t)) return false;
    if (VALUE_FLAGS.has(t)) {
      i++;
      continue;
    }
    if (t.startsWith("-")) continue;
    return true;
  }
  return false;
}

function lineReason(line: string): string | null {
  if (/dtolnay\/rust-toolchain/.test(line)) return "uses dtolnay/rust-toolchain";
  if (/(?:^|[{,\s])["']?toolchain["']?\s*:/.test(line)) return "sets a toolchain: input";
  if (/\bRUSTUP_TOOLCHAIN\b/.test(line)) return "sets RUSTUP_TOOLCHAIN";
  if (/\bcargo\s+\+\S+/.test(line)) return "runs cargo +<toolchain>";
  const m = RUSTUP_SELECT.exec(line);
  if (m && namesToolchain(m[1] ?? "")) return "rustup names an explicit toolchain";
  return null;
}

/**
 * Scans one workflow's YAML text; returns one finding per offending line (1-based).
 * A shell line continued with a trailing `\` is read together with the lines after it.
 */
export function scanWorkflow(text: string, file: string): Finding[] {
  const findings: Finding[] = [];
  const lines = text.split(/\r?\n/).map(stripComment);
  for (let i = 0; i < lines.length; i++) {
    const start = i;
    let joined = lines[i] ?? "";
    while (/\\\s*$/.test(joined) && i + 1 < lines.length) {
      joined = `${joined.replace(/\\\s*$/, "")} ${lines[++i] ?? ""}`;
    }
    const reason = lineReason(joined);
    if (reason) findings.push({ file, line: start + 1, reason });
  }
  return findings;
}

/** Scans every .yml / .yaml file in a workflows directory. */
export function scanWorkflowDir(dir: string): Finding[] {
  return readdirSync(dir)
    .filter((name) => /\.ya?ml$/.test(name))
    .sort()
    .flatMap((name) => {
      const path = join(dir, name);
      return scanWorkflow(readFileSync(path, "utf8"), path);
    });
}

// CLI: `bun toolchain-pin.ts [workflows-dir]`; exits 1 when any workflow pins Rust.
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const here = dirname(fileURLToPath(import.meta.url));
  const dir = process.argv[2] ?? resolve(here, "../../../../.github/workflows");
  const findings = scanWorkflowDir(dir);
  for (const f of findings) console.error(`${f.file}:${f.line} — ${f.reason}`);
  if (findings.length > 0) {
    console.error("Workflows must install Rust from rust-toolchain.toml.");
    process.exit(1);
  }
  console.log(`toolchain-pin: ${dir} is clean`);
}
