/**
 * Under a C/POSIX locale (`LANG=C`, common on Linux servers and CI) WebKitGTK
 * reports `navigator.language` as `"C"`, which `Intl` rejects with a
 * RangeError. uPlot builds an `Intl.NumberFormat(navigator.language)` when its
 * module loads, so that one bad tag blanked the whole window. `main.ts` imports
 * this module first, before anything that reads the language.
 */

/** The fallback when the reported language is not a tag `Intl` accepts. */
const FALLBACK_LOCALE = "en-US";

/** `tag` if `Intl` accepts it, otherwise the fallback. */
export function usableLocale(tag: string | undefined): string {
  if (!tag) return FALLBACK_LOCALE;
  try {
    new Intl.NumberFormat(tag);
    return tag;
  } catch {
    return FALLBACK_LOCALE;
  }
}

/** Give `nav` a usable `language` (and `languages`) when the reported one is rejected. */
export function guardNavigatorLanguage(nav: Navigator): void {
  const language = usableLocale(nav.language);
  if (language === nav.language) return;
  for (const [key, value] of [
    ["language", language],
    ["languages", Object.freeze([language])],
  ] as const) {
    try {
      Object.defineProperty(nav, key, { value, configurable: true });
    } catch {
      // A browser that refuses the override keeps its own value.
    }
  }
}

if (typeof navigator !== "undefined") guardNavigatorLanguage(navigator);
