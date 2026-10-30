// Release publishing (FR 4V3DHP, after FR E1ZJ2N).
//
// The release workflow's publish job runs this script once every file is built and
// checked (`bun publish.ts --notes <file> <dist files...>`, with TAG, TITLE,
// PRERELEASE, GH_TOKEN and GH_REPO in the environment). It finds the release for the
// tag (drafts included) or creates a draft, and from then on addresses the release by
// the id it holds: the one in the create response, or the one the list returned. It
// never looks a new draft up again, because the release list can lag behind a create
// (the v0.6.0 run failed that way). The release stays a draft until every file is
// attached and checked; files left over from an earlier run are deleted only then.
// Every update sends the tag name, and the published release's tag is checked
// against the tag this run built (FR 4PJRVN).

import { readFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseTag } from "./versions";

/** A release as the GitHub releases API returns it (only the fields used here). */
export interface Release {
  id: number;
  tag_name: string;
  draft: boolean;
  prerelease?: boolean;
  name?: string | null;
  body?: string | null;
}

/** A release asset (only the fields used here). */
export interface Asset {
  id: number;
  name: string;
}

/** The fields of a release that `updateRelease` changes. */
export interface ReleasePatch {
  /** Always sent: a PATCH without it can leave a published draft on an `untagged-…` tag. */
  tag_name: string;
  name?: string;
  body?: string;
  draft?: boolean;
  prerelease?: boolean;
  make_latest?: "true" | "false" | "legacy";
}

/** The GitHub API calls the publish logic needs. */
export interface GitHubClient {
  /** Every release of the repository, drafts included. */
  listReleases(): Promise<Release[]>;
  tagExists(tag: string): Promise<boolean>;
  createDraftRelease(input: { tag_name: string; name: string; body: string }): Promise<Release>;
  updateRelease(id: number, patch: ReleasePatch): Promise<Release>;
  listAssets(id: number): Promise<Asset[]>;
  deleteAsset(assetId: number): Promise<void>;
  uploadAsset(id: number, name: string, path: string): Promise<void>;
}

/** What to publish. */
export interface PublishOptions {
  tag: string;
  title: string;
  notes: string;
  prerelease: boolean;
  /** Paths of the files to attach; each is attached under its base name. */
  files: string[];
}

/** True for a GitHub id: a positive integer. */
export const isReleaseId = (id: unknown): id is number =>
  typeof id === "number" && Number.isInteger(id) && id > 0;

/** The numeric `[major, minor, patch]` of a release tag (see `parseTag`), or undefined. */
function versionOf(tag: string): number[] | undefined {
  try {
    return parseTag(tag).version.split("-")[0]?.split(".").map(Number);
  } catch {
    return undefined;
  }
}

/** Numeric order of two versions (negative when a < b). */
function compareVersions(a: number[], b: number[]): number {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return (a[i] ?? 0) - (b[i] ?? 0);
  return 0;
}

/**
 * True when another published, non-pre-release release has a higher version than
 * `tag`: a manual run for an older tag must not take the Latest badge from it.
 */
export function newerFinalExists(releases: Release[], tag: string): boolean {
  const own = versionOf(tag);
  if (own === undefined) return false;
  return releases.some((r) => {
    if (r.draft || r.prerelease || r.tag_name === tag) return false;
    const v = versionOf(r.tag_name);
    return v !== undefined && compareVersions(v, own) > 0;
  });
}

const log = (msg: string): void => {
  process.stderr.write(`${msg}\n`);
};

/**
 * Creates or finishes the release for `opts.tag` and publishes it once every file is
 * attached. Throws (naming the tag) before any write when the tag is missing, when
 * the tag has more than one release, or when a release id is not a positive integer.
 */
export async function publishRelease(gh: GitHubClient, opts: PublishOptions): Promise<void> {
  const { tag } = opts;
  if (!(await gh.tagExists(tag))) {
    throw new Error(`tag ${tag} does not exist; push it before publishing`);
  }

  const releases = await gh.listReleases();
  const existing = releases.filter((r) => r.tag_name === tag);
  if (existing.length > 1) {
    const ids = existing.map((r) => String(r.id)).join(", ");
    throw new Error(
      `more than one release for ${tag} (${existing.length} releases: ${ids}); delete the extra drafts first`,
    );
  }

  let id: number;
  const found = existing[0];
  if (found !== undefined) {
    if (!isReleaseId(found.id)) {
      throw new Error(`the release listed for ${tag} has no valid id (${String(found.id)})`);
    }
    id = found.id;
    log(`release ${id} exists for ${tag}; updating its title and notes`);
    await gh.updateRelease(id, { tag_name: tag, name: opts.title, body: opts.notes });
  } else {
    const created = await gh.createDraftRelease({
      tag_name: tag,
      name: opts.title,
      body: opts.notes,
    });
    if (!isReleaseId(created?.id)) {
      throw new Error(
        `the create response for ${tag} carries no valid release id (${String(created?.id)})`,
      );
    }
    id = created.id;
    log(`created draft release ${id} for ${tag}`);
  }

  // Replace same-name assets (a re-run), then attach every file. On an already
  // published release a replaced file is missing until its upload lands, as with
  // `gh release upload --clobber`.
  const before = await gh.listAssets(id);
  const names = opts.files.map((path) => basename(path));
  for (const path of opts.files) {
    const name = basename(path);
    for (const a of before.filter((x) => x.name === name)) await gh.deleteAsset(a.id);
    log(`uploading ${name}`);
    await gh.uploadAsset(id, name, path);
  }

  // Every file must be on the release under its own name before anything is deleted.
  const after = await gh.listAssets(id);
  const present = new Set(after.map((a) => a.name));
  const missing = names.filter((n) => !present.has(n));
  if (missing.length > 0) {
    throw new Error(`${missing.join(", ")} not on release ${id} (${tag}) after upload`);
  }

  // Drop files left over from an earlier run that this run did not build.
  const wanted = new Set(names);
  for (const a of after) {
    if (!wanted.has(a.name)) {
      log(`deleting leftover asset ${a.name}`);
      await gh.deleteAsset(a.id);
    }
  }

  const published = await gh.updateRelease(id, {
    tag_name: tag,
    draft: false,
    prerelease: opts.prerelease,
    make_latest: opts.prerelease || newerFinalExists(releases, tag) ? "false" : "true",
  });
  log(`published release ${id} for ${tag}`);

  // The published release must sit on the tag this run built (the v0.6.0 run ended
  // on an `untagged-…` tag); the check reads the tag GitHub reports in the response.
  if (published?.tag_name !== tag) {
    throw new Error(
      `release ${id} was published on tag ${String(published?.tag_name)}, not on ${tag}; ` +
        `it is public now, so set its tag by hand (PATCH /releases/${id} with tag_name=${tag})`,
    );
  }
}

// ---------------------------------------------------------------------------
// The GitHub REST client over fetch
// ---------------------------------------------------------------------------

export interface GitHubClientOptions {
  token: string;
  /** `owner/name`. */
  repo: string;
  fetch?: typeof fetch;
}

const API = "https://api.github.com";
const UPLOADS = "https://uploads.github.com";

/** The parts of a request that differ between API calls. */
interface RequestOptions {
  method?: string;
  body?: BodyInit;
  contentType?: string;
}

function requireId(what: string, id: unknown): number {
  if (!isReleaseId(id)) throw new Error(`invalid ${what} id: ${String(id)}`);
  return id;
}

/** The `rel="next"` URL of a Link header, or undefined. */
function nextLink(link: string | null): string | undefined {
  if (!link) return undefined;
  for (const part of link.split(",")) {
    const m = /<([^>]+)>\s*;\s*rel="next"/.exec(part);
    if (m) return m[1];
  }
  return undefined;
}

/** A GitHubClient over the GitHub REST API. */
export function createGitHubClient(opts: GitHubClientOptions): GitHubClient {
  const doFetch = opts.fetch ?? fetch;
  const repoUrl = `${API}/repos/${opts.repo}`;

  const request = async (url: string, init: RequestOptions = {}): Promise<Response> => {
    const headers: Record<string, string> = {
      authorization: `Bearer ${opts.token}`,
      accept: "application/vnd.github+json",
      "x-github-api-version": "2022-11-28",
    };
    if (init.contentType) headers["content-type"] = init.contentType;
    return doFetch(url, { method: init.method ?? "GET", headers, body: init.body });
  };

  const fail = async (res: Response, what: string): Promise<never> => {
    const text = await res.text().catch(() => "");
    throw new Error(`${what}: HTTP ${res.status} ${res.statusText} ${text}`.trim());
  };

  const send = async <T>(what: string, url: string, init: RequestOptions = {}): Promise<T> => {
    const res = await request(url, init);
    if (!res.ok) return fail(res, what);
    if (res.status === 204) return undefined as T;
    return (await res.json()) as T;
  };

  const paginate = async <T>(what: string, url: string): Promise<T[]> => {
    const out: T[] = [];
    let next: string | undefined = url;
    while (next !== undefined) {
      const res = await request(next);
      if (!res.ok) return fail(res, what);
      out.push(...((await res.json()) as T[]));
      next = nextLink(res.headers.get("link"));
    }
    return out;
  };

  const json = (data: unknown): RequestOptions => ({
    body: JSON.stringify(data),
    contentType: "application/json",
  });

  return {
    listReleases: () => paginate<Release>("list releases", `${repoUrl}/releases?per_page=100`),

    async tagExists(tag) {
      const res = await request(`${repoUrl}/git/ref/tags/${encodeURIComponent(tag)}`);
      if (res.status === 404) return false;
      if (!res.ok) return fail(res, `check tag ${tag}`);
      return true;
    },

    createDraftRelease: (input) =>
      send<Release>(`create release ${input.tag_name}`, `${repoUrl}/releases`, {
        method: "POST",
        ...json({ ...input, draft: true }),
      }),

    async updateRelease(id, patch) {
      const rid = requireId("release", id);
      return send<Release>(`update release ${rid}`, `${repoUrl}/releases/${rid}`, {
        method: "PATCH",
        ...json(patch),
      });
    },

    async listAssets(id) {
      const rid = requireId("release", id);
      return paginate<Asset>(
        `list assets of release ${rid}`,
        `${repoUrl}/releases/${rid}/assets?per_page=100`,
      );
    },

    async deleteAsset(assetId) {
      const aid = requireId("asset", assetId);
      await send<void>(`delete asset ${aid}`, `${repoUrl}/releases/assets/${aid}`, {
        method: "DELETE",
      });
    },

    async uploadAsset(id, name, path) {
      const rid = requireId("release", id);
      const url = `${UPLOADS}/repos/${opts.repo}/releases/${rid}/assets?name=${encodeURIComponent(name)}`;
      await send<unknown>(`upload ${name} to release ${rid}`, url, {
        method: "POST",
        body: new Uint8Array(readFileSync(path)),
        contentType: "application/octet-stream",
      });
    },
  };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

async function main(argv: string[]): Promise<number> {
  const files: string[] = [];
  let notesPath: string | undefined;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i] ?? "";
    if (a === "--notes") notesPath = argv[++i];
    else files.push(a);
  }
  const env = process.env;
  const missing = ["GH_TOKEN", "GH_REPO", "TAG", "TITLE"].filter((k) => !env[k]);
  if (notesPath === undefined) missing.push("--notes <file>");
  if (files.length === 0) missing.push("files to attach");
  if (missing.length > 0) {
    process.stderr.write(
      `usage: bun publish.ts --notes <file> <files...> (env TAG, TITLE, PRERELEASE, GH_TOKEN, GH_REPO); missing: ${missing.join(", ")}\n`,
    );
    return 2;
  }
  const gh = createGitHubClient({ token: env.GH_TOKEN ?? "", repo: env.GH_REPO ?? "" });
  try {
    await publishRelease(gh, {
      tag: env.TAG ?? "",
      title: env.TITLE ?? "",
      notes: readFileSync(notesPath ?? "", "utf8"),
      prerelease: env.PRERELEASE === "true",
      files,
    });
  } catch (e) {
    process.stderr.write(`${e instanceof Error ? e.message : String(e)}\n`);
    return 1;
  }
  return 0;
}

// Only when run as a script: the tests import this module.
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(await main(process.argv.slice(2)));
}
