/**
 * open-powerz web edition (FR W8SHXC), MIT: makes the page cross-origin isolated.
 *
 * Protocol triggering runs the trigger engine in a worker that blocks on a
 * SharedArrayBuffer, which a page only gets when cross-origin isolated. GitHub Pages
 * cannot send the two headers that ask for it, so this service worker adds
 * `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy:
 * require-corp` to the site's own responses. It takes effect from the next load.
 *
 * Whatever goes wrong here, the request is never held: a failed fetch falls back to the
 * plain network fetch (its result unchanged), and a response that cannot be rebuilt is
 * passed on as it came.
 *
 * coi-sw version 2 (2026-11-09). Bump it with every change, so browsers install the new
 * worker.
 */

self.addEventListener("install", () => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

/** `response` with the cross-origin isolation headers, or itself when it cannot be
 * rebuilt (an opaque one, or any error). */
function isolated(response) {
  // An opaque response cannot be rebuilt.
  if (response.status === 0) return response;
  try {
    const headers = new Headers(response.headers);
    headers.set("Cross-Origin-Opener-Policy", "same-origin");
    headers.set("Cross-Origin-Embedder-Policy", "require-corp");
    headers.set("Cross-Origin-Resource-Policy", "same-origin");
    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers,
    });
  } catch {
    return response;
  }
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  // Only the site's own GETs: anything else goes to the network untouched.
  if (request.method !== "GET") return;
  // A request the browser may not make from here: leave it alone.
  if (request.cache === "only-if-cached" && request.mode !== "same-origin") return;
  if (new URL(request.url).origin !== self.location.origin) return;
  event.respondWith(
    fetch(request)
      .then(isolated)
      // Never hold the request: on any failure, the network's own answer, unchanged.
      .catch(() => fetch(event.request)),
  );
});
