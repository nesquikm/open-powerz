import { describe, expect, it } from "vitest";
import { guardNavigatorLanguage, usableLocale } from "./localeGuard";

describe("locale guard (a C/POSIX locale must not blank the app)", () => {
  it("keeps a valid language tag", () => {
    expect(usableLocale("de-DE")).toBe("de-DE");
  });

  it("replaces tags Intl rejects, such as WebKitGTK's 'C' under LANG=C", () => {
    expect(usableLocale("C")).toBe("en-US");
    expect(usableLocale("")).toBe("en-US");
    expect(usableLocale(undefined)).toBe("en-US");
  });

  it("patches a navigator whose language Intl rejects, so libraries reading it at import keep working", () => {
    const nav = { language: "C", languages: ["C"] } as unknown as Navigator;
    guardNavigatorLanguage(nav);
    expect(nav.language).toBe("en-US");
    expect(() => new Intl.NumberFormat(nav.language)).not.toThrow();
  });

  it("leaves a navigator with a valid language alone", () => {
    const nav = { language: "fr-FR" } as unknown as Navigator;
    guardNavigatorLanguage(nav);
    expect(nav.language).toBe("fr-FR");
  });
});
