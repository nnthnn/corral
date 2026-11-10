import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { ChangedFile, Worktree } from "../types";
import { openDiff, stepDiff } from "./diff";

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

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  // Diffs are only ever opened for the selected worktree, and the stale
  // guard keys on that selection.
  useStore.setState({ selectedId: "corral/hulling" });
  vi.mocked(invoke).mockReset();
});

describe("openDiff", () => {
  it("opens the diff tab and loads content on success", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(
      "--- a/src/main.rs\n+++ b/src/main.rs\n",
    );
    await openDiff(wt, "src/main.rs");
    expect(invoke).toHaveBeenCalledWith("file_diff", {
      project: "corral",
      feature: "hulling",
      path: "src/main.rs",
    });
    expect(useStore.getState().diffTab).toEqual({ path: "src/main.rs" });
    expect(useStore.getState().activeTab).toBe("diff");
    expect(useStore.getState().diffContent).toContain("+++ b/src/main.rs");
    expect(useStore.getState().diffError).toBeNull();
  });

  it("records an error string on failure", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("no such file"));
    await openDiff(wt, "src/gone.rs");
    expect(useStore.getState().diffError).toBe("Error: no such file");
    expect(useStore.getState().diffContent).toBeNull();
  });

  it("ignores a stale response after a different file was opened", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce("stale diff content")
      .mockResolvedValueOnce("fresh diff content");
    const first = openDiff(wt, "src/first.rs");
    await openDiff(wt, "src/second.rs");
    await first;
    expect(useStore.getState().diffTab).toEqual({ path: "src/second.rs" });
    expect(useStore.getState().diffContent).toBe("fresh diff content");
  });

  it("ignores a same-path response that belongs to a different worktree", async () => {
    const other: Worktree = { ...wt, feature: "churning" };
    // The first (slow) fetch belongs to hulling; by the time it resolves we've
    // switched to churning and opened that worktree's same-named file.
    vi.mocked(invoke)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) =>
            setTimeout(() => resolve("hulling diff"), 0),
          ),
      )
      .mockResolvedValueOnce("churning diff");

    const first = openDiff(wt, "src/main.rs");
    useStore.setState({ selectedId: "corral/churning" });
    await openDiff(other, "src/main.rs");
    await first;

    expect(useStore.getState().diffContent).toBe("churning diff");
  });
});

describe("stepDiff", () => {
  const files: ChangedFile[] = [
    {
      path: "a.rs",
      status: "modified",
      old_path: null,
      staged: false,
      sync_state: "uncommitted",
      dirty: true,
      added: 1,
      removed: 0,
    },
    {
      path: "b.rs",
      status: "modified",
      old_path: null,
      staged: false,
      sync_state: "uncommitted",
      dirty: true,
      added: 1,
      removed: 0,
    },
    {
      path: "c.rs",
      status: "modified",
      old_path: null,
      staged: false,
      sync_state: "uncommitted",
      dirty: true,
      added: 1,
      removed: 0,
    },
  ];

  function select(diffPath: string) {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      activeTab: "diff",
      diffTab: { path: diffPath },
      changes: { base: "main", files: [...files] },
    });
  }

  it("opens the next file in the changes list", async () => {
    select("b.rs");
    stepDiff(1);
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledWith("file_diff", {
      project: "corral",
      feature: "hulling",
      path: "c.rs",
    });
  });

  it("opens the previous file in the changes list", async () => {
    select("b.rs");
    stepDiff(-1);
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledWith("file_diff", {
      project: "corral",
      feature: "hulling",
      path: "a.rs",
    });
  });

  it("jumps to the first file when the current path is not in the list", async () => {
    select("ghost.rs");
    stepDiff(1);
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledWith("file_diff", {
      project: "corral",
      feature: "hulling",
      path: "a.rs",
    });
  });

  it("does nothing past the ends", () => {
    select("c.rs");
    stepDiff(1);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does nothing when the diff tab is not active", () => {
    select("b.rs");
    useStore.setState({ activeTab: "agent" });
    stepDiff(1);
    expect(invoke).not.toHaveBeenCalled();
  });
});
