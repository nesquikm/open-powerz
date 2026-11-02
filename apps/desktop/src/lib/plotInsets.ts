/**
 * Plots stacked on one time range line up (FR AXHK2T, AC-AXHK2T.6): each sizes its
 * value axes from its own tick labels and shows its own series, so on its own each plot
 * area would start and end at a different x. `SharedInsets` pads every attached plot so
 * its plot area's left and right insets match the widest of them, and the same ms lands
 * at the same x in all.
 *
 * The padding is a plain number per plot and side, changed only by {@link align} after
 * the plots are laid out: uPlot's own sizing passes never see it move, so they settle as
 * they would without it.
 */

import type uPlot from "uplot";

interface Member {
  plot: uPlot | null;
  /** Extra padding, CSS px, left and right. */
  extra: [number, number];
  relayout: () => void;
}

/** A plot's laid-out insets, CSS px: its plot area's distance from its left and right. */
function insetsOf(u: uPlot): [number, number] | null {
  const left = parseFloat(u.over.style.left);
  const width = parseFloat(u.over.style.width);
  if (!Number.isFinite(left) || !Number.isFinite(width) || !(u.width > 0)) return null;
  return [left, u.width - left - width];
}

export class SharedInsets {
  #members = new Map<string, Member>();

  /** Plot `id`'s padding; `relayout` lays it out again after {@link align} changes it.
   * Hand the plot to `bind` once it is built. */
  attach(
    id: string,
    relayout: () => void,
  ): { padding: uPlot.Padding; bind: (u: uPlot) => void; detach: () => void } {
    const m: Member = { plot: null, extra: [0, 0], relayout };
    this.#members.set(id, m);
    // uPlot's own margin stays (null top and bottom); the sides get the extra on top of
    // it, through a function so uPlot reads the current value on every layout.
    const side = (which: 0 | 1) => (): number => m.extra[which];
    return {
      padding: [null, side(1), null, side(0)],
      bind: (u) => {
        m.plot = u;
      },
      detach: () => {
        if (this.#members.get(id) === m) this.#members.delete(id);
        // The others may now be wider than they need to be.
        this.align();
      },
    };
  }

  /** Matches every attached plot's insets to the widest, laying out again the plots
   * whose padding changes. Cheap when nothing changes; call it after each draw. */
  align(): void {
    const natural = new Map<Member, [number, number]>();
    const target: [number, number] = [0, 0];
    for (const m of this.#members.values()) {
      const at = m.plot === null ? null : insetsOf(m.plot);
      if (at === null) continue;
      const own: [number, number] = [at[0] - m.extra[0], at[1] - m.extra[1]];
      natural.set(m, own);
      target[0] = Math.max(target[0], own[0]);
      target[1] = Math.max(target[1], own[1]);
    }
    for (const [m, own] of natural) {
      const next: [number, number] = [target[0] - own[0], target[1] - own[1]];
      if (Math.abs(next[0] - m.extra[0]) < 0.5 && Math.abs(next[1] - m.extra[1]) < 0.5) continue;
      m.extra = next;
      m.relayout();
    }
  }
}
