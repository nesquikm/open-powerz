// AC-PBDEQE.6: the explore-a-session and record-a-csv pages describe the File tab's
// stats scope label, the Whole file / Shown range toggle, and what the live Stats panel
// counts ("Since connect or Reset").
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const page = (name: string): string =>
  readFileSync(join(repoRoot, `docs/how-to/${name}.md`), "utf8");

const pages = {
  "explore-a-session": page("explore-a-session"),
  "record-a-csv": page("record-a-csv"),
};

describe.each(Object.entries(pages))(
  "%s describes the stats scope (AC-PBDEQE.6)",
  (_name, text) => {
    it("names the Shown range scope", () => {
      expect(text).toContain("Shown range");
    });

    it("names the Whole file scope of the stats", () => {
      // Beside the stats or the toggle, not only the zoom's Whole file button.
      expect(text).toMatch(
        /Whole file[^\n]*(stats|Shown range)|(stats|Shown range)[^\n]*Whole file/i,
      );
    });

    it("says the scope follows the zoom", () => {
      expect(text).toMatch(/Shown range[^]{0,400}\bzoom/i);
    });

    it("says what the live Stats panel counts", () => {
      expect(text).toContain("Since connect or Reset");
    });
  },
);

describe("record-a-csv's Open a recording section (AC-PBDEQE.6)", () => {
  const section =
    /^## Open a recording\b[^]*?(?=^## |(?![^]))/m.exec(pages["record-a-csv"])?.[0] ?? "";

  it("describes the scope label and toggle where the File tab is described", () => {
    expect(section).toContain("Shown range");
    expect(section).toMatch(/Whole file/);
  });

  it("says a shown range integrates its rows, unlike the whole file's columns", () => {
    expect(section).toMatch(/Shown range[^]*\b(rows?)\b/i);
  });
});
