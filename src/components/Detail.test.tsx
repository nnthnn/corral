import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { Detail } from "./Detail";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

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
  vi.mocked(open).mockReset();
});

describe("Detail", () => {
  it("shows the no-worktree-selected state when wt is null", () => {
    render(<Detail wt={null} />);
    expect(screen.getByText("No worktree selected")).toBeInTheDocument();
  });

  it("opens the add-project folder picker from the empty state", async () => {
    vi.mocked(open).mockResolvedValueOnce(null);
    render(<Detail wt={null} />);
    fireEvent.click(screen.getByText("Add project"));
    await waitFor(() =>
      expect(open).toHaveBeenCalledWith({ directory: true, multiple: false }),
    );
  });

  it("hides the New worktree card when there are no projects", () => {
    useStore.setState({ projects: [] });
    render(<Detail wt={null} />);
    expect(screen.queryByText("New worktree")).not.toBeInTheDocument();
  });

  it("nudges the sidebar's add button when New worktree is clicked", () => {
    useStore.setState({ projects: ["corral"] });
    render(<Detail wt={null} />);
    fireEvent.click(screen.getByText("New worktree"));
    expect(useStore.getState().worktreeNudgeTarget).toBe("corral");
  });

  it("shows the no-agent-running state and starts the agent on click", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
    });
    render(<Detail wt={wt} />);
    expect(screen.getByText("No agent in this pen.")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Start agent"));
    expect(invoke).toHaveBeenCalledWith("start_session", {
      project: "corral",
      feature: "hulling",
      fresh: false,
    });
  });

  it("shows the merged state once the branch is merged into base", async () => {
    const merged: Worktree = {
      ...wt,
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
        commits_ahead_of_base: 0,
        merged_into_base: true,
        conflicts_with_base: false,
        has_remote: false,
        last_commit_message: "",
        last_commit_time: 0,
        base_added: 0,
        base_removed: 0,
      },
    };
    render(<Detail wt={merged} />);
    expect(screen.getByText("Merged into main.")).toBeInTheDocument();
    expect(screen.queryByText("No agent in this pen.")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("Remove"));
    await waitFor(() => expect(useStore.getState().deleteTarget).toBeTruthy());
  });

  it("does not show the merged state when merged_into_base is true but the worktree has uncommitted edits since", () => {
    const mergedButDirty: Worktree = {
      ...wt,
      status: {
        branch: "feat/hulling",
        ahead: 0,
        behind: 0,
        staged: 0,
        modified: 1,
        untracked: 0,
        added: 0,
        removed: 0,
        conflicts: 0,
        commits_ahead_of_base: 0,
        merged_into_base: true,
        conflicts_with_base: false,
        has_remote: false,
        last_commit_message: "",
        last_commit_time: 0,
        base_added: 0,
        base_removed: 0,
      },
    };
    render(<Detail wt={mergedButDirty} />);
    expect(screen.queryByText("Merged into main.")).not.toBeInTheDocument();
    expect(screen.getByText("No agent in this pen.")).toBeInTheDocument();
  });

  it("shows the merged state when GitHub reports the PR merged, even with local commits still ahead", async () => {
    useStore.setState({
      prInfoByWtId: new Map([
        [
          "corral/hulling",
          { number: 3, url: "https://x", state: "merged", is_draft: false },
        ],
      ]),
    });
    const mergedViaGithub: Worktree = {
      ...wt,
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
        // Simulates a squash merge: local ancestry never catches up.
        commits_ahead_of_base: 3,
        merged_into_base: false,
        conflicts_with_base: false,
        has_remote: true,
        last_commit_message: "",
        last_commit_time: 0,
        base_added: 0,
        base_removed: 0,
      },
    };
    render(<Detail wt={mergedViaGithub} />);
    expect(screen.getByText("Merged into main.")).toBeInTheDocument();
  });

  it("prefers a confirmed no-PR from the live store over a stale truthy wt.pr_state", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", null]]),
    });
    const mergedLocallyStalePrState: Worktree = {
      ...wt,
      pr_state: { number: 5, url: "https://x", state: "open", is_draft: false },
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
        commits_ahead_of_base: 0,
        merged_into_base: true,
        conflicts_with_base: false,
        has_remote: false,
        last_commit_message: "",
        last_commit_time: 0,
        base_added: 0,
        base_removed: 0,
      },
    };
    render(<Detail wt={mergedLocallyStalePrState} />);
    expect(screen.getByText("Merged into main.")).toBeInTheDocument();
  });
});
