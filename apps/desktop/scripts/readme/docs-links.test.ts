// FR SPYPSD (AC-SPYPSD.2): every relative link and every anchor in the README and in
// every page under docs/ resolves: the target file exists, and the anchor matches a
// heading slugged the way GitHub slugs it (or an explicit {#id} / <a id|name>). This
// file runs under `bun run test`, which the gate runs on Linux, macOS and Windows.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, posix, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const read = (rel: string): string => readFileSync(join(repoRoot, rel), "utf8");

/** README.md plus every .md file under docs/, as repo-relative posix paths. */
function pages(): string[] {
  const entries = readdirSync(join(repoRoot, "docs"), { recursive: true, encoding: "utf8" });
  const docs = entries
    .filter((e) => e.endsWith(".md"))
    .map((e) => posix.join("docs", e.split("\\").join("/")))
    .sort();
  return ["README.md", ...docs];
}

/** The markdown with fenced code blocks blanked (line count kept) and inline code
 * spans removed, so links shown as code are not checked. */
function proseLines(markdown: string): string[] {
  let fence: string | undefined;
  return markdown.split("\n").map((line) => {
    const open = /^\s{0,3}(`{3,}|~{3,})/.exec(line);
    if (fence !== undefined) {
      if (open && open[1]?.[0] === fence[0] && (open[1]?.length ?? 0) >= fence.length) {
        fence = undefined;
      }
      return "";
    }
    if (open) {
      fence = open[1];
      return "";
    }
    return line.replace(/(`+)(?:(?!\1)[\s\S])*?\1/g, "");
  });
}

/** GitHub's heading slug: the heading's rendered text, lowercased, with every
 * character other than letters, digits, `-`, `_` and spaces dropped, and each space
 * turned into `-`. */
function githubSlug(heading: string): string {
  const text = heading
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1") // images: alt text
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1") // links: link text
    .replace(/\[([^\]]*)\]\[[^\]]*\]/g, "$1") // reference links
    .replace(/<[^>]+>/g, "") // HTML tags
    .replace(/`/g, "")
    .replace(/(\*\*|__|\*|~~)/g, "")
    .replace(/(^|\s)_([^_]+)_(?=\s|$)/g, "$1$2")
    .trim()
    .toLowerCase();
  return text.replace(/[^\p{L}\p{M}\p{N}\p{Pc}\- ]/gu, "").replace(/ /g, "-");
}

/** Every anchor a page defines: heading slugs (duplicates get -1, -2, …), explicit
 * `{#id}` heading ids and `<a id="…">` / `<a name="…">` tags. */
function anchors(markdown: string): Set<string> {
  const out = new Set<string>();
  const seen = new Map<string, number>();
  const lines = proseLines(markdown);
  const raw = markdown.split("\n");
  lines.forEach((line, i) => {
    const original = raw[i] ?? "";
    for (const m of original.matchAll(/<a\s+[^>]*\b(?:id|name)\s*=\s*["']([^"']+)["']/gi)) {
      if (line !== "" || original === "") out.add(m[1] ?? "");
    }
    let heading: string | undefined;
    const atx = /^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/.exec(line);
    if (atx) heading = atx[1];
    const next = lines[i + 1] ?? "";
    if (
      !heading &&
      line.trim() !== "" &&
      /^\s{0,3}(=+|-+)\s*$/.test(next) &&
      !/^\s*[-*|>]/.test(line)
    ) {
      heading = line.trim();
    }
    if (heading === undefined) return;
    // Headings keep their inline code for the slug: take it from the raw line.
    const rawHeading = atx
      ? (/^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/.exec(original)?.[1] ?? heading)
      : original.trim();
    const explicit = /\s*\{#([^}\s]+)\}\s*$/.exec(rawHeading);
    if (explicit) {
      out.add(explicit[1] ?? "");
      return;
    }
    const base = githubSlug(rawHeading);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    out.add(n === 0 ? base : `${base}-${n}`);
  });
  return out;
}

interface Link {
  page: string;
  line: number;
  target: string;
}

/** Inline links and images `[t](target)`, reference definitions `[t]: target` and
 * HTML `href` / `src` attributes, outside code. */
function links(page: string, markdown: string): Link[] {
  const out: Link[] = [];
  proseLines(markdown).forEach((line, i) => {
    const push = (target: string | undefined) => {
      if (target) out.push({ page, line: i + 1, target: target.trim() });
    };
    for (const m of line.matchAll(/\]\(\s*<?([^)\s>]+)>?(?:\s+["'(][^)]*)?\)/g)) push(m[1]);
    const def = /^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+["'(].*)?$/.exec(line);
    if (def) push(def[1]);
    for (const m of line.matchAll(/\b(?:href|src)\s*=\s*["']([^"']+)["']/gi)) push(m[1]);
  });
  return out;
}

/** Whether a link leaves the repository (a URL scheme or protocol-relative). */
const external = (target: string): boolean =>
  /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith("//");

/** Problems with one link, or undefined when it resolves. */
function check(link: Link, anchorCache: Map<string, Set<string>>): string | undefined {
  const [rawPath = "", rawAnchor] = link.target.split("#", 2);
  const path = decodeURIComponent(rawPath.split("?")[0] ?? "");
  const where = `${link.page}:${link.line}: ${link.target}`;
  let file: string;
  if (path === "") {
    file = join(repoRoot, link.page);
  } else {
    file = path.startsWith("/")
      ? join(repoRoot, path)
      : resolve(dirname(join(repoRoot, link.page)), path);
    const rel = relative(repoRoot, file);
    if (rel.startsWith("..")) return `${where}: points outside the repository`;
    if (!existsSync(file)) return `${where}: no such file`;
  }
  if (rawAnchor === undefined || rawAnchor === "") return undefined;
  if (!file.endsWith(".md") || statSync(file).isDirectory()) return undefined;
  let set = anchorCache.get(file);
  if (!set) {
    set = anchors(readFileSync(file, "utf8"));
    anchorCache.set(file, set);
  }
  const anchor = decodeURIComponent(rawAnchor);
  if (!set.has(anchor)) return `${where}: no heading or anchor #${anchor}`;
  return undefined;
}

describe("GitHub heading slugs (AC-SPYPSD.2)", () => {
  it("lowercases, drops punctuation but - and _, and turns spaces into -", () => {
    expect(githubSlug("Chart style and Save image")).toBe("chart-style-and-save-image");
    expect(githubSlug("What's new? (v0.12)")).toBe("whats-new-v012");
    expect(githubSlug("`--transport` and snake_case")).toBe("--transport-and-snake_case");
    expect(githubSlug("D+/D− hint")).toBe("dd-hint");
    expect(githubSlug("The [link](x.md) text")).toBe("the-link-text");
  });

  it("numbers duplicate headings and honours explicit anchors", () => {
    const set = anchors(
      '# Intro\n\n## Usage\n\n## Usage\n\n## Custom {#my-id}\n\n<a id="pinned"></a>\n\n```\n# not a heading\n```\n',
    );
    expect([...set].sort()).toEqual(["intro", "my-id", "pinned", "usage", "usage-1"].sort());
  });

  it("skips links inside fenced code and inline code", () => {
    const found = links(
      "x.md",
      "[a](a.md)\n\n```\n[b](b.md)\n```\n\nSee `[c](c.md)` and [d](d.md#e).\n",
    );
    expect(found.map((l) => l.target)).toEqual(["a.md", "d.md#e"]);
  });
});

describe("README and docs links resolve (AC-SPYPSD.2)", () => {
  const all = pages();

  it("checks the README and every docs page", () => {
    expect(all).toContain("README.md");
    expect(all).toContain("docs/README.md");
    expect(all).toContain("docs/reference/cli.md");
    expect(all).toContain("docs/how-to/web-edition.md");
  });

  it("finds links to check, anchors included", () => {
    const found = all.flatMap((p) => links(p, read(p))).filter((l) => !external(l.target));
    expect(found.length).toBeGreaterThan(50);
    expect(found.some((l) => l.target.includes("#"))).toBe(true);
  });

  it("every relative link points at a file that exists, and every anchor at a heading", () => {
    const cache = new Map<string, Set<string>>();
    const problems: string[] = [];
    for (const page of all) {
      for (const link of links(page, read(page))) {
        if (external(link.target)) continue;
        const problem = check(link, cache);
        if (problem) problems.push(problem);
      }
    }
    expect(problems).toEqual([]);
  });
});

describe("the link check runs in the gate on every OS (AC-SPYPSD.2)", () => {
  it("Vitest picks up scripts/**/*.test.ts", () => {
    const config = read("apps/desktop/vite.config.ts");
    expect(config).toMatch(/["']scripts\/\*\*\/\*\.test\.ts["']/);
  });

  it("the CI gate job runs bun run test on Linux, macOS and Windows", () => {
    const ci = read(".github/workflows/ci.yml");
    const gate = /\n {2}gate:\n([\s\S]*?)(?=\n {2}[a-z0-9_-]+:\n|$)/.exec(ci)?.[1] ?? "";
    expect(gate).toMatch(/ubuntu/);
    expect(gate).toMatch(/macos/);
    expect(gate).toMatch(/windows/);
    expect(gate).toMatch(/bun run test\b/);
  });
});
