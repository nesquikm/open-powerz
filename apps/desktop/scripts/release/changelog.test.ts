// FR E1ZJ2N: the release notes are the CHANGELOG section for the tagged version
// (AC-E1ZJ2N.5), and the release title is the version plus its codename.
import { describe, expect, it } from "vitest";
import { extractRelease, releaseTitle } from "./changelog";

const CHANGELOG = `# Changelog

All notable changes to open-powerz are documented here.

## [0.6.0-rc.1] — 2026-10-27 — "Special Delivery"

### Added
- Release candidate notes

## [0.6.0] — 2026-10-28 — "Special Delivery"

### Added
- Release builds on a version tag (E1ZJ2N)

### Fixed
- Something small

## [0.5.0] — 2026-10-26 — "Flash Gordon"

### Added
- Firmware update (V9SK52)

## [0.4.1] — 2026-10-25

### Fixed
- A heading without a codename

## [0.1.0] — 2026-10-20 — "Wire Whisperer"

### Added
- First release

Total test count at release: 10 tests.
`;

describe("extractRelease (AC-E1ZJ2N.5)", () => {
  it("extracts a section in the middle of the file up to the next heading", () => {
    const r = extractRelease(CHANGELOG, "0.5.0");
    expect(r).toEqual({
      version: "0.5.0",
      date: "2026-10-26",
      codename: "Flash Gordon",
      notes: "### Added\n- Firmware update (V9SK52)",
    });
  });

  it("keeps every subsection of the version's body and trims it", () => {
    const r = extractRelease(CHANGELOG, "0.6.0");
    expect(r?.notes).toBe(
      "### Added\n- Release builds on a version tag (E1ZJ2N)\n\n### Fixed\n- Something small",
    );
    expect(r?.codename).toBe("Special Delivery");
    expect(r?.date).toBe("2026-10-28");
  });

  it("matches a pre-release heading only for the pre-release version", () => {
    const rc = extractRelease(CHANGELOG, "0.6.0-rc.1");
    expect(rc?.version).toBe("0.6.0-rc.1");
    expect(rc?.notes).toBe("### Added\n- Release candidate notes");
    expect(rc?.date).toBe("2026-10-27");
    // The final version must not pick up the rc heading, nor the rc the final one.
    expect(extractRelease(CHANGELOG, "0.6.0")?.notes).not.toContain("Release candidate");
    expect(extractRelease(CHANGELOG, "0.6.0-rc.2")).toBeNull();
  });

  it("does not match the final version against a file that only has its pre-release", () => {
    const onlyRc = `# Changelog\n\n## [1.0.0-rc.1] — 2026-11-01 — "Almost"\n\n- rc\n`;
    expect(extractRelease(onlyRc, "1.0.0")).toBeNull();
    expect(extractRelease(onlyRc, "1.0.0-rc.1")?.notes).toBe("- rc");
  });

  it("returns null for a version with no section", () => {
    expect(extractRelease(CHANGELOG, "9.9.9")).toBeNull();
    expect(extractRelease(CHANGELOG, "0.5")).toBeNull();
  });

  it("extracts the last section in the file, which has no following heading", () => {
    const r = extractRelease(CHANGELOG, "0.1.0");
    expect(r?.codename).toBe("Wire Whisperer");
    expect(r?.notes).toBe("### Added\n- First release\n\nTotal test count at release: 10 tests.");
  });

  it("reads a heading without a codename", () => {
    const r = extractRelease(CHANGELOG, "0.4.1");
    expect(r).not.toBeNull();
    expect(r?.version).toBe("0.4.1");
    expect(r?.date).toBe("2026-10-25");
    expect(r?.codename).toBeUndefined();
    expect(r?.notes).toBe("### Fixed\n- A heading without a codename");
  });

  it("reads a heading with neither date nor codename", () => {
    const text = `## [2.0.0]\n\n- bare\n\n## [1.0.0] — 2026-01-01\n\n- old\n`;
    const r = extractRelease(text, "2.0.0");
    expect(r?.date).toBeUndefined();
    expect(r?.codename).toBeUndefined();
    expect(r?.notes).toBe("- bare");
  });

  it("handles CRLF line endings", () => {
    const r = extractRelease(CHANGELOG.replace(/\n/g, "\r\n"), "0.5.0");
    expect(r?.codename).toBe("Flash Gordon");
    expect(r?.notes.replace(/\r\n/g, "\n")).toBe("### Added\n- Firmware update (V9SK52)");
  });
});

describe("extractRelease edge cases", () => {
  it("returns an empty body for a heading with no notes", () => {
    const text = "## [0.6.0] — 2026-10-28\n\n## [0.5.0] — 2026-10-26\n\n- old\n";
    expect(extractRelease(text, "0.6.0")?.notes).toBe("");
  });

  it("stops the last section at the link references", () => {
    const text = [
      "## [0.6.0] — 2026-10-28",
      "",
      "- new",
      "",
      "[0.6.0]: https://github.com/nesquikm/open-powerz/releases/tag/v0.6.0",
      "[0.5.0]: https://github.com/nesquikm/open-powerz/releases/tag/v0.5.0",
      "",
    ].join("\n");
    expect(extractRelease(text, "0.6.0")?.notes).toBe("- new");
  });

  it("takes the first of two headings for the same version", () => {
    const text = "## [0.6.0] — 2026-10-28\n\n- first\n\n## [0.6.0] — 2026-10-29\n\n- second\n";
    expect(extractRelease(text, "0.6.0")).toEqual({
      version: "0.6.0",
      date: "2026-10-28",
      notes: "- first",
    });
  });

  it("reads an unquoted codename", () => {
    const section = extractRelease(
      "## [0.6.0] — 2026-10-28 — Special Delivery\n\n- new\n",
      "0.6.0",
    );
    expect(section?.codename).toBe("Special Delivery");
    expect(section?.notes).toBe("- new");
  });
});

describe("releaseTitle (AC-E1ZJ2N.5)", () => {
  it("joins the tag and the quoted codename with an em dash", () => {
    expect(releaseTitle("v0.6.0", "Special Delivery")).toBe('v0.6.0 — "Special Delivery"');
  });

  it("is just the tag without a codename", () => {
    expect(releaseTitle("v0.4.1")).toBe("v0.4.1");
    expect(releaseTitle("v1.0.0-rc.1", undefined)).toBe("v1.0.0-rc.1");
  });
});
