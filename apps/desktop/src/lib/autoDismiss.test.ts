import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AutoDismiss, NOTICE_TIMEOUT_MS } from "./autoDismiss";

describe("AutoDismiss (AC-AKNVND.8)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("waits 15 s", () => {
    expect(NOTICE_TIMEOUT_MS).toBe(15_000);
  });

  it("dismisses after the timeout", () => {
    const done = vi.fn();
    const t = new AutoDismiss(done);
    t.start();
    vi.advanceTimersByTime(NOTICE_TIMEOUT_MS - 1);
    expect(done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("pauses while held (pointer over or focus inside) and restarts the full wait on release", () => {
    const done = vi.fn();
    const t = new AutoDismiss(done);
    t.start();
    vi.advanceTimersByTime(10_000);
    t.hold("pointer");
    vi.advanceTimersByTime(60_000);
    expect(done).not.toHaveBeenCalled();
    t.hold("focus");
    t.release("pointer");
    vi.advanceTimersByTime(60_000);
    expect(done).not.toHaveBeenCalled();
    t.release("focus");
    vi.advanceTimersByTime(NOTICE_TIMEOUT_MS - 1);
    expect(done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("a stopped timer never fires; a new start replaces the old one", () => {
    const done = vi.fn();
    const t = new AutoDismiss(done);
    t.start();
    vi.advanceTimersByTime(5_000);
    t.start();
    vi.advanceTimersByTime(NOTICE_TIMEOUT_MS - 1);
    expect(done).not.toHaveBeenCalled();
    t.stop();
    vi.advanceTimersByTime(60_000);
    expect(done).not.toHaveBeenCalled();
  });
});
