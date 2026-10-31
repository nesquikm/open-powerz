// CHANGELOG section lookup for releases (FR E1ZJ2N).
//
// Headings look like `## [0.6.0] — 2026-10-28 — "Special Delivery"`; the date and
// the codename (quoted or not) are optional. A section ends at the next `## `
// heading or at the Keep a Changelog link references (`[0.6.0]: https://…`).
// An unquoted codename never starts like a date (`2026-10-2`, `2026/10/28`), so a
// heading with a malformed date is not a release heading at all.

/** One version's CHANGELOG section. */
export interface ReleaseSection {
  version: string;
  date?: string;
  codename?: string;
  /** The section body, trimmed, with LF line endings. */
  notes: string;
}

const HEADING_RE =
  /^## \[([^\]]+)\](?:\s*[—–-]\s*(\d{4}-\d{2}-\d{2}))?(?:\s*[—–-]\s*(?:"([^"]*)"|(?!\d+[-/.]\d+[-/.]\d)([^"\s].*?)))?\s*$/;
const LINK_REF_RE = /^\[[^\]]+\]:\s/;

/**
 * The first section for exactly `version` (a pre-release never matches its final
 * version), or null.
 */
export function extractRelease(changelog: string, version: string): ReleaseSection | null {
  const lines = changelog.replace(/\r\n/g, "\n").split("\n");
  for (let i = 0; i < lines.length; i++) {
    const m = HEADING_RE.exec(lines[i] ?? "");
    if (!m || m[1] !== version) continue;
    const body: string[] = [];
    for (const line of lines.slice(i + 1)) {
      if (line.startsWith("## ") || LINK_REF_RE.test(line)) break;
      body.push(line);
    }
    const section: ReleaseSection = { version, notes: body.join("\n").trim() };
    if (m[2] !== undefined) section.date = m[2];
    const codename = m[3] ?? m[4];
    if (codename !== undefined) section.codename = codename;
    return section;
  }
  return null;
}

/** `v0.6.0 — "Special Delivery"`, or just the tag without a codename. */
export function releaseTitle(tag: string, codename?: string): string {
  return codename ? `${tag} — "${codename}"` : tag;
}
