// Release version checks (FR E1ZJ2N).
//
// A release starts from a tag of the form `vX.Y.Z`, optionally with a
// pre-release suffix such as `-rc.1`. Before any build, the release check job
// compares the tag with every release file (`bun versions.ts <tag|--from-cargo>
// [--root <repo>] [--notes <file>]`).

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { extractRelease, releaseTitle } from "./changelog";

/** The version a release tag names, and whether it is a pre-release. */
export interface ReleaseTag {
  version: string;
  prerelease: boolean;
}

const TAG_RE = /^v(\d+\.\d+\.\d+)(?:-([0-9A-Za-z.-]+))?$/;

/** Parses a release tag (`v1.2.3` or `v1.2.3-rc.1`); throws an Error naming the tag otherwise. */
export function parseTag(tag: string): ReleaseTag {
  const m = TAG_RE.exec(tag);
  if (!m) {
    throw new Error(
      `"${tag}" is not a release tag: expected vX.Y.Z, optionally with a pre-release suffix such as -rc.1`,
    );
  }
  const pre = m[2];
  return {
    version: pre === undefined ? (m[1] ?? "") : `${m[1]}-${pre}`,
    prerelease: pre !== undefined,
  };
}

/** The release files' contents, as read from the repository. */
export interface ReleaseFiles {
  tag: string;
  cargoToml: string;
  packageJson: string;
  tauriConf: string;
  changelog: string;
}

/** The `version` of `[workspace.package]` in a Cargo.toml, or undefined. */
export function cargoWorkspaceVersion(cargoToml: string): string | undefined {
  let inSection = false;
  for (const raw of cargoToml.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("[")) {
      inSection = line === "[workspace.package]";
      continue;
    }
    if (!inSection) continue;
    const m = /^version\s*=\s*"([^"]*)"/.exec(line);
    if (m) return m[1];
  }
  return undefined;
}

function jsonVersion(text: string): string | undefined {
  try {
    const v = (JSON.parse(text) as { version?: unknown }).version;
    return typeof v === "string" ? v : undefined;
  } catch {
    return undefined;
  }
}

/** One clear message per release file that does not match the tag; empty when all match. */
export function checkRelease(files: ReleaseFiles): string[] {
  let version: string;
  try {
    version = parseTag(files.tag).version;
  } catch (e) {
    return [(e as Error).message];
  }
  const msgs: string[] = [];
  const compare = (file: string, found: string | undefined) => {
    if (found !== version) {
      msgs.push(
        `${file}: version is ${found === undefined ? "missing" : found}, but the tag ${files.tag} needs ${version}`,
      );
    }
  };
  compare("Cargo.toml [workspace.package]", cargoWorkspaceVersion(files.cargoToml));
  compare("apps/desktop/package.json", jsonVersion(files.packageJson));
  compare("apps/desktop/src-tauri/tauri.conf.json", jsonVersion(files.tauriConf));
  const section = extractRelease(files.changelog, version);
  if (section === null) {
    msgs.push(`CHANGELOG.md: no "## [${version}]" section for the tag ${files.tag}`);
  } else if (section.notes === "") {
    msgs.push(`CHANGELOG.md: the "## [${version}]" section is empty; it becomes the release notes`);
  }
  return msgs;
}

// A release asset carries no file mode, so a downloaded AppImage is never executable.
// Tauri names it `<productName>_<version>_amd64.AppImage`.
export function appImageNote(version: string): string {
  const file = `open-powerz_${version}_amd64.AppImage`;
  return [
    "### Linux AppImage",
    "",
    "The AppImage is not executable after download. Make it so, then run it:",
    "",
    "```sh",
    `chmod +x ${file}`,
    `./${file}`,
    "```",
    "",
  ].join("\n");
}

function main(argv: string[]): number {
  let tagArg: string | undefined;
  let root = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
  let notesPath: string | undefined;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--root" || a === "--notes") {
      const v = argv[++i];
      if (v === undefined) {
        process.stderr.write(`${a} needs a value\n`);
        return 2;
      }
      if (a === "--root") root = resolve(v);
      else notesPath = v;
    } else if (tagArg === undefined && a !== undefined) {
      tagArg = a;
    } else {
      process.stderr.write(`unexpected argument: ${a}\n`);
      return 2;
    }
  }
  if (tagArg === undefined) {
    process.stderr.write(
      "usage: bun versions.ts <tag|--from-cargo> [--root <repo>] [--notes <file>]\n",
    );
    return 2;
  }
  const unreadable: string[] = [];
  const read = (p: string): string => {
    try {
      return readFileSync(join(root, p), "utf8");
    } catch (e) {
      unreadable.push(
        `${p}: cannot read ${join(root, p)} (${(e as NodeJS.ErrnoException).code ?? e})`,
      );
      return "";
    }
  };
  const cargoToml = read("Cargo.toml");
  const tag = tagArg === "--from-cargo" ? `v${cargoWorkspaceVersion(cargoToml) ?? ""}` : tagArg;
  const files: ReleaseFiles = {
    tag,
    cargoToml,
    packageJson: read("apps/desktop/package.json"),
    tauriConf: read("apps/desktop/src-tauri/tauri.conf.json"),
    changelog: read("CHANGELOG.md"),
  };
  // A file that cannot be read is the problem to report, not the version it lacks.
  const msgs = unreadable.length > 0 ? unreadable : checkRelease(files);
  if (msgs.length > 0) {
    for (const m of msgs) process.stderr.write(`${m}\n`);
    return 1;
  }
  const { version, prerelease } = parseTag(tag);
  const section = extractRelease(files.changelog, version);
  if (notesPath !== undefined) {
    writeFileSync(notesPath, `${section?.notes ?? ""}\n\n${appImageNote(version)}`);
  }
  process.stdout.write(
    [
      `tag=${tag}`,
      `version=${version}`,
      `prerelease=${prerelease}`,
      `title=${releaseTitle(tag, section?.codename)}`,
    ].join("\n") + "\n",
  );
  return 0;
}

// Only when run as a script: the tests import this module.
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
