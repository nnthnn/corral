import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { GitStatus, Worktree } from "../types";
import { initWorktreeWatch } from "./worktree-watch-events";

const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));

vi.mock("./changes", () => ({ loadChanges: vi.fn() }));
vi.mock("./tauri", () => ({ invoke: vi.fn() }));

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

const status: GitStatus = {
  branch: "feat/hulling",
  ahead: 3,
  behind: 0,
  staged: 0,
  modified: 0,
  untracked: 0,
  added: 0,
  removed: 0,
  conflicts: 0,
  commits_ahead_of_base: 3,
  merged_into_base: false,
  conflicts_with_base: false,
  has_remote: true,
  last_commit_message: "",
  last_commit_time: 0,
  base_added: 0,
  base_removed: 0,
};

beforeEach(async () => {
  useStore.setState(useStore.getInitialState());
  listeners.clear();
  const { loadChanges } = await import("./changes");
  vi.mocked(loadChanges).mockReset();
  const { invoke } = await import("./tauri");
  vi.mocked(invoke).mockReset().mockResolvedValue(status);
});

describe("initWorktreeWatch", () => {
  it("refreshes changes when the event matches the selected worktree", async () => {
    const { loadChanges } = await import("./changes");
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    await initWorktreeWatch();

    listeners.get("worktree-changed")?.({
      payload: { project: "corral", feature: "hulling", git_changed: false },
    });

    expect(loadChanges).toHaveBeenCalledWith(wt);
  });

  it("ignores events for a worktree that isn't selected", async () => {
    const { loadChanges } = await import("./changes");
    useStore.setState({ worktrees: [wt], selectedId: "corral/other" });
    await initWorktreeWatch();

    listeners.get("worktree-changed")?.({
      payload: { project: "corral", feature: "hulling", git_changed: true },
    });

    expect(loadChanges).not.toHaveBeenCalled();
  });

  it("refreshes the worktree's git status when the event matches the selected worktree", async () => {
    const { invoke } = await import("./tauri");
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    await initWorktreeWatch();

    listeners.get("worktree-changed")?.({
      payload: { project: "corral", feature: "hulling", git_changed: true },
    });
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("refresh_worktree_status", {
        project: "corral",
        feature: "hulling",
      });
    });

    expect(
      useStore.getState().worktrees.find((w) => w.feature === "hulling")
        ?.status,
    ).toEqual(status);
  });

  it("skips the git status refresh for an ordinary file edit", async () => {
    const { loadChanges } = await import("./changes");
    const { invoke } = await import("./tauri");
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    await initWorktreeWatch();

    listeners.get("worktree-changed")?.({
      payload: { project: "corral", feature: "hulling", git_changed: false },
    });

    expect(loadChanges).toHaveBeenCalledWith(wt);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not refresh git status for a worktree that isn't selected", async () => {
    const { invoke } = await import("./tauri");
    useStore.setState({ worktrees: [wt], selectedId: "corral/other" });
    await initWorktreeWatch();

    listeners.get("worktree-changed")?.({
      payload: { project: "corral", feature: "hulling", git_changed: true },
    });

    expect(invoke).not.toHaveBeenCalled();
  });
});
