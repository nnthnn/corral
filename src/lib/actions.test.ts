import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../session";
import { useStore } from "../store";
import type { PrInfo, Worktree } from "../types";
import { clearSelection, removeWorktree, selectWorktree } from "./actions";
import * as data from "./data";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./data", () => ({ loadAll: vi.fn() }));

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

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue({ base: null, files: [] });
  vi.mocked(data.loadAll).mockReset().mockResolvedValue(undefined);
});

describe("selectWorktree", () => {
  it("selects the worktree, clears its unread flag, and loads its changes", async () => {
    useStore.setState({
      worktrees: [wt],
      unreadOutput: new Set(["corral/hulling"]),
    });
    selectWorktree("corral/hulling");
    expect(useStore.getState().selectedId).toBe("corral/hulling");
    expect(useStore.getState().unreadOutput.has("corral/hulling")).toBe(false);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("branch_changes", {
        project: "corral",
        feature: "hulling",
      }),
    );
  });

  it("closes an open diff tab, falling back to the agent tab", () => {
    useStore.setState({
      worktrees: [wt],
      diffTab: { path: "src/main.rs" },
      activeTab: "diff",
    });
    selectWorktree("corral/hulling");
    expect(useStore.getState().diffTab).toBeNull();
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("keeps open tabs when re-selecting the already-selected worktree", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      diffTab: { path: "src/main.rs" },
      activeTab: "diff",
      changesPanelTab: "files",
    });
    selectWorktree("corral/hulling");
    expect(useStore.getState().diffTab).toEqual({ path: "src/main.rs" });
    expect(useStore.getState().activeTab).toBe("diff");
    expect(useStore.getState().changesPanelTab).toBe("files");
  });

  it("closes an open pr tab when the newly selected worktree has no cached PR, falling back to the agent tab", () => {
    useStore.setState({
      worktrees: [wt],
      activeTab: "pr",
      prInfoByWtId: new Map(),
    });
    selectWorktree("corral/hulling");
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("always focuses the agent tab on selection, even when the newly selected worktree has a cached PR", () => {
    const prInfo: PrInfo = {
      number: 42,
      url: "https://github.com/owner/repo/pull/42",
      state: "open",
      is_draft: false,
    };
    useStore.setState({
      worktrees: [wt],
      activeTab: "pr",
      prInfoByWtId: new Map([["corral/hulling", prInfo]]),
    });
    selectWorktree("corral/hulling");
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("resets the Files panel back to Changes when switching worktrees", () => {
    const other: Worktree = { ...wt, feature: "churning" };
    useStore.setState({
      worktrees: [wt, other],
      changesPanelTab: "files",
    });
    selectWorktree("corral/churning");
    expect(useStore.getState().changesPanelTab).toBe("changes");
  });

  it("clears the previous worktree's changes when switching", () => {
    const other: Worktree = { ...wt, feature: "churning" };
    useStore.setState({
      worktrees: [wt, other],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
    });
    selectWorktree("corral/churning");
    expect(useStore.getState().changes).toBeNull();
  });

  it("acknowledges a waiting session's attention cue when selected", () => {
    const session: SessionInfo = {
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
      waiting: true,
      last_summary: null,
    };
    useStore.setState({
      worktrees: [wt],
      sessions: new Map([["corral/hulling", session]]),
    });
    selectWorktree("corral/hulling");
    expect(useStore.getState().acknowledgedWaiting.has("corral/hulling")).toBe(
      true,
    );
  });
});

describe("selectWorktree PR status wiring", () => {
  it("fetches PR status only for a worktree with a remote", () => {
    const withRemote: Worktree = {
      project: "corral",
      feature: "hulling",
      slug: "hulling",
      branch: "feat/hulling",
      path: "/path",
      has_resumable_session: false,
      note: "",
      status: {
        branch: "feat/hulling",
        ahead: 0,
        behind: 0,
        staged: 0,
        modified: 0,
        untracked: 0,
        added: 0,
        removed: 0,
        conflicts: 0,
        commits_ahead_of_base: 1,
        merged_into_base: false,
        conflicts_with_base: false,
        has_remote: true,
        last_commit_message: "",
        last_commit_time: 0,
        base_added: 0,
        base_removed: 0,
      },
    };
    useStore.setState({ worktrees: [withRemote] });
    selectWorktree("corral/hulling");
    expect(invoke).toHaveBeenCalledWith("pr_status", {
      project: "corral",
      feature: "hulling",
    });
  });

  it("does not fetch PR status for a worktree without a remote", () => {
    const noRemote: Worktree = {
      project: "corral",
      feature: "hulling",
      slug: "hulling",
      branch: "feat/hulling",
      path: "/path",
      has_resumable_session: false,
      note: "",
      status: null,
    };
    useStore.setState({ worktrees: [noRemote] });
    selectWorktree("corral/hulling");
    expect(invoke).not.toHaveBeenCalledWith("pr_status", expect.anything());
  });
});

describe("clearSelection", () => {
  it("resets selection, diff tab, and active tab", () => {
    useStore.setState({
      selectedId: "corral/hulling",
      diffTab: { path: "src/main.rs" },
      activeTab: "diff",
    });
    clearSelection();
    expect(useStore.getState()).toMatchObject({
      selectedId: null,
      diffTab: null,
      activeTab: "agent",
    });
  });

  it("resets the Files panel back to Changes", () => {
    useStore.setState({ changesPanelTab: "files" });
    clearSelection();
    expect(useStore.getState().changesPanelTab).toBe("changes");
  });
});

describe("removeWorktree", () => {
  it("closes the delete dialog as soon as the backend call resolves, before the animation window elapses", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    useStore.setState({ deleteTarget: wt });
    vi.useFakeTimers();
    try {
      const promise = removeWorktree(wt, true);
      await vi.advanceTimersByTimeAsync(0);
      expect(useStore.getState().deleteTarget).toBeNull();
      expect(useStore.getState().removingIds.has("corral/hulling")).toBe(true);
      await vi.advanceTimersByTimeAsync(200);
      await promise;
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears session/selection state and removingIds only after the animation window", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    useStore.setState({
      deleteTarget: wt,
      selectedId: "corral/hulling",
      sessions: new Map([
        [
          "corral/hulling",
          {
            id: "corral/hulling",
            project: "corral",
            feature: "hulling",
            running: false,
            exit_code: 0,
            killed: false,
            waiting: false,
            last_summary: null,
          },
        ],
      ]),
    });
    vi.useFakeTimers();
    try {
      const promise = removeWorktree(wt, true);
      await vi.advanceTimersByTimeAsync(0);
      expect(useStore.getState().sessions.has("corral/hulling")).toBe(true);
      await vi.advanceTimersByTimeAsync(200);
      await promise;
      expect(useStore.getState().sessions.has("corral/hulling")).toBe(false);
      expect(useStore.getState().removingIds.has("corral/hulling")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears PR preparation when removing a worktree", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    useStore.setState({
      deleteTarget: wt,
      preparingPr: new Map([["corral/hulling", { token: 1, kind: "create" }]]),
    });
    vi.useFakeTimers();
    try {
      const promise = removeWorktree(wt, true);
      await vi.advanceTimersByTimeAsync(200);
      await promise;
      expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears cached PR info and checks for the removed worktree", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    const prInfo: PrInfo = {
      number: 42,
      url: "https://github.com/owner/repo/pull/42",
      state: "open",
      is_draft: false,
    };
    useStore.setState({
      deleteTarget: wt,
      prInfoByWtId: new Map([["corral/hulling", prInfo]]),
      prDetailsByWtId: new Map([["corral/hulling", null]]),
    });
    vi.useFakeTimers();
    try {
      const promise = removeWorktree(wt, true);
      await vi.advanceTimersByTimeAsync(200);
      await promise;
      expect(useStore.getState().prInfoByWtId.has("corral/hulling")).toBe(
        false,
      );
      expect(useStore.getState().prDetailsByWtId.has("corral/hulling")).toBe(
        false,
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("auto-selects the adjacent worktree and flashes the main pane when the removed one was selected", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    const other: Worktree = { ...wt, feature: "churning" };
    useStore.setState({
      deleteTarget: wt,
      selectedId: "corral/hulling",
      worktrees: [wt, other],
      projects: ["corral"],
    });
    vi.useFakeTimers();
    try {
      const promise = removeWorktree(wt, true);
      await vi.advanceTimersByTimeAsync(200);
      await promise;
      expect(useStore.getState().selectedId).toBe("corral/churning");
      expect(useStore.getState().reselectFlash).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not flash or reselect when a different worktree was selected", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    useStore.setState({ deleteTarget: wt, selectedId: "corral/other" });
    vi.useFakeTimers();
    try {
      const promise = removeWorktree(wt, true);
      await vi.advanceTimersByTimeAsync(200);
      await promise;
      expect(useStore.getState().selectedId).toBe("corral/other");
      expect(useStore.getState().reselectFlash).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps removingIds set while loadAll is still pending, only clearing it after loadAll resolves", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    let resolveLoadAll: () => void = () => {};
    vi.mocked(data.loadAll).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveLoadAll = () => resolve(undefined);
        }),
    );
    useStore.setState({ deleteTarget: wt });
    vi.useFakeTimers();
    try {
      const promise = removeWorktree(wt, true);
      await vi.advanceTimersByTimeAsync(200);
      expect(useStore.getState().removingIds.has("corral/hulling")).toBe(true);
      resolveLoadAll();
      await promise;
      expect(useStore.getState().removingIds.has("corral/hulling")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("rejects and leaves the dialog open when the backend call fails", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    useStore.setState({ deleteTarget: wt });
    await expect(removeWorktree(wt, true)).rejects.toThrow("boom");
    expect(useStore.getState().deleteTarget).toEqual(wt);
    expect(useStore.getState().removingIds.size).toBe(0);
  });
});
