// FR 4V3DHP: the release publish job keeps the id of the draft it created (or the
// release it found by tag) and addresses the release by that id for the asset check,
// the cleanup and the final publish. The v0.6.0 run failed because it looked the new
// draft up again in a release list that did not show it yet (AC-4V3DHP.1–4, 6).
// AC-4V3DHP.5 (manual run for v0.6.0 and the next tag) is checked after the merge.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createGitHubClient,
  publishRelease,
  type GitHubClient,
  type PublishOptions,
} from "./publish";

// ---------------------------------------------------------------------------
// In-memory fake GitHub API
// ---------------------------------------------------------------------------

interface FakeAsset {
  id: number;
  name: string;
}

interface FakeRelease {
  id: unknown;
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  name: string;
  body: string;
  make_latest?: string;
  assets: FakeAsset[];
}

interface Call {
  method: string;
  args: unknown[];
}

interface FakeOptions {
  /** Releases that exist before the run (drafts included). */
  releases?: {
    tag_name: string;
    draft?: boolean;
    prerelease?: boolean;
    name?: string;
    assets?: string[];
    id?: unknown;
  }[];
  /** The tag of the release that holds the Latest badge before the run (default: the highest published final version). */
  latest?: string;
  /** GitHub reports this tag for the release once it is published, whatever was sent. */
  retagOnPublish?: string;
  /** Tags that exist in the repository. */
  tags?: string[];
  /** listReleases misses a release created in this run for this many calls (Infinity = never shows it). */
  listLag?: number;
  /** Uploads of these names "succeed" but never land on the release. */
  dropUploads?: string[];
  /** Overrides the id the create response carries. */
  createdId?: unknown;
}

const isValidId = (id: unknown): boolean =>
  typeof id === "number" && Number.isInteger(id) && id > 0;

/** Semantic order of two `vX.Y.Z[-pre]` tags (negative when a < b); a pre-release sorts before its final. */
function compareTags(a: string, b: string): number {
  const parse = (t: string) => {
    const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-(.+))?$/.exec(t);
    return m ? { n: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] } : { n: [0, 0, 0] };
  };
  const x = parse(a);
  const y = parse(b);
  for (let i = 0; i < 3; i++) if (x.n[i] !== y.n[i]) return (x.n[i] ?? 0) - (y.n[i] ?? 0);
  if (x.pre === y.pre) return 0;
  if (x.pre === undefined) return 1;
  if (y.pre === undefined) return -1;
  return x.pre < y.pre ? -1 : 1;
}

class FakeGitHub implements GitHubClient {
  calls: Call[] = [];
  releases: FakeRelease[] = [];
  private tags: Set<string>;
  private nextReleaseId = 40413437;
  private nextAssetId = 9000;
  private lagLeft: number;
  private hidden = new Set<FakeRelease>();
  private drop: Set<string>;
  private createdId: unknown;
  private hasCreatedId: boolean;
  private retag: string | undefined;
  /** The id of the release that holds the Latest badge, or undefined. */
  latestId: unknown;

  constructor(opts: FakeOptions = {}) {
    this.tags = new Set(opts.tags ?? []);
    this.lagLeft = opts.listLag ?? 0;
    this.drop = new Set(opts.dropUploads ?? []);
    this.hasCreatedId = "createdId" in opts;
    this.createdId = opts.createdId;
    for (const r of opts.releases ?? []) {
      this.releases.push({
        id: "id" in r ? r.id : this.nextReleaseId++,
        tag_name: r.tag_name,
        draft: r.draft ?? false,
        prerelease: r.prerelease ?? false,
        name: r.name ?? r.tag_name,
        body: "",
        assets: (r.assets ?? []).map((name) => ({ id: this.nextAssetId++, name })),
      });
    }
    this.retag = opts.retagOnPublish;
    const latest =
      opts.latest !== undefined
        ? this.releases.find((r) => r.tag_name === opts.latest)
        : this.highestFinal();
    this.latestId = latest?.id;
  }

  /** The published, non-pre-release release with the highest version (GitHub's "legacy" rule). */
  private highestFinal(): FakeRelease | undefined {
    return this.releases
      .filter((r) => !r.draft && !r.prerelease)
      .sort((a, b) => compareTags(a.tag_name, b.tag_name))
      .at(-1);
  }

  /** The tag of the release that holds the Latest badge, or undefined. */
  latestTag(): string | undefined {
    return this.releases.find((r) => r.id === this.latestId)?.tag_name;
  }

  private view(r: FakeRelease) {
    return {
      id: r.id as number,
      tag_name: r.tag_name,
      draft: r.draft,
      prerelease: r.prerelease,
      name: r.name,
      body: r.body,
    };
  }

  private record(method: string, ...args: unknown[]): void {
    this.calls.push({ method, args: structuredClone(args) });
  }

  private byId(id: unknown): FakeRelease {
    const r = this.releases.find((x) => x.id === id);
    if (!r || !isValidId(id)) throw new Error(`gh: Not Found (HTTP 404) for release ${String(id)}`);
    return r;
  }

  listReleases = async () => {
    this.record("listReleases");
    const visible = this.releases.filter((r) => !this.hidden.has(r));
    // The lag counts only list calls made after the create: a list before it sees nothing new.
    if (this.hidden.size > 0) {
      this.lagLeft--;
      if (this.lagLeft <= 0) this.hidden.clear();
    }
    return visible.map((r) => this.view(r));
  };

  tagExists = async (tag: string) => {
    this.record("tagExists", tag);
    return this.tags.has(tag);
  };

  createDraftRelease = async (input: { tag_name: string; name: string; body: string }) => {
    this.record("createDraftRelease", input);
    const r: FakeRelease = {
      id: this.hasCreatedId ? this.createdId : this.nextReleaseId++,
      tag_name: input.tag_name,
      draft: true,
      prerelease: false,
      name: input.name,
      body: input.body,
      assets: [],
    };
    this.releases.push(r);
    if (this.lagLeft > 0) this.hidden.add(r);
    return {
      id: r.id as number,
      tag_name: r.tag_name,
      draft: true,
      prerelease: false,
      name: r.name,
      body: r.body,
    };
  };

  updateRelease = async (
    id: number,
    patch: {
      tag_name?: string;
      name?: string;
      body?: string;
      draft?: boolean;
      prerelease?: boolean;
      make_latest?: "true" | "false" | "legacy";
    },
  ) => {
    this.record("updateRelease", id, patch);
    const r = this.byId(id);
    const publishing = r.draft && patch.draft === false;
    if (patch.tag_name !== undefined) r.tag_name = patch.tag_name;
    // Like GitHub on the v0.6.0 run: publishing a draft with a PATCH that carries no
    // tag_name leaves the release on an "untagged-…" tag.
    else if (publishing) r.tag_name = `untagged-${(r.id as number).toString(16)}`;
    if (patch.name !== undefined) r.name = patch.name;
    if (patch.body !== undefined) r.body = patch.body;
    if (patch.draft !== undefined) r.draft = patch.draft;
    if (patch.prerelease !== undefined) r.prerelease = patch.prerelease;
    if (patch.make_latest !== undefined) r.make_latest = patch.make_latest;
    if (publishing && this.retag !== undefined) r.tag_name = this.retag;
    // The Latest badge: "true" takes it, "false" leaves it, "legacy" goes to the highest
    // published final version; GitHub's default for a newly published release is "true".
    if (!r.draft && !r.prerelease) {
      const mode = patch.make_latest ?? (publishing ? "true" : undefined);
      if (mode === "true") this.latestId = r.id;
      else if (mode === "legacy") this.latestId = this.highestFinal()?.id;
    }
    return this.view(r);
  };

  /** GET /releases/{id}. */
  getRelease = async (id: number) => {
    this.record("getRelease", id);
    return this.view(this.byId(id));
  };

  /** GET /releases/latest. */
  getLatestRelease = async () => {
    this.record("getLatestRelease");
    const r = this.releases.find((x) => x.id === this.latestId);
    if (!r) throw new Error("gh: Not Found (HTTP 404) for the latest release");
    return this.view(r);
  };

  listAssets = async (id: number) => {
    this.record("listAssets", id);
    return this.byId(id).assets.map((a) => ({ ...a }));
  };

  deleteAsset = async (assetId: number) => {
    this.record("deleteAsset", assetId);
    for (const r of this.releases) {
      const at = r.assets.findIndex((a) => a.id === assetId);
      if (at >= 0) {
        r.assets.splice(at, 1);
        return;
      }
    }
    throw new Error(`gh: Not Found (HTTP 404) for asset ${assetId}`);
  };

  uploadAsset = async (id: number, name: string, path: string) => {
    this.record("uploadAsset", id, name, path);
    const r = this.byId(id);
    // Like GitHub: a second asset with the same name is refused, so a re-run must replace it.
    if (r.assets.some((a) => a.name === name))
      throw new Error(`Validation Failed: already_exists (HTTP 422) for ${name}`);
    if (this.drop.has(name)) return;
    r.assets.push({ id: this.nextAssetId++, name });
  };

  /** Calls whose first argument is a release id. */
  idCalls(): Call[] {
    return this.calls.filter((c) =>
      ["updateRelease", "listAssets", "uploadAsset", "getRelease"].includes(c.method),
    );
  }

  /** Calls that change something on GitHub. */
  writes(): Call[] {
    return this.calls.filter((c) =>
      ["createDraftRelease", "updateRelease", "deleteAsset", "uploadAsset"].includes(c.method),
    );
  }

  indexOf(pred: (c: Call) => boolean): number {
    return this.calls.findIndex(pred);
  }

  lastIndexOf(pred: (c: Call) => boolean): number {
    for (let i = this.calls.length - 1; i >= 0; i--) if (pred(this.calls[i]!)) return i;
    return -1;
  }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const TAG = "v0.6.0";
const FILE_NAMES = [
  "open-powerz_0.6.0_universal.dmg",
  "open-powerz_0.6.0_x64-setup.exe",
  "open-powerz_0.6.0_x64_en-US.msi",
  "open-powerz_0.6.0_amd64.AppImage",
  "open-powerz_0.6.0_amd64.deb",
  "powerz-0.6.0-macos-universal.tar.gz",
  "powerz-0.6.0-linux-x86_64.tar.gz",
  "powerz-0.6.0-windows-x86_64.zip",
  "SHA256SUMS.txt",
];

let dist = "";
let files: string[] = [];

beforeEach(() => {
  dist = mkdtempSync(join(tmpdir(), "publish-test-"));
  files = FILE_NAMES.map((name) => {
    const p = join(dist, name);
    writeFileSync(p, `contents of ${name}\n`);
    return p;
  });
});

afterEach(() => {
  rmSync(dist, { recursive: true, force: true });
});

const options = (over: Partial<PublishOptions> = {}): PublishOptions => ({
  tag: TAG,
  title: 'v0.6.0 — "Special Delivery"',
  notes: "### Added\n\n- Release pipeline.\n",
  prerelease: false,
  files,
  ...over,
});

const finalPublish = (gh: FakeGitHub): Call | undefined =>
  gh.calls
    .filter(
      (c) => c.method === "updateRelease" && (c.args[1] as { draft?: boolean })?.draft === false,
    )
    .at(-1);

const releaseFor = (gh: FakeGitHub, tag = TAG): FakeRelease[] =>
  gh.releases.filter((r) => r.tag_name === tag);

// ---------------------------------------------------------------------------
// AC-4V3DHP.1: a fresh tag — the id comes from the create response
// ---------------------------------------------------------------------------

describe("fresh tag (AC-4V3DHP.1, AC-4V3DHP.4)", () => {
  it("creates one draft, attaches every file and publishes it", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    const rs = releaseFor(gh);
    expect(rs).toHaveLength(1);
    const r = rs[0]!;
    expect(r.draft).toBe(false);
    expect(r.prerelease).toBe(false);
    expect(r.make_latest).toBe("true");
    expect(r.name).toBe('v0.6.0 — "Special Delivery"');
    expect(r.body).toBe("### Added\n\n- Release pipeline.\n");
    expect(r.assets.map((a) => a.name).sort()).toEqual([...FILE_NAMES].sort());
  });

  it("creates the release as a draft with the title and notes", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    const creates = gh.calls.filter((c) => c.method === "createDraftRelease");
    expect(creates).toHaveLength(1);
    expect(creates[0]!.args[0]).toMatchObject({
      tag_name: TAG,
      name: 'v0.6.0 — "Special Delivery"',
      body: "### Added\n\n- Release pipeline.\n",
    });
  });

  it("uses the created id for the asset check, the uploads and the final publish", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    const created = releaseFor(gh)[0]!.id;
    const idCalls = gh.idCalls();
    expect(idCalls.map((c) => c.method)).toEqual(
      expect.arrayContaining(["listAssets", "uploadAsset", "updateRelease"]),
    );
    for (const c of idCalls) expect(c.args[0], `${c.method} used another id`).toBe(created);
  });

  it("never lists the releases again after creating the draft", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    const create = gh.indexOf((c) => c.method === "createDraftRelease");
    expect(create).toBeGreaterThanOrEqual(0);
    expect(gh.lastIndexOf((c) => c.method === "listReleases")).toBeLessThan(create);
  });

  it("publishes only after every file is attached and checked", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    const lastUpload = gh.lastIndexOf((c) => c.method === "uploadAsset");
    const check = gh.lastIndexOf((c) => c.method === "listAssets");
    const publish = gh.calls.indexOf(finalPublish(gh)!);
    expect(publish, "the draft is never published").toBeGreaterThanOrEqual(0);
    expect(lastUpload).toBeLessThan(check);
    expect(check).toBeLessThan(publish);
    expect(publish).toBe(gh.calls.length - 1);
    // Nothing un-drafts the release before the final publish.
    const early = gh.calls
      .slice(0, publish)
      .filter(
        (c) => c.method === "updateRelease" && (c.args[1] as { draft?: boolean })?.draft === false,
      );
    expect(early).toEqual([]);
  });

  it("uploads each file under its own name from its path", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    const uploads = gh.calls.filter((c) => c.method === "uploadAsset");
    expect(uploads).toHaveLength(FILE_NAMES.length);
    for (const u of uploads) expect(u.args[1]).toBe(basename(u.args[2] as string));
    expect(uploads.map((u) => u.args[2]).sort()).toEqual([...files].sort());
  });

  it("refuses a tag that does not exist and creates nothing", async () => {
    const gh = new FakeGitHub({ tags: [] });
    await expect(publishRelease(gh, options())).rejects.toThrow(/v0\.6\.0/);
    expect(gh.writes()).toEqual([]);
    expect(gh.releases).toEqual([]);
  });

  it("marks a pre-release tag as a pre-release that is not the latest", async () => {
    const tag = "v0.7.0-rc.1";
    const gh = new FakeGitHub({ tags: [tag] });
    await publishRelease(gh, options({ tag, title: "v0.7.0-rc.1", prerelease: true }));

    expect(finalPublish(gh)?.args[1]).toMatchObject({
      draft: false,
      prerelease: true,
      make_latest: "false",
    });
    const r = releaseFor(gh, tag)[0]!;
    expect(r.draft).toBe(false);
    expect(r.prerelease).toBe(true);
    expect(r.make_latest).toBe("false");
  });

  it("publishes a final release as the latest, not a pre-release", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    expect(finalPublish(gh)?.args[1]).toMatchObject({
      draft: false,
      prerelease: false,
      make_latest: "true",
    });
  });
});

// ---------------------------------------------------------------------------
// AC-4V3DHP.2: an existing release for the tag (the v0.6.0 draft) is finished
// ---------------------------------------------------------------------------

describe("existing release for the tag (AC-4V3DHP.2, AC-4V3DHP.4)", () => {
  const leftDraft = () =>
    new FakeGitHub({
      tags: [TAG, "v0.5.0"],
      releases: [
        { tag_name: "v0.5.0", draft: false, assets: ["old.dmg"] },
        { tag_name: TAG, draft: true, name: "stale title", assets: FILE_NAMES },
      ],
    });

  it("finds the draft by tag, does not create another and publishes it", async () => {
    const gh = leftDraft();
    const draftId = releaseFor(gh)[0]!.id;
    await publishRelease(gh, options());

    expect(gh.calls.filter((c) => c.method === "createDraftRelease")).toEqual([]);
    expect(releaseFor(gh)).toHaveLength(1);
    const r = releaseFor(gh)[0]!;
    expect(r.id).toBe(draftId);
    expect(r.draft).toBe(false);
    expect(r.make_latest).toBe("true");
  });

  it("addresses every later call by the listed release's id", async () => {
    const gh = leftDraft();
    const draftId = releaseFor(gh)[0]!.id;
    await publishRelease(gh, options());

    const idCalls = gh.idCalls();
    expect(idCalls.map((c) => c.method)).toEqual(
      expect.arrayContaining(["listAssets", "uploadAsset", "updateRelease"]),
    );
    for (const c of idCalls) expect(c.args[0], `${c.method} used another id`).toBe(draftId);
    // The other tag's release is untouched.
    const other = gh.releases.find((r) => r.tag_name === "v0.5.0")!;
    expect(other.assets.map((a) => a.name)).toEqual(["old.dmg"]);
  });

  it("updates the existing release's title and notes", async () => {
    const gh = leftDraft();
    await publishRelease(gh, options());

    const r = releaseFor(gh)[0]!;
    expect(r.name).toBe('v0.6.0 — "Special Delivery"');
    expect(r.body).toBe("### Added\n\n- Release pipeline.\n");
  });

  it("replaces same-name assets instead of failing on them (clobber)", async () => {
    const gh = leftDraft();
    const before = new Map(releaseFor(gh)[0]!.assets.map((a) => [a.name, a.id]));
    await publishRelease(gh, options());

    const after = releaseFor(gh)[0]!.assets;
    expect(after.map((a) => a.name).sort()).toEqual([...FILE_NAMES].sort());
    for (const a of after)
      expect(a.id, `${a.name} was not re-uploaded`).not.toBe(before.get(a.name));
    expect(gh.calls.filter((c) => c.method === "uploadAsset")).toHaveLength(FILE_NAMES.length);
  });

  it("deletes leftover assets only after every file is on the release", async () => {
    const gh = new FakeGitHub({
      tags: [TAG],
      releases: [
        {
          tag_name: TAG,
          draft: true,
          assets: ["open-powerz_0.6.0-old_universal.dmg", "stray.txt"],
        },
      ],
    });
    const leftoverIds = releaseFor(gh)[0]!.assets.map((a) => a.id);
    await publishRelease(gh, options());

    const r = releaseFor(gh)[0]!;
    expect(r.assets.map((a) => a.name).sort()).toEqual([...FILE_NAMES].sort());
    const leftoverDeletes = gh.calls
      .map((c, i) => ({ c, i }))
      .filter(({ c }) => c.method === "deleteAsset" && leftoverIds.includes(c.args[0] as number));
    expect(leftoverDeletes).toHaveLength(2);
    const lastUpload = gh.lastIndexOf((c) => c.method === "uploadAsset");
    const firstCheck = gh.calls.findIndex((c, i) => c.method === "listAssets" && i > lastUpload);
    expect(firstCheck, "no asset check after the uploads").toBeGreaterThan(lastUpload);
    for (const { i } of leftoverDeletes) expect(i).toBeGreaterThan(firstCheck);
    expect(gh.calls.indexOf(finalPublish(gh)!)).toBeGreaterThan(leftoverDeletes.at(-1)!.i);
  });

  it("finishes an already published release on a re-run", async () => {
    const gh = new FakeGitHub({
      tags: [TAG],
      releases: [{ tag_name: TAG, draft: false, assets: FILE_NAMES }],
    });
    await publishRelease(gh, options());

    expect(gh.calls.filter((c) => c.method === "createDraftRelease")).toEqual([]);
    expect(releaseFor(gh)).toHaveLength(1);
    expect(
      releaseFor(gh)[0]!
        .assets.map((a) => a.name)
        .sort(),
    ).toEqual([...FILE_NAMES].sort());
  });
});

// ---------------------------------------------------------------------------
// AC-4V3DHP.3: an empty or malformed release id stops before any call uses it
// ---------------------------------------------------------------------------

describe("invalid release ids (AC-4V3DHP.3)", () => {
  it.each([
    ["undefined", undefined],
    ["null", null],
    ["an empty string", ""],
    ["a numeric string", "40413437"],
    ["zero", 0],
    ["a negative number", -5],
    ["a fraction", 1.5],
    ["NaN", Number.NaN],
  ])("stops when the create response carries %s as id", async (_, bad) => {
    const gh = new FakeGitHub({ tags: [TAG], createdId: bad });
    const err = await publishRelease(gh, options()).then(
      () => undefined,
      (e: unknown) => e,
    );
    expect(err, "publishRelease did not fail").toBeInstanceOf(Error);
    expect((err as Error).message).toMatch(/v0\.6\.0/);
    expect((err as Error).message).toMatch(/\bid\b/i);
    expect(gh.idCalls(), "a call used the invalid id").toEqual([]);
    expect(gh.calls.filter((c) => c.method === "deleteAsset")).toEqual([]);
  });

  it.each([
    ["an empty string", ""],
    ["undefined", undefined],
    ["a string", "abc"],
  ])("stops when the listed release has %s as id", async (_, bad) => {
    const gh = new FakeGitHub({
      tags: [TAG],
      releases: [{ tag_name: TAG, draft: true, id: bad, assets: [] }],
    });
    await expect(publishRelease(gh, options())).rejects.toThrow(/v0\.6\.0/);
    expect(gh.idCalls()).toEqual([]);
    expect(gh.calls.filter((c) => c.method === "createDraftRelease")).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// AC-4V3DHP.4: the remaining failure cases
// ---------------------------------------------------------------------------

describe("failure cases (AC-4V3DHP.4)", () => {
  it("refuses two releases for one tag and changes nothing", async () => {
    const gh = new FakeGitHub({
      tags: [TAG],
      releases: [
        { tag_name: TAG, draft: true },
        { tag_name: TAG, draft: true },
      ],
    });
    await expect(publishRelease(gh, options())).rejects.toThrow(/v0\.6\.0/);
    await expect(
      publishRelease(
        new FakeGitHub({
          tags: [TAG],
          releases: [
            { tag_name: TAG, draft: true },
            { tag_name: TAG, draft: false },
          ],
        }),
        options(),
      ),
    ).rejects.toThrow(/more than one|2 releases|two releases/i);
    expect(gh.writes()).toEqual([]);
  });

  it("stops before publishing when an upload leaves a file missing", async () => {
    const missing = "powerz-0.6.0-linux-x86_64.tar.gz";
    const gh = new FakeGitHub({
      tags: [TAG],
      releases: [{ tag_name: TAG, draft: true, assets: ["stray.txt"] }],
      dropUploads: [missing],
    });
    await expect(publishRelease(gh, options())).rejects.toThrow(missing);

    const r = releaseFor(gh)[0]!;
    expect(r.draft, "an incomplete release was published").toBe(true);
    expect(finalPublish(gh)).toBeUndefined();
    // The leftover is kept: nothing is deleted before every file is on the release.
    expect(r.assets.map((a) => a.name)).toContain("stray.txt");
  });

  it("stops before publishing when a fresh draft misses a file", async () => {
    const missing = "SHA256SUMS.txt";
    const gh = new FakeGitHub({ tags: [TAG], dropUploads: [missing] });
    await expect(publishRelease(gh, options())).rejects.toThrow(missing);
    expect(releaseFor(gh)[0]!.draft).toBe(true);
    expect(finalPublish(gh)).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// AC-4V3DHP.6: regression — the v0.6.0 run (run 40413437391)
// ---------------------------------------------------------------------------

describe("v0.6.0 replay: the release list lags behind the create (AC-4V3DHP.6)", () => {
  it.each([1, 3, Number.POSITIVE_INFINITY])(
    "publishes the new draft when the list misses it for %s call(s)",
    async (lag) => {
      const gh = new FakeGitHub({ tags: [TAG], listLag: lag });
      await publishRelease(gh, options());

      const rs = releaseFor(gh);
      expect(rs).toHaveLength(1);
      expect(rs[0]!.draft).toBe(false);
      expect(rs[0]!.assets.map((a) => a.name).sort()).toEqual([...FILE_NAMES].sort());
      for (const c of gh.idCalls())
        expect(isValidId(c.args[0]), `${c.method}(${String(c.args[0])})`).toBe(true);
    },
  );

  it("old lookup (list again after create) would have found no id", async () => {
    // The pre-fix workflow step: list, create when missing, then list AGAIN to find the id,
    // and read the assets of whatever that second list returned.
    const oldLookup = async (gh: FakeGitHub): Promise<void> => {
      const ids = async () =>
        (await gh.listReleases()).filter((r) => r.tag_name === TAG).map((r) => r.id);
      let found = await ids();
      if (found.length === 0) {
        await gh.createDraftRelease({ tag_name: TAG, name: TAG, body: "" });
        found = await ids();
      }
      // `releases/$ids/assets` with an empty $ids: GitHub answers 404.
      await gh.listAssets(found[0] as number);
    };

    const old = new FakeGitHub({ tags: [TAG], listLag: 1 });
    await expect(oldLookup(old)).rejects.toThrow(/404/);
    const listed = old.calls.filter((c) => c.method === "listReleases");
    expect(listed, "the old flow listed before and after the create").toHaveLength(2);

    // The fixed script on the same lagging list takes the id from the create response.
    const fixed = new FakeGitHub({ tags: [TAG], listLag: 1 });
    await publishRelease(fixed, options());
    const created = fixed.calls.find((c) => c.method === "createDraftRelease");
    expect(created, "the fixed script created the draft").toBeDefined();
    expect(fixed.calls.filter((c) => c.method === "listAssets")[0]?.args[0]).toBe(40413437);
    expect(releaseFor(fixed)[0]!.draft).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// The real client over fetch (AC-4V3DHP.3): never touches the network here
// ---------------------------------------------------------------------------

type FetchArgs = [input: string | URL | Request, init?: RequestInit];

const json = (data: unknown, init: ResponseInit = {}): Response =>
  new Response(JSON.stringify(data), {
    status: 200,
    ...init,
    headers: { "content-type": "application/json", ...(init.headers ?? {}) },
  });

const urlOf = (args: FetchArgs): string => {
  const [input] = args;
  return typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
};

describe("GitHub client over fetch", () => {
  const make = (impl: (...args: FetchArgs) => Promise<Response>) => {
    const fetchFn = vi.fn(impl);
    const client = createGitHubClient({
      token: "test-token",
      repo: "nesquikm/open-powerz",
      fetch: fetchFn as unknown as typeof fetch,
    });
    return { client, fetchFn };
  };

  describe("refuses an invalid release id before any request (AC-4V3DHP.3)", () => {
    const bad: [string, unknown][] = [
      ["an empty string", ""],
      ["undefined", undefined],
      ["null", null],
      ["a non-numeric string", "abc"],
      ["zero", 0],
      ["a negative number", -1],
      ["a fraction", 2.5],
      ["NaN", Number.NaN],
    ];

    it.each(bad)("listAssets(%s)", async (_, id) => {
      const { client, fetchFn } = make(async () => json([]));
      await expect(client.listAssets(id as number)).rejects.toThrow(/id/i);
      expect(fetchFn).not.toHaveBeenCalled();
    });

    it.each(bad)("updateRelease(%s)", async (_, id) => {
      const { client, fetchFn } = make(async () => json({}));
      await expect(
        client.updateRelease(id as number, { tag_name: TAG, draft: false }),
      ).rejects.toThrow(/id/i);
      expect(fetchFn).not.toHaveBeenCalled();
    });

    it.each(bad)("uploadAsset(%s)", async (_, id) => {
      const { client, fetchFn } = make(async () => json({}));
      await expect(client.uploadAsset(id as number, "a.txt", files[0]!)).rejects.toThrow(/id/i);
      expect(fetchFn).not.toHaveBeenCalled();
    });

    it.each(bad)("deleteAsset(%s)", async (_, id) => {
      const { client, fetchFn } = make(async () => new Response(null, { status: 204 }));
      await expect(client.deleteAsset(id as number)).rejects.toThrow(/id/i);
      expect(fetchFn).not.toHaveBeenCalled();
    });
  });

  it("follows the Link next page when listing releases (drafts included)", async () => {
    const page2 = "https://api.github.com/repositories/1/releases?per_page=100&page=2";
    const { client, fetchFn } = make(async (...args) => {
      const url = urlOf(args);
      if (url === page2) return json([{ id: 2, tag_name: "v0.5.0", draft: false }]);
      return json([{ id: 1, tag_name: "v0.6.0", draft: true }], {
        headers: { link: `<${page2}>; rel="next", <${page2}>; rel="last"` },
      });
    });

    const releases = await client.listReleases();
    expect(releases.map((r) => [r.id, r.tag_name, r.draft])).toEqual([
      [1, "v0.6.0", true],
      [2, "v0.5.0", false],
    ]);
    expect(fetchFn).toHaveBeenCalledTimes(2);
    const first = urlOf(fetchFn.mock.calls[0]! as FetchArgs);
    expect(first.startsWith("https://api.github.com/repos/nesquikm/open-powerz/releases")).toBe(
      true,
    );
    expect(urlOf(fetchFn.mock.calls[1]! as FetchArgs)).toBe(page2);
  });

  it("follows the Link next page when listing assets", async () => {
    const page2 = "https://api.github.com/repositories/1/releases/7/assets?per_page=100&page=2";
    const { client, fetchFn } = make(async (...args) =>
      urlOf(args) === page2
        ? json([{ id: 12, name: "b.zip" }])
        : json([{ id: 11, name: "a.dmg" }], { headers: { link: `<${page2}>; rel="next"` } }),
    );

    const assets = await client.listAssets(7);
    expect(assets.map((a) => a.name)).toEqual(["a.dmg", "b.zip"]);
    expect(urlOf(fetchFn.mock.calls[0]! as FetchArgs)).toMatch(
      /^https:\/\/api\.github\.com\/repos\/nesquikm\/open-powerz\/releases\/7\/assets\b/,
    );
    expect(fetchFn).toHaveBeenCalledTimes(2);
  });

  it("sends the token on every request", async () => {
    const { client, fetchFn } = make(async () => json([]));
    await client.listReleases();
    const init = (fetchFn.mock.calls[0]! as FetchArgs)[1];
    const headers = new Headers(init?.headers);
    expect(headers.get("authorization")).toBe("Bearer test-token");
  });

  it("creates a draft through the releases API and returns the response with its id", async () => {
    const { client, fetchFn } = make(async () =>
      json({ id: 40413437, tag_name: TAG, draft: true }, { status: 201 }),
    );
    const created = await client.createDraftRelease({ tag_name: TAG, name: "t", body: "n" });
    expect(created.id).toBe(40413437);
    const [input, init] = fetchFn.mock.calls[0]! as FetchArgs;
    expect(urlOf([input])).toBe("https://api.github.com/repos/nesquikm/open-powerz/releases");
    expect(init?.method).toBe("POST");
    expect(JSON.parse(String(init?.body))).toMatchObject({
      tag_name: TAG,
      name: "t",
      body: "n",
      draft: true,
    });
  });

  it("publishes by id with a PATCH", async () => {
    const { client, fetchFn } = make(async () => json({ id: 42, tag_name: TAG, draft: false }));
    await client.updateRelease(42, {
      tag_name: TAG,
      draft: false,
      prerelease: false,
      make_latest: "true",
    });
    const [input, init] = fetchFn.mock.calls[0]! as FetchArgs;
    expect(urlOf([input])).toBe("https://api.github.com/repos/nesquikm/open-powerz/releases/42");
    expect(init?.method).toBe("PATCH");
    expect(JSON.parse(String(init?.body))).toEqual({
      tag_name: TAG,
      draft: false,
      prerelease: false,
      make_latest: "true",
    });
  });

  it("uploads to uploads.github.com under the release id and file name", async () => {
    const { client, fetchFn } = make(async () => json({ id: 1, name: "x" }, { status: 201 }));
    await client.uploadAsset(42, FILE_NAMES[0]!, files[0]!);
    const [input, init] = fetchFn.mock.calls[0]! as FetchArgs;
    const url = new URL(urlOf([input]));
    expect(url.origin).toBe("https://uploads.github.com");
    expect(url.pathname).toBe("/repos/nesquikm/open-powerz/releases/42/assets");
    expect(url.searchParams.get("name")).toBe(FILE_NAMES[0]);
    expect(init?.method).toBe("POST");
    expect(new Headers(init?.headers).get("content-type")).toBe("application/octet-stream");
    expect(Buffer.from(init?.body as Uint8Array)).toEqual(readFileSync(files[0]!));
  });

  it("deletes an asset by its id", async () => {
    const { client, fetchFn } = make(async () => new Response(null, { status: 204 }));
    await client.deleteAsset(9001);
    const [input, init] = fetchFn.mock.calls[0]! as FetchArgs;
    expect(urlOf([input])).toBe(
      "https://api.github.com/repos/nesquikm/open-powerz/releases/assets/9001",
    );
    expect(init?.method).toBe("DELETE");
  });

  it("checks the tag through the git refs API", async () => {
    const { client, fetchFn } = make(async (...args) =>
      urlOf(args).endsWith("/git/ref/tags/v0.6.0")
        ? json({ ref: "refs/tags/v0.6.0" })
        : new Response(JSON.stringify({ message: "Not Found" }), { status: 404 }),
    );
    expect(await client.tagExists("v0.6.0")).toBe(true);
    expect(await client.tagExists("v9.9.9")).toBe(false);
    expect(urlOf(fetchFn.mock.calls[0]! as FetchArgs)).toBe(
      "https://api.github.com/repos/nesquikm/open-powerz/git/ref/tags/v0.6.0",
    );
  });

  it("fails on an HTTP error with the status in the message", async () => {
    const { client } = make(
      async () => new Response(JSON.stringify({ message: "Not Found" }), { status: 404 }),
    );
    await expect(client.listAssets(42)).rejects.toThrow(/404/);
  });
});

// ===========================================================================
// FR 4PJRVN: release pipeline fixes
// ===========================================================================

const updates = (gh: FakeGitHub): Call[] => gh.calls.filter((c) => c.method === "updateRelease");

// ---------------------------------------------------------------------------
// AC-4PJRVN.1: every update sends the tag name; the published tag is checked
// ---------------------------------------------------------------------------

describe("tag name on every release update (AC-4PJRVN.1)", () => {
  it("sends the tag name with every update of a fresh draft, the final publish included", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    const us = updates(gh);
    expect(us.length, "the draft was never updated").toBeGreaterThan(0);
    for (const u of us)
      expect((u.args[1] as { tag_name?: string }).tag_name, "an update without tag_name").toBe(TAG);
  });

  it("sends the tag name with every update of an existing release", async () => {
    const gh = new FakeGitHub({
      tags: [TAG],
      releases: [{ tag_name: TAG, draft: true, name: "stale title", assets: FILE_NAMES }],
    });
    await publishRelease(gh, options());

    const us = updates(gh);
    expect(us.length).toBeGreaterThanOrEqual(2);
    for (const u of us) expect((u.args[1] as { tag_name?: string }).tag_name).toBe(TAG);
  });

  it("v0.6.0 replay: the published draft keeps its tag instead of becoming untagged-…", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    expect(gh.releases.map((r) => r.tag_name)).toEqual([TAG]);
    expect(gh.releases.some((r) => r.tag_name.startsWith("untagged-"))).toBe(false);
    const r = releaseFor(gh)[0]!;
    expect(r.draft).toBe(false);
  });

  it("fails, naming both tags, when the published release carries another tag", async () => {
    const gh = new FakeGitHub({ tags: [TAG], retagOnPublish: "untagged-5a1b2c" });
    const err = await publishRelease(gh, options()).then(
      () => undefined,
      (e: unknown) => e,
    );
    expect(err, "a release on the wrong tag passed the check").toBeInstanceOf(Error);
    expect((err as Error).message).toContain(TAG);
    expect((err as Error).message).toContain("untagged-5a1b2c");
    // The check runs after the publish, against what GitHub reports.
    expect(finalPublish(gh), "the check ran before publishing").toBeDefined();
  });

  it("fails on a mismatched tag for an existing release too", async () => {
    const gh = new FakeGitHub({
      tags: [TAG],
      releases: [{ tag_name: TAG, draft: true, assets: [] }],
      retagOnPublish: "v0.5.9",
    });
    await expect(publishRelease(gh, options())).rejects.toThrow(/v0\.5\.9/);
  });
});

// ---------------------------------------------------------------------------
// AC-4PJRVN.2: an older tag does not take the Latest badge
// ---------------------------------------------------------------------------

describe("the Latest badge (AC-4PJRVN.2)", () => {
  const notTrue = (gh: FakeGitHub) =>
    expect(
      (finalPublish(gh)?.args[1] as { make_latest?: string })?.make_latest,
      "an older tag asked for the Latest badge",
    ).not.toBe("true");

  it("a manual re-run of an older, already published tag leaves Latest on the newer release", async () => {
    const gh = new FakeGitHub({
      tags: [TAG, "v0.7.0"],
      releases: [
        { tag_name: "v0.7.0", draft: false },
        { tag_name: TAG, draft: false, assets: FILE_NAMES },
      ],
      latest: "v0.7.0",
    });
    await publishRelease(gh, options());

    expect(gh.latestTag()).toBe("v0.7.0");
    expect(releaseFor(gh)[0]!.draft).toBe(false);
    notTrue(gh);
  });

  it("a manual run for an older tag with no release yet does not take Latest", async () => {
    const gh = new FakeGitHub({
      tags: [TAG, "v0.7.0"],
      releases: [{ tag_name: "v0.7.0", draft: false }],
      latest: "v0.7.0",
    });
    await publishRelease(gh, options());

    expect(gh.latestTag()).toBe("v0.7.0");
    expect(releaseFor(gh)[0]!.draft).toBe(false);
    notTrue(gh);
  });

  it("compares versions by number, not as text (v0.9.0 is older than v0.10.0)", async () => {
    const tag = "v0.9.0";
    const gh = new FakeGitHub({
      tags: [tag, "v0.10.0"],
      releases: [{ tag_name: "v0.10.0", draft: false }],
      latest: "v0.10.0",
    });
    await publishRelease(gh, options({ tag, title: tag }));

    expect(gh.latestTag()).toBe("v0.10.0");
  });

  it("a newer tag takes Latest from an older release", async () => {
    const tag = "v0.10.0";
    const gh = new FakeGitHub({
      tags: [tag, "v0.9.0"],
      releases: [{ tag_name: "v0.9.0", draft: false }],
      latest: "v0.9.0",
    });
    await publishRelease(gh, options({ tag, title: tag }));

    expect(gh.latestTag()).toBe(tag);
  });

  it("a re-run of the current Latest keeps the badge", async () => {
    const gh = new FakeGitHub({
      tags: [TAG, "v0.5.0"],
      releases: [
        { tag_name: "v0.5.0", draft: false },
        { tag_name: TAG, draft: false, assets: FILE_NAMES },
      ],
      latest: TAG,
    });
    await publishRelease(gh, options());

    expect(gh.latestTag()).toBe(TAG);
  });

  it("a newer draft or pre-release does not stop a final release from taking Latest", async () => {
    const gh = new FakeGitHub({
      tags: [TAG, "v0.5.0", "v0.9.0", "v1.0.0-rc.1"],
      releases: [
        { tag_name: "v0.5.0", draft: false },
        { tag_name: "v0.9.0", draft: true },
        { tag_name: "v1.0.0-rc.1", draft: false, prerelease: true },
      ],
      latest: "v0.5.0",
    });
    await publishRelease(gh, options());

    expect(gh.latestTag()).toBe(TAG);
  });

  it("the first release of the repository takes Latest", async () => {
    const gh = new FakeGitHub({ tags: [TAG] });
    await publishRelease(gh, options());

    expect(gh.latestTag()).toBe(TAG);
  });
});

// ---------------------------------------------------------------------------
// AC-4PJRVN.3: edit, upload and delete-asset calls address the release by id
// ---------------------------------------------------------------------------

interface Req {
  method: string;
  url: URL;
  body: unknown;
}

const respond = (status: number, data?: unknown): Response =>
  data === undefined
    ? new Response(null, { status })
    : new Response(JSON.stringify(data), {
        status,
        headers: { "content-type": "application/json" },
      });

/**
 * A fetch that serves the GitHub REST routes the publish script may use from a
 * FakeGitHub's state, and records every request.
 */
function fakeGitHubFetch(gh: FakeGitHub, repo: string) {
  const requests: Req[] = [];
  const base = `/repos/${repo}`;
  const fetchFn = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input.href : input.url,
    );
    const method = (init?.method ?? "GET").toUpperCase();
    let body: unknown;
    if (typeof init?.body === "string") body = JSON.parse(init.body);
    requests.push({ method, url, body });
    const path = url.pathname;
    const route = (re: RegExp) => (path.startsWith(base) ? re.exec(path.slice(base.length)) : null);
    try {
      let m: RegExpExecArray | null;
      if (url.origin === "https://uploads.github.com") {
        m = route(/^\/releases\/(\d+)\/assets$/);
        if (method === "POST" && m) {
          await gh.uploadAsset(Number(m[1]), url.searchParams.get("name") ?? "", files[0]!);
          return respond(201, { id: 1, name: url.searchParams.get("name") });
        }
        return respond(404, { message: "Not Found" });
      }
      if (method === "GET" && (m = route(/^\/git\/ref\/tags\/(.+)$/)))
        return (await gh.tagExists(decodeURIComponent(m[1]!)))
          ? respond(200, { ref: `refs/tags/${m[1]}` })
          : respond(404, { message: "Not Found" });
      if (method === "GET" && route(/^\/releases$/)) return respond(200, await gh.listReleases());
      if (method === "POST" && route(/^\/releases$/))
        return respond(201, await gh.createDraftRelease(body as never));
      if (method === "GET" && route(/^\/releases\/latest$/))
        return respond(200, await gh.getLatestRelease());
      if (method === "GET" && (m = route(/^\/releases\/tags\/(.+)$/))) {
        const r = gh.releases.find((x) => x.tag_name === decodeURIComponent(m![1]!) && !x.draft);
        return r ? respond(200, { id: r.id, tag_name: r.tag_name }) : respond(404, {});
      }
      if (method === "GET" && (m = route(/^\/releases\/(\d+)$/)))
        return respond(200, await gh.getRelease(Number(m[1])));
      if (method === "PATCH" && (m = route(/^\/releases\/(\d+)$/)))
        return respond(200, await gh.updateRelease(Number(m[1]), body as never));
      if (method === "GET" && (m = route(/^\/releases\/(\d+)\/assets$/)))
        return respond(200, await gh.listAssets(Number(m[1])));
      if (method === "DELETE" && (m = route(/^\/releases\/assets\/(\d+)$/))) {
        await gh.deleteAsset(Number(m[1]));
        return respond(204);
      }
      return respond(404, { message: `no route for ${method} ${path}` });
    } catch (e) {
      return respond(/422/.test(String(e)) ? 422 : 404, { message: String(e) });
    }
  };
  return { fetchFn, requests };
}

describe("release addressed by id over the REST API (AC-4PJRVN.3)", () => {
  const REPO = "nesquikm/open-powerz";

  const run = async (gh: FakeGitHub) => {
    const { fetchFn, requests } = fakeGitHubFetch(gh, REPO);
    const client = createGitHubClient({
      token: "test-token",
      repo: REPO,
      fetch: fetchFn as unknown as typeof fetch,
    });
    await publishRelease(client, options());
    return requests;
  };

  const scenarios: [string, () => FakeGitHub][] = [
    ["a fresh tag", () => new FakeGitHub({ tags: [TAG] })],
    [
      "a re-run with leftover assets",
      () =>
        new FakeGitHub({
          tags: [TAG],
          releases: [{ tag_name: TAG, draft: true, assets: [...FILE_NAMES, "stray.txt"] }],
        }),
    ],
  ];

  it.each(scenarios)("edits, uploads and deletes by numeric id on %s", async (_, make) => {
    const gh = make();
    const requests = await run(gh);
    const id = releaseFor(gh)[0]!.id as number;

    const edits = requests.filter((r) => r.method === "PATCH");
    const uploads = requests.filter((r) => r.url.origin === "https://uploads.github.com");
    const deletes = requests.filter((r) => r.method === "DELETE");
    expect(edits.length).toBeGreaterThan(0);
    expect(uploads).toHaveLength(FILE_NAMES.length);
    for (const r of edits) expect(r.url.pathname).toBe(`/repos/${REPO}/releases/${id}`);
    for (const r of uploads) expect(r.url.pathname).toBe(`/repos/${REPO}/releases/${id}/assets`);
    for (const r of deletes)
      expect(r.url.pathname).toMatch(/^\/repos\/[^/]+\/[^/]+\/releases\/assets\/\d+$/);
    expect(releaseFor(gh)[0]!.draft).toBe(false);
  });

  it.each(scenarios)("never puts the tag in a release path on %s", async (_, make) => {
    const requests = await run(make());
    const releasePaths = requests.map((r) => r.url.pathname).filter((p) => p.includes("/releases"));
    for (const p of releasePaths) {
      expect(p, "a request looked the release up by tag").not.toMatch(/\/releases\/tags\//);
      expect(p).not.toContain(TAG);
    }
  });

  it("sends the tag name in every PATCH body (AC-4PJRVN.1 over the API)", async () => {
    const requests = await run(new FakeGitHub({ tags: [TAG] }));
    const edits = requests.filter((r) => r.method === "PATCH");
    expect(edits.length).toBeGreaterThan(0);
    for (const r of edits) expect((r.body as { tag_name?: string }).tag_name).toBe(TAG);
  });

  it("deletes only assets of the release it publishes", async () => {
    const gh = new FakeGitHub({
      tags: [TAG, "v0.5.0"],
      releases: [
        { tag_name: "v0.5.0", draft: false, assets: ["stray.txt", "old.dmg"] },
        { tag_name: TAG, draft: true, assets: ["stray.txt"] },
      ],
    });
    const otherIds = gh.releases.find((r) => r.tag_name === "v0.5.0")!.assets.map((a) => a.id);
    await publishRelease(gh, options());

    const deleted = gh.calls.filter((c) => c.method === "deleteAsset").map((c) => c.args[0]);
    expect(deleted.length).toBeGreaterThan(0);
    for (const d of deleted) expect(otherIds).not.toContain(d);
    expect(gh.releases.find((r) => r.tag_name === "v0.5.0")!.assets).toHaveLength(2);
  });
});
