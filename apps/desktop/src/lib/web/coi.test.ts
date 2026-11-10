/**
 * FR W8SHXC: cross-origin isolation for the web edition.
 *
 * - `registerCoiServiceWorker` registers `coi-sw.js` only in the web build (`VITE_BACKEND`
 *   is `web`), only when the page is not isolated yet, and never throws.
 * - `public/coi-sw.js` adds the isolation headers to the site's own GET responses and
 *   never holds a request: a failed fetch falls back to the plain network fetch, whose
 *   result goes through unchanged; other origins and other methods are left alone.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import { registerCoiServiceWorker } from "./coi";

describe("registerCoiServiceWorker", () => {
  const container = () => ({ register: vi.fn(() => Promise.resolve({})) });

  it("registers coi-sw.js under the base in the web build", async () => {
    const c = container();
    expect(await registerCoiServiceWorker("/open-powerz/", "web", false, c)).toBe(true);
    expect(c.register).toHaveBeenCalledWith("/open-powerz/coi-sw.js", { scope: "/open-powerz/" });
  });

  it("never registers outside the web build", async () => {
    for (const backend of [undefined, "", "mock", "tauri", "desktop"]) {
      const c = container();
      expect(await registerCoiServiceWorker("/", backend, false, c)).toBe(false);
      expect(c.register).not.toHaveBeenCalled();
    }
  });

  it("skips an isolated page and a browser without service workers", async () => {
    const c = container();
    expect(await registerCoiServiceWorker("/", "web", true, c)).toBe(false);
    expect(c.register).not.toHaveBeenCalled();
    expect(await registerCoiServiceWorker("/", "web", false, null)).toBe(false);
  });

  it("a failed registration is false, not a throw", async () => {
    const c = { register: vi.fn(() => Promise.reject(new Error("blocked"))) };
    expect(await registerCoiServiceWorker("/", "web", false, c)).toBe(false);
  });
});

// ---- public/coi-sw.js ----

const ORIGIN = "https://example.github.io";
const SW_SOURCE = readFileSync(
  fileURLToPath(new URL("../../../public/coi-sw.js", import.meta.url)),
  "utf8",
);

type FetchListener = (event: FetchEventLike) => void;

interface FetchEventLike {
  request: { url: string; method: string; cache: string; mode: string };
  respondWith(response: Promise<Response>): void;
}

/** Run coi-sw.js against a fake service worker scope with `fetch` as the network. */
function loadServiceWorker(fetch: (request: unknown) => Promise<Response>): FetchListener {
  const listeners: Record<string, (e: unknown) => void> = {};
  const self = {
    location: { origin: ORIGIN },
    skipWaiting: () => {},
    clients: { claim: () => Promise.resolve() },
    addEventListener: (type: string, fn: (e: unknown) => void) => {
      listeners[type] = fn;
    },
  };
  new Function("self", "fetch", "Headers", "Response", SW_SOURCE)(self, fetch, Headers, Response);
  const onFetch = listeners.fetch;
  if (!onFetch) throw new Error("coi-sw.js has no fetch listener");
  return onFetch as FetchListener;
}

function request(path: string, method = "GET", origin = ORIGIN) {
  return { url: `${origin}${path}`, method, cache: "default", mode: "navigate" };
}

/** Dispatch `req`; the response the worker answers with, or `null` when it left it. */
async function dispatch(onFetch: FetchListener, req: FetchEventLike["request"]) {
  let answered: Promise<Response> | null = null;
  onFetch({ request: req, respondWith: (r) => (answered = r) });
  return answered === null ? null : await (answered as Promise<Response>);
}

describe("coi-sw.js", () => {
  it("carries a version comment", () => {
    expect(SW_SOURCE).toMatch(/coi-sw version \d+/);
  });

  it("adds the isolation headers to the site's own responses", async () => {
    const fetch = vi.fn(() => Promise.resolve(new Response("<html>", { status: 200 })));
    const response = await dispatch(loadServiceWorker(fetch), request("/open-powerz/"));
    expect(response?.headers.get("Cross-Origin-Opener-Policy")).toBe("same-origin");
    expect(response?.headers.get("Cross-Origin-Embedder-Policy")).toBe("require-corp");
    expect(await response?.text()).toBe("<html>");
  });

  it("a failed fetch falls back to the network's result, unchanged", async () => {
    const plain = new Response("from the network", { status: 200 });
    const fetch = vi
      .fn<(r: unknown) => Promise<Response>>()
      .mockRejectedValueOnce(new TypeError("Failed to fetch"))
      .mockResolvedValueOnce(plain);
    const response = await dispatch(loadServiceWorker(fetch), request("/open-powerz/app.js"));
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(response).toBe(plain);
    expect(response?.headers.get("Cross-Origin-Embedder-Policy")).toBeNull();
  });

  it("a response it cannot rebuild goes through as it came", async () => {
    // A status a `Response` cannot be built with: the rebuild throws.
    const odd = { status: 101, statusText: "", headers: new Headers(), body: null };
    const fetch = vi.fn(() => Promise.resolve(odd as unknown as Response));
    const response = await dispatch(loadServiceWorker(fetch), request("/socket"));
    expect(response).toBe(odd);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("a network that fails twice gives the browser the network error, not a hang", async () => {
    const fetch = vi.fn(() => Promise.reject(new TypeError("offline")));
    await expect(dispatch(loadServiceWorker(fetch), request("/x"))).rejects.toThrow("offline");
  });

  it("leaves other origins and other methods to the browser", async () => {
    const fetch = vi.fn(() => Promise.resolve(new Response("")));
    const onFetch = loadServiceWorker(fetch);
    expect(await dispatch(onFetch, request("/x", "GET", "https://cdn.example.com"))).toBeNull();
    expect(await dispatch(onFetch, request("/x", "POST"))).toBeNull();
    expect(fetch).not.toHaveBeenCalled();
  });
});
