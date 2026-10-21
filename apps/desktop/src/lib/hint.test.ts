import { describe, expect, it } from "vitest";
import { splitHint } from "./hint";
import { LINUX_PERMISSION_DENIED } from "./mockBackend";

// AC-X2SPDW.8: a hint renders in the body font; the commands and file paths it
// quotes (in backticks) render in monospace.
describe("splitHint (AC-X2SPDW.8)", () => {
  it("keeps a hint without quotes as one prose part", () => {
    expect(splitHint("Replug the meter and try again.")).toEqual([
      { text: "Replug the meter and try again.", code: false },
    ]);
  });

  it("splits quoted commands and paths out as code parts, without the quotes", () => {
    expect(
      splitHint(
        "Unload it with `sudo modprobe -r powerz`, then connect again (see `docs/linux.md`).",
      ),
    ).toEqual([
      { text: "Unload it with ", code: false },
      { text: "sudo modprobe -r powerz", code: true },
      { text: ", then connect again (see ", code: false },
      { text: "docs/linux.md", code: true },
      { text: ").", code: false },
    ]);
  });

  it("leaves an unmatched backtick as plain text", () => {
    expect(splitHint("Press ` to open")).toEqual([{ text: "Press ` to open", code: false }]);
  });

  it("drops empty parts", () => {
    expect(splitHint("`sudo udevadm trigger`")).toEqual([
      { text: "sudo udevadm trigger", code: true },
    ]);
  });

  it("renders the Linux permission hint's commands and paths as code and the rest as prose", () => {
    const hint = LINUX_PERMISSION_DENIED.hint ?? "";
    const parts = splitHint(hint);
    const code = parts.filter((p) => p.code).map((p) => p.text);
    const prose = parts.filter((p) => !p.code).map((p) => p.text);
    expect(code).toEqual([
      "sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/",
      "sudo udevadm control --reload-rules",
      "sudo udevadm trigger",
      "docs/linux.md",
    ]);
    expect(prose.join("")).toContain("Install the udev rule");
    expect(prose.join("")).toContain("replug the meter");
    expect(parts.every((p) => !p.text.includes("`"))).toBe(true);
  });
});
