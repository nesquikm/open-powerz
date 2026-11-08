/**
 * FR QFTCX1, AC-QFTCX1.3: the "not supported" view names Chrome and Edge, links the
 * desktop downloads and still offers the demo meter.
 *
 * Rendered with `svelte/server` (no DOM needed).
 */

import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import NotSupported from "../components/NotSupported.svelte";
import { RELEASES_URL } from "./features";

const body = (): string => render(NotSupported as never, { props: {} as never }).body;
const text = (): string =>
  body()
    .replace(/<[^>]+>/g, " ")
    .replace(/\s+/g, " ");

describe("NotSupported (AC-QFTCX1.3)", () => {
  it("has a heading saying the browser is not supported", () => {
    expect(body()).toMatch(/<h[1-6][^>]*>[^<]*not supported/i);
  });

  it("names Chrome and Edge as the browsers to use", () => {
    expect(text()).toMatch(/\bChrome\b/);
    expect(text()).toMatch(/\bEdge\b/);
  });

  it("links the desktop app downloads", () => {
    expect(body()).toContain(`href="${RELEASES_URL}"`);
    expect(text()).toMatch(/desktop app/i);
  });

  it("says the demo meter still works here", () => {
    expect(text()).toMatch(/demo meter/i);
  });
});
