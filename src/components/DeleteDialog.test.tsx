import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { DeleteDialog } from "./DeleteDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const wt: Worktree = {
  project: "corral",
  feature: "hulling",
  slug: "hulling",
  branch: "feat/hulling",
  path: "/path/hulling",
  status: {
    branch: "feat/hulling",
    ahead: 2,
    behind: 0,
    staged: 1,
    modified: 1,
    untracked: 0,
    added: 0,
    removed: 0,
    conflicts: 0,
    commits_ahead_of_base: 0,
    merged_into_base: false,
    conflicts_with_base: false,
    has_remote: false,
    last_commit_message: "",
    last_commit_time: 0,
    base_added: 0,
    base_removed: 0,
  },
  has_resumable_session: false,
  note: "",
};

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
});

describe("DeleteDialog", () => {
  it("shows the target's identity, path, and dirty/ahead warnings", () => {
    useStore.setState({ deleteTarget: wt });
    render(<DeleteDialog />);
    expect(screen.getByText("corral/hulling")).toBeInTheDocument();
    expect(screen.getByText("/path/hulling")).toBeInTheDocument();
    expect(
      screen.getByText(/2 uncommitted changes will be lost/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/2 commits not merged upstream/),
    ).toBeInTheDocument();
  });

  it("shows a spinner and disables the form while removal is in flight", async () => {
    useStore.setState({ deleteTarget: wt });
    let resolveInvoke: () => void = () => {};
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveInvoke = () => resolve(undefined);
        }),
    );
    render(<DeleteDialog />);
    fireEvent.click(screen.getByText("Remove"));
    await vi.waitFor(() => {
      expect(screen.getByText("Removing…")).toBeInTheDocument();
    });
    expect(screen.getByText("Cancel")).toBeDisabled();
    expect(screen.getByRole("checkbox")).toBeDisabled();
    // Still open — the backend call hasn't resolved yet.
    expect(useStore.getState().deleteTarget).toEqual(wt);
    resolveInvoke();
    await vi.waitFor(() => expect(useStore.getState().deleteTarget).toBeNull());
  });

  it("removes the worktree, cleans up session state, and reloads on submit", async () => {
    useStore.setState({
      deleteTarget: wt,
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
            waiting: true,
            last_summary: null,
          },
        ],
      ]),
      selectedId: "corral/hulling",
    });
    vi.mocked(invoke)
      .mockResolvedValueOnce(undefined) // remove_worktree
      .mockResolvedValueOnce([]) // list_worktrees (loadAll)
      .mockResolvedValueOnce([]); // list_sessions (loadAll)
    vi.useFakeTimers();
    try {
      render(<DeleteDialog />);
      fireEvent.click(screen.getByText("Remove"));
      await vi.advanceTimersByTimeAsync(0);
      expect(useStore.getState().deleteTarget).toBeNull();
      await vi.advanceTimersByTimeAsync(200);
      expect(useStore.getState().sessions.has("corral/hulling")).toBe(false);
      expect(useStore.getState().selectedId).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("respects unchecking 'also delete branch'", async () => {
    useStore.setState({ deleteTarget: wt });
    vi.mocked(invoke)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);
    render(<DeleteDialog />);
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByText("Remove"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("remove_worktree", {
        project: "corral",
        feature: "hulling",
        deleteBranch: false,
      }),
    );
  });

  it("defaults the checkbox to the delete_branch_default setting", async () => {
    useStore.setState({
      deleteTarget: wt,
      settings: {
        ...useStore.getState().settings,
        delete_branch_default: false,
      },
    });
    vi.mocked(invoke)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);
    render(<DeleteDialog />);
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    fireEvent.click(screen.getByText("Remove"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("remove_worktree", {
        project: "corral",
        feature: "hulling",
        deleteBranch: false,
      }),
    );
  });

  it("cancels without calling remove_worktree", () => {
    useStore.setState({ deleteTarget: wt });
    render(<DeleteDialog />);
    fireEvent.click(screen.getByText("Cancel"));
    expect(useStore.getState().deleteTarget).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("re-enables the form and keeps the dialog open when removal fails", async () => {
    useStore.setState({ deleteTarget: wt });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    render(<DeleteDialog />);
    fireEvent.click(screen.getByText("Remove"));
    await vi.waitFor(() =>
      expect(screen.queryByText("Removing…")).not.toBeInTheDocument(),
    );
    expect(useStore.getState().deleteTarget).toEqual(wt);
    expect(screen.getByText("Cancel")).not.toBeDisabled();
  });
});
