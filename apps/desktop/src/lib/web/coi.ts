/**
 * FR W8SHXC: cross-origin isolation for the web edition. Protocol triggering needs
 * `SharedArrayBuffer`, which a page only gets when cross-origin isolated; GitHub Pages
 * cannot send the headers, so the web build registers our own service worker
 * (`public/coi-sw.js`) that adds them to the site's responses from the next load.
 */

/** The parts of `navigator.serviceWorker` the registration uses. */
interface ServiceWorkerContainerLike {
  register(url: string, options?: { scope?: string }): Promise<unknown>;
}

/** Whether this page is cross-origin isolated (has `SharedArrayBuffer`). */
export function pageIsolated(): boolean {
  return (globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated === true;
}

/** Register `coi-sw.js` under `base` unless the page is already isolated, and only in
 * the web build (`backend` is the build's `VITE_BACKEND`: `web`); the desktop app and
 * the mock build never register it. Never throws: without it, triggering a real meter
 * asks for a reload. */
export function registerCoiServiceWorker(
  base: string,
  backend: string | undefined,
  isolated: boolean = pageIsolated(),
  container: ServiceWorkerContainerLike | null = typeof navigator !== "undefined" &&
  "serviceWorker" in navigator
    ? navigator.serviceWorker
    : null,
): Promise<boolean> {
  if (backend !== "web" || isolated || !container) return Promise.resolve(false);
  return container.register(`${base}coi-sw.js`, { scope: base }).then(
    () => true,
    () => false,
  );
}
