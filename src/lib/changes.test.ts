import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";
import { loadChanges, scheduleChangesRefresh } from "./changes";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

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

const other: Worktree = { ...wt, feature: "other" };

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.useFakeTimers();
});

describe("loadChanges", () => {
  it("stores branch_changes on success", async () => {
    useStore.setState({ selectedId: wtId(wt) });
    vi.mocked(invoke).mockResolvedValueOnce({ base: "main", files: [] });
    await loadChanges(wt);
    expect(useStore.getState().changes).toEqual({ base: "main", files: [] });
  });

  it("falls back to an empty diff on error", async () => {
    useStore.setState({ selectedId: wtId(wt) });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("nope"));
    await loadChanges(wt);
    expect(useStore.getState().changes).toEqual({ base: null, files: [] });
  });

  it("ignores a stale response after the selected worktree changed", async () => {
    useStore.setState({ selectedId: wtId(wt) });
    vi.mocked(invoke).mockResolvedValueOnce({ base: "main", files: [] });
    const promise = loadChanges(wt);
    useStore.setState({ selectedId: wtId(other) });
    await promise;
    expect(useStore.getState().changes).toEqual(
      useStore.getInitialState().changes,
    );
  });
});

describe("scheduleChangesRefresh", () => {
  it("reloads changes for the current worktree after a delay", () => {
    vi.mocked(invoke).mockResolvedValue({ base: null, files: [] });
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    scheduleChangesRefresh();
    vi.advanceTimersByTime(1600);
    expect(invoke).toHaveBeenCalledWith("branch_changes", {
      project: "corral",
      feature: "hulling",
    });
  });
});
