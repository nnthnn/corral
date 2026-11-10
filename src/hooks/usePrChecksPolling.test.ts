import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as checks from "../lib/checks";
import { useStore } from "../store";
import type { CheckRun, Worktree } from "../types";
import { usePrChecksPolling } from "./usePrChecksPolling";

vi.mock("../lib/checks", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/checks")>("../lib/checks");
  return { ...actual, fetchPrChecks: vi.fn(), fetchPrDetails: vi.fn() };
});

const wt: Worktree = {
  project: "corral",
  feature: "hulling",
  slug: "hulling",
  branch: "feat/hulling",
  path: "/path",
  status: null,
  has_resumable_session: false,
  note: "",
};

const pendingCheck: CheckRun = {
  name: "build",
  workflow: "ci.yml",
  bucket: "pending",
  url: "https://x/1",
};
const passCheck: CheckRun = { ...pendingCheck, bucket: "pass" };

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(checks.fetchPrChecks).mockReset();
  vi.mocked(checks.fetchPrDetails).mockReset();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("usePrChecksPolling", () => {
  it("does nothing when there's no PR open", () => {
    renderHook(() => usePrChecksPolling(wt, false));
    expect(checks.fetchPrChecks).not.toHaveBeenCalled();
  });

  it("does not restart polling when the worktree object changes identity but not id", () => {
    // A store refresh (fs watcher, status poll, commit) rebuilds the worktree
    // object. That must not tear the effect down and refetch `gh` checks.
    const { rerender } = renderHook(
      ({ worktree }: { worktree: Worktree }) =>
        usePrChecksPolling(worktree, true),
      { initialProps: { worktree: wt } },
    );
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(1);

    rerender({ worktree: { ...wt } });
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(1);
  });

  it("fetches immediately once, then polls while checks are pending", () => {
    renderHook(() => usePrChecksPolling(wt, true));
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(1);

    useStore.setState({
      checksByWtId: new Map([["corral/hulling", [pendingCheck]]]),
    });
    act(() => vi.advanceTimersByTime(12_000));
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(2);
  });

  it("retries PR details while they're missing, then stops once loaded", () => {
    renderHook(() => usePrChecksPolling(wt, true));
    expect(checks.fetchPrDetails).toHaveBeenCalledTimes(1);

    useStore.setState({
      checksByWtId: new Map([["corral/hulling", [pendingCheck]]]),
    });
    act(() => vi.advanceTimersByTime(12_000));
    // No details cached yet, so the poll retried them too.
    expect(checks.fetchPrDetails).toHaveBeenCalledTimes(2);

    useStore.setState({
      prDetailsByWtId: new Map([
        ["corral/hulling", { title: "t", body: "b", comments: [] }],
      ]),
    });
    act(() => vi.advanceTimersByTime(12_000));
    // Details are now loaded, so the poll stops refreshing them.
    expect(checks.fetchPrDetails).toHaveBeenCalledTimes(2);
  });

  it("stops polling once every check is resolved", () => {
    renderHook(() => usePrChecksPolling(wt, true));
    useStore.setState({
      checksByWtId: new Map([["corral/hulling", [passCheck]]]),
    });
    act(() => vi.advanceTimersByTime(60_000));
    // Only the one immediate fetch from mount — the poll saw an already-
    // resolved check and cleared itself instead of fetching again, even
    // though 60s is still well within the empty-list grace window (the
    // grace window doesn't apply here — the list isn't empty).
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(1);
  });

  it("keeps polling on an empty check list while within the grace window", () => {
    renderHook(() => usePrChecksPolling(wt, true));
    // checksByWtId has no entry for this wt at all — an empty/missing list.
    act(() => vi.advanceTimersByTime(12_000));
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(2);
    act(() => vi.advanceTimersByTime(12_000));
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(3);
    act(() => vi.advanceTimersByTime(12_000));
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(4);
  });

  it("stops polling on an empty check list once the grace window elapses", () => {
    renderHook(() => usePrChecksPolling(wt, true));
    // Advance to just past the 120s grace window (10 poll ticks of 12s).
    act(() => vi.advanceTimersByTime(120_000));
    const callsAtGraceEnd = vi.mocked(checks.fetchPrChecks).mock.calls.length;
    // The tick at exactly 120_000ms is still >= EMPTY_GRACE_MS since startedAt,
    // so it should have stopped by now. Advancing further should not fetch again.
    act(() => vi.advanceTimersByTime(24_000));
    expect(checks.fetchPrChecks).toHaveBeenCalledTimes(callsAtGraceEnd);
  });
});
