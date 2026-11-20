// FR 31HJ41: the Linux permission fix reaches release users. The .deb installs the
// shipped udev rule (AC-31HJ41.3); the Linux guide and the install guide lead with the
// one-line command and keep the copy-from-checkout route for source builds (AC-31HJ41.4).
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(here, "../../../..");
const SRC_TAURI = join(REPO_ROOT, "apps/desktop/src-tauri");
const RULE_FILE = join(REPO_ROOT, "packaging/linux/60-powerz.rules");
const read = (p: string): string => (existsSync(p) ? readFileSync(p, "utf8") : "");

const TEE = "| sudo tee /etc/udev/rules.d/60-powerz.rules";
const CP = "sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/";

/** The `echo '<rule>' | sudo tee …` command lines in a Markdown text. */
const teeLines = (md: string): string[] =>
  md
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.includes(TEE) && /^echo '[^']+' \|/.test(l));

/** The first Markdown section (`## …`) whose heading matches `heading`. */
const section = (md: string, heading: RegExp): string => {
  const parts = md.split(/^(?=## )/m);
  return parts.find((p) => heading.test(p.split("\n")[0] ?? "")) ?? "";
};

/** Paragraphs (blank-line separated) of a Markdown text. */
const paragraphs = (md: string): string[] => md.split(/\n\s*\n/);

describe("the .deb installs the udev rule (AC-31HJ41.3)", () => {
  type Conf = { bundle?: { linux?: { deb?: { files?: Record<string, string> } } } };
  const conf = JSON.parse(read(join(SRC_TAURI, "tauri.conf.json")) || "{}") as Conf;

  it("maps /usr/lib/udev/rules.d/60-powerz.rules to the shipped rule file", () => {
    const files = conf.bundle?.linux?.deb?.files ?? {};
    const source = files["/usr/lib/udev/rules.d/60-powerz.rules"];
    expect(source, "no bundle.linux.deb.files entry for the udev rule").toBeTypeOf("string");
    expect(resolve(SRC_TAURI, source ?? "")).toBe(RULE_FILE);
    expect(existsSync(RULE_FILE)).toBe(true);
  });
});

describe("docs/linux.md for release users (AC-31HJ41.4)", () => {
  const doc = read(join(REPO_ROOT, "docs/linux.md"));
  const install = section(doc, /udev rule/i);

  it("gives the one-line tee command for vendor 5fc9 in the udev section", () => {
    const lines = teeLines(install);
    expect(lines, "no `echo '<rule>' | sudo tee` line in the udev section").toHaveLength(1);
    expect(lines[0]).toContain('ATTRS{idVendor}=="5fc9"');
    expect(lines[0]).toContain('TAG+="uaccess"');
    expect(lines[0]).toContain('ENV{ID_MM_DEVICE_IGNORE}="1"');
  });

  it("leads with the one-line command, before the copy-from-checkout route", () => {
    const tee = install.indexOf(TEE);
    const cp = install.indexOf(CP);
    expect(tee, "no tee command").toBeGreaterThan(-1);
    expect(cp, "no copy-from-checkout command").toBeGreaterThan(-1);
    expect(tee).toBeLessThan(cp);
  });

  it("keeps the copy-from-checkout route for source builds", () => {
    const p = paragraphs(install).findIndex((x) => x.includes(CP));
    const context = paragraphs(install)
      .slice(Math.max(0, p - 2), p + 1)
      .join("\n");
    expect(p).toBeGreaterThan(-1);
    expect(context).toMatch(/source|checkout|clone/i);
  });

  it("notes that the .deb installs the rule", () => {
    const deb = paragraphs(doc).filter((x) => /\.deb\b/.test(x));
    expect(deb.some((x) => /install/i.test(x) && /rule/i.test(x))).toBe(true);
  });
});

describe("docs/how-to/install-a-release.md matches (AC-31HJ41.4)", () => {
  const guide = read(join(REPO_ROOT, "docs/how-to/install-a-release.md"));
  const linux = section(guide, /^## Linux/);

  it("gives the same one-line tee command in its Linux section", () => {
    const lines = teeLines(linux);
    expect(lines, "no `echo '<rule>' | sudo tee` line in the Linux section").toHaveLength(1);
    const docLines = teeLines(read(join(REPO_ROOT, "docs/linux.md")));
    expect(docLines).toContain(lines[0]);
  });

  it("notes that the .deb installs the rule", () => {
    const deb = paragraphs(linux).filter((x) => /\.deb\b/.test(x));
    expect(deb.some((x) => /install/i.test(x) && /rule/i.test(x))).toBe(true);
  });

  it("does not send release users to the checkout path", () => {
    expect(linux).not.toContain(CP);
  });
});

describe("the guides' one-line rule is the code's rule (31HJ41 review)", () => {
  const transport = read(join(REPO_ROOT, "crates/powerz-protocol/src/transport/mod.rs"));
  const rule = /macro_rules! linux_udev_rule \{\s*\(\) => \{\s*r#"([\s\S]*?)"#/.exec(
    transport,
  )?.[1];

  it("finds the rule in the protocol crate", () => {
    expect(rule).toBeTypeOf("string");
  });

  for (const guide of ["docs/linux.md", "docs/how-to/install-a-release.md"]) {
    it(`${guide} writes exactly LINUX_UDEV_RULE`, () => {
      const text = read(join(REPO_ROOT, guide));
      expect(text).toContain(`echo '${rule}' ${TEE}`);
    });
  }
});
