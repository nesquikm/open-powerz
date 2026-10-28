// README feature coverage and link checks (FR 7V4B90).
//
//   bun scripts/readme/readme-features.ts [repo-root]
//
// Maps each CHANGELOG "Added" entry (by FR id) to a README Features line via
// readme-features.json, and checks that every relative README link resolves.
// A mapping value "<area-slug>: <phrase>" is covered only by a bullet in that
// area containing <phrase> (case-insensitive); a bare "<area-slug>" needs any
// bullet in the area. null marks an entry that is not a user-facing feature:
// AH2AEN (the desktop test suite), V9SK56 (staged firmware checks on a real
// meter) and 1HHT04 (read-only real-meter checks) are development
// verification, not something a user does.
import { existsSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

/** CHANGELOG FR id -> "<area-slug>[: <phrase>]", or null for a non-feature entry. */
export type FeatureMap = Record<string, string | null>;

export interface AddedEntry {
  version: string;
  id: string;
  text: string;
}

export interface CoverageFinding {
  id: string;
  reason: string;
}

export interface LinkFinding {
  link: string;
  reason: string;
}

/** GitHub-style heading anchor: lowercase, punctuation dropped, spaces to hyphens. */
export function slugify(heading: string): string {
  return heading
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");
}

/** Every bullet under a "### Added" group, with its version and trailing (FR id) if any. */
function addedBullets(changelog: string): { version: string; id?: string; text: string }[] {
  const bullets: { version: string; id?: string; text: string }[] = [];
  let version: string | undefined;
  let inAdded = false;
  for (const line of changelog.split(/\r?\n/)) {
    const release = /^## \[([^\]]+)\]/.exec(line);
    if (release) {
      version = release[1];
      inAdded = false;
      continue;
    }
    if (/^### /.test(line)) {
      inAdded = line.slice(4).trim() === "Added";
      continue;
    }
    if (!inAdded || version === undefined || !/^[-*] /.test(line)) continue;
    const id = /\(([A-Z0-9]+)\)\s*$/.exec(line)?.[1];
    bullets.push({ version, id, text: line.slice(2).trim() });
  }
  return bullets;
}

/** Every bullet under a "### Added" group, with its version and trailing (FR id). */
export function parseAddedEntries(changelog: string): AddedEntry[] {
  const entries: AddedEntry[] = [];
  for (const { version, id, text } of addedBullets(changelog)) {
    if (id !== undefined) entries.push({ version, id, text });
  }
  return entries;
}

/** README "## Features" sub-heading slug -> its bullet lines, in order. */
export function parseFeatureAreas(readme: string): Map<string, string[]> {
  const areas = new Map<string, string[]>();
  let inFeatures = false;
  let current: string[] | undefined;
  for (const line of readme.split(/\r?\n/)) {
    if (/^## /.test(line)) {
      inFeatures = line.slice(3).trim() === "Features";
      current = undefined;
      continue;
    }
    if (!inFeatures) continue;
    if (/^### /.test(line)) {
      current = [];
      areas.set(slugify(line.slice(4)), current);
    } else if (current && /^[-*] /.test(line)) {
      current.push(line);
    }
  }
  return areas;
}

/** One finding per CHANGELOG "Added" entry with no README Features line behind it. */
export function checkFeatureCoverage(
  changelog: string,
  readme: string,
  map: FeatureMap,
): CoverageFinding[] {
  const areas = parseFeatureAreas(readme);
  const findings: CoverageFinding[] = [];
  for (const { version, id, text } of addedBullets(changelog)) {
    if (id === undefined) {
      findings.push({
        id: "untagged",
        reason: `${version} Added entry has no (FR id): ${text}`,
      });
      continue;
    }
    if (!Object.hasOwn(map, id)) {
      findings.push({
        id,
        reason: `${version} Added entry ${id} is missing from readme-features.json`,
      });
      continue;
    }
    const value = map[id];
    if (value === null || value === undefined) continue;
    const colon = value.indexOf(": ");
    const area = colon < 0 ? value : value.slice(0, colon);
    const phrase = colon < 0 ? undefined : value.slice(colon + 2).trim();
    const lines = areas.get(area);
    if (lines === undefined) {
      findings.push({ id, reason: `README Features has no "${area}" area for ${id}` });
    } else if (lines.length === 0) {
      findings.push({ id, reason: `README Features area "${area}" has no feature line for ${id}` });
    } else if (
      phrase !== undefined &&
      !lines.some((line) => line.toLowerCase().includes(phrase.toLowerCase()))
    ) {
      findings.push({
        id,
        reason: `README Features area "${area}" has no line containing "${phrase}" for ${id}`,
      });
    }
  }
  return findings;
}

/**
 * Markdown lines outside fenced code blocks. Inline code spans are blanked, or
 * only lose their backticks with `keepCode` (heading text keeps them for anchors).
 */
function proseLines(markdown: string, keepCode = false): string[] {
  const lines: string[] = [];
  let fence: string | undefined;
  for (const line of markdown.split(/\r?\n/)) {
    if (fence === undefined) {
      fence = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
      if (fence === undefined) lines.push(line.replace(/`([^`]*)`/g, keepCode ? "$1" : ""));
      continue;
    }
    // A fence closes on a bare marker of the same character, at least as long as it opened.
    const close = /^\s*(`{3,}|~{3,})\s*$/.exec(line)?.[1];
    if (close !== undefined && close[0] === fence[0] && close.length >= fence.length) {
      fence = undefined;
    }
  }
  return lines;
}

/** Relative link and image targets (in-page anchors included), not URLs or code. */
export function readmeLinks(readme: string): string[] {
  const links: string[] = [];
  for (const line of proseLines(readme)) {
    const targets = [
      ...[...line.matchAll(/\]\(\s*<?([^)\s>]+)>?(?:\s+["'(][^)]*)?\)/g)].map((m) => m[1]),
      /^\s{0,3}\[[^\]]+\]:\s*<?([^\s>]+)/.exec(line)?.[1],
    ];
    for (const target of targets) {
      if (target === undefined || /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(target)) continue;
      links.push(target);
    }
  }
  return links;
}

/** GitHub anchors of every heading, with -1, -2 suffixes for repeats. */
function headingAnchors(markdown: string): Set<string> {
  const anchors = new Set<string>();
  const seen = new Map<string, number>();
  for (const line of proseLines(markdown, true)) {
    const text = /^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/.exec(line)?.[1];
    if (text === undefined) continue;
    const slug = slugify(text.replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1"));
    const n = seen.get(slug) ?? 0;
    seen.set(slug, n + 1);
    anchors.add(n === 0 ? slug : `${slug}-${n}`);
  }
  return anchors;
}

/** A percent-decoded link target, or undefined when its escapes are malformed. */
function decodeTarget(target: string): string | undefined {
  try {
    return decodeURIComponent(target);
  } catch {
    return undefined;
  }
}

/** One finding per relative README link that points at no file or heading. */
export function checkReadmeLinks(readme: string, root: string): LinkFinding[] {
  const anchors = headingAnchors(readme);
  const findings: LinkFinding[] = [];
  for (const link of readmeLinks(readme)) {
    const hash = link.indexOf("#");
    const path = (hash < 0 ? link : link.slice(0, hash)).split("?")[0] ?? "";
    const decoded = decodeTarget(path === "" ? link.slice(hash + 1) : path);
    if (decoded === undefined) {
      findings.push({ link, reason: `malformed link target ${link}` });
      continue;
    }
    if (path === "") {
      const anchor = decoded.toLowerCase();
      if (!anchors.has(anchor)) {
        findings.push({ link, reason: `README has no heading for anchor #${anchor}` });
      }
      continue;
    }
    const target = resolve(root, decoded.replace(/^\/+/, ""));
    const inside = relative(resolve(root), target);
    if (inside === ".." || inside.startsWith(`..${sep}`) || isAbsolute(inside)) {
      findings.push({ link, reason: `${decoded} points outside the repository` });
    } else if (!existsSync(target)) {
      findings.push({ link, reason: `${decoded} does not exist in the repository` });
    }
  }
  return findings;
}

function main(argv: string[]): number {
  const here = dirname(fileURLToPath(import.meta.url));
  const root = resolve(argv[0] ?? join(here, "../../../.."));
  const readme = readFileSync(join(root, "README.md"), "utf8");
  const changelog = readFileSync(join(root, "CHANGELOG.md"), "utf8");
  const mapPath = join(here, "readme-features.json");
  const map = JSON.parse(readFileSync(mapPath, "utf8")) as FeatureMap;
  const problems = [
    ...checkFeatureCoverage(changelog, readme, map).map((f) => `feature ${f.id}: ${f.reason}`),
    ...checkReadmeLinks(readme, root).map((f) => `link ${f.link}: ${f.reason}`),
  ];
  for (const p of problems) console.error(p);
  if (problems.length > 0) return 1;
  console.log("README features and links: ok");
  return 0;
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
