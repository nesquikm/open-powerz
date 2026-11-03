/**
 * FR C6VR9K, AC .5: the PD marker layer's per-frame loop is capped.
 *
 * API pinned here: `clusterMarkers(markers, toPx, cap?)` makes at most `cap` clusters
 * and stops there: it maps no marker past the one that would start cluster `cap + 1`.
 * Without `cap` it clusters every marker, as before. The chart passes
 * `drawLoopCap(plot width, lane height)` (`./drawCap`).
 */

import { describe, expect, it } from "vitest";
import { clusterMarkers, type PdMarker } from "./pdTimeline";

function markers(n: number): PdMarker[] {
  return Array.from({ length: n }, (_, i) => ({ index: i, ms: i * 1000, kind: "other" }));
}

describe("clusterMarkers' cap (AC-C6VR9K.5)", () => {
  it("AC-C6VR9K.5: makes at most `cap` clusters and maps no marker past them", () => {
    // 10 000 markers 100 px apart: one cluster each without a cap.
    const all = markers(10_000);
    let mapped = 0;
    const toPx = (ms: number): number => {
      mapped++;
      return ms / 10;
    };
    const clusters = clusterMarkers(all, toPx, 50);
    expect(clusters).toHaveLength(50);
    expect(clusters.map((c) => c.markers[0]!.index)).toEqual(
      Array.from({ length: 50 }, (_, i) => i),
    );
    expect(mapped).toBeLessThanOrEqual(51);
  });

  it("AC-C6VR9K.5: markers merged into the last cluster still join it", () => {
    // Pairs 1 px apart, pairs 100 px apart: each cluster holds two markers.
    const all: PdMarker[] = Array.from({ length: 200 }, (_, i) => ({
      index: i,
      ms: Math.floor(i / 2) * 100 + (i % 2),
      kind: "other",
    }));
    const clusters = clusterMarkers(all, (ms) => ms, 10);
    expect(clusters).toHaveLength(10);
    for (const c of clusters) expect(c.count).toBe(2);
  });

  it("without a cap, clusters every marker as before", () => {
    expect(clusterMarkers(markers(300), (ms) => ms / 10)).toHaveLength(300);
  });
});
