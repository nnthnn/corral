import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as worktrees from "../lib/worktrees";
import { useStore } from "../store";
import type { GitStatus, Worktree } from "../types";
import { RebaseButton, rebaseButtonState } from "./RebaseButton";

vi.mock("../lib/worktrees", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/worktrees")>(
      "../lib/worktrees",
    );
  return {
    ...actual,
    rebaseWorktree: vi.fn(),
    askClaudeToRebase: vi.fn().mockResolvedValue(undefined),
  };
});

function wtWith(status: Partial<GitStatus> | null): Worktree {
  return {
    project: "p",
    feature: "f",
    slug: "f",
    branch: "feat/f",
    path: "/tmp/f",
    has_resumable_session: false,
    note: "",
    status: status && {
      branch: "feat/f",
      ahead: 0,
      behind: 0,
      staged: 0,
      modified: 0,
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
      ...status,
    },
  };
}

describe("rebaseButtonState", () => {
  it("is hidden when there's no status", () => {
    expect(rebaseButtonState(wtWith(null))).toEqual({ kind: "hidden" });
  });

  it("is hidden when there's no conflict", () => {
    expect(rebaseButtonState(wtWith({ conflicts_with_base: false }))).toEqual({
      kind: "hidden",
    });
  });

  it("shows a note when there's a conflict but the worktree is dirty", () => {
    expect(
      rebaseButtonState(wtWith({ conflicts_with_base: true, modified: 1 })),
    ).toEqual({
      kind: "note",
      text: "Uncommitted changes",
    });
  });

  it("offers to rebase when there's a conflict and the worktree is clean", () => {
    expect(rebaseButtonState(wtWith({ conflicts_with_base: true }))).toEqual({
      kind: "rebase",
    });
  });
});

describe("RebaseButton", () => {
  beforeEach(() => {
    useStore.setState(useStore.getInitialState());
    vi.mocked(worktrees.rebaseWorktree).mockReset();
    vi.mocked(worktrees.askClaudeToRebase)
      .mockReset()
      .mockResolvedValue(undefined);
  });

  it("defaults to direct mode, calling rebaseWorktree", () => {
    render(<RebaseButton wt={wtWith({ conflicts_with_base: true })} />);
    fireEvent.click(screen.getByText("Rebase"));
    expect(worktrees.rebaseWorktree).toHaveBeenCalled();
    expect(worktrees.askClaudeToRebase).not.toHaveBeenCalled();
  });

  it("shows a note instead of a button when the worktree is dirty", () => {
    render(
      <RebaseButton wt={wtWith({ conflicts_with_base: true, modified: 1 })} />,
    );
    expect(screen.getByText("Uncommitted changes")).toBeInTheDocument();
    expect(screen.queryByText("Rebase")).not.toBeInTheDocument();
  });

  it("in chat mode, asks Claude to rebase instead of rebasing directly", () => {
    const wt = wtWith({ conflicts_with_base: true });
    render(<RebaseButton wt={wt} mode="chat" base="main" />);
    fireEvent.click(screen.getByText("Rebase"));
    expect(worktrees.askClaudeToRebase).toHaveBeenCalledWith(wt, "main");
    expect(worktrees.rebaseWorktree).not.toHaveBeenCalled();
  });

  it("in chat mode without a detected base, is disabled", () => {
    render(
      <RebaseButton
        wt={wtWith({ conflicts_with_base: true })}
        mode="chat"
        base={null}
      />,
    );
    expect(screen.getByText("Rebase")).toBeDisabled();
  });
});
