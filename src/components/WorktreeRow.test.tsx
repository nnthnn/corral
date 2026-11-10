import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { GitStatus, Worktree } from "../types";
import { WorktreeRow } from "./WorktreeRow";

function wt(status: GitStatus | null): Worktree {
  return {
    project: "corral",
    feature: "churning",
    slug: "churning",
    branch: "feat/churning",
    path: "/path",
    status,
    has_resumable_session: false,
    note: "",
  };
}

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
});

describe("WorktreeRow", () => {
  it("shows the Merged feature-status icon instead of an agent-activity dot", () => {
    const status: GitStatus = {
      branch: "feat/churning",
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
    };
    render(
      <WorktreeRow
        wt={wt(status)}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByTitle("Merged")).toBeInTheDocument();
    expect(document.querySelector(".sess-dot")).toBeNull();
  });

  it("shows the pr_open dot when the backend poll has cached an open PR", () => {
    const status: GitStatus = {
      branch: "feat/churning",
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
    };
    render(
      <WorktreeRow
        wt={{
          ...wt(status),
          pr_state: {
            number: 7,
            url: "https://x",
            state: "open",
            is_draft: false,
          },
        }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByTitle("PR open")).toBeInTheDocument();
  });

  it("prefers a confirmed no-PR from the live store over a stale poll's pr_state", () => {
    const status: GitStatus = {
      branch: "feat/churning",
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
    };
    useStore.setState({
      prInfoByWtId: new Map([["corral/churning", null]]),
    });
    render(
      <WorktreeRow
        wt={{
          ...wt(status),
          pr_state: {
            number: 1,
            url: "https://x",
            state: "open",
            is_draft: false,
          },
        }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.queryByTitle("PR open")).not.toBeInTheDocument();
    expect(screen.getByTitle("Committed")).toBeInTheDocument();
  });

  it("shows the pr_draft dot for a draft PR", () => {
    const status: GitStatus = {
      branch: "feat/churning",
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
    };
    render(
      <WorktreeRow
        wt={{
          ...wt(status),
          pr_state: {
            number: 7,
            url: "https://x",
            state: "open",
            is_draft: true,
          },
        }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByTitle("Draft PR")).toBeInTheDocument();
  });

  /** A clean, remote-having status — the shape a worktree with a PR has. */
  const cleanStatus: GitStatus = {
    branch: "feat/churning",
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
    has_remote: true,
    last_commit_message: "",
    last_commit_time: 0,
    base_added: 0,
    base_removed: 0,
  };

  it("shows a CI-running indicator from the backend poll summary", () => {
    render(
      <WorktreeRow
        wt={{
          ...wt(cleanStatus),
          pr_checks: { total: 3, pending: 2, failed: 0 },
        }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(
      screen.getByTitle("Pull request checks running"),
    ).toBeInTheDocument();
  });

  it("shows a CI-failed indicator from the backend poll summary", () => {
    render(
      <WorktreeRow
        wt={{
          ...wt(cleanStatus),
          pr_checks: { total: 3, pending: 1, failed: 1 },
        }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByTitle("Pull request checks failed")).toBeInTheDocument();
  });

  it("prefers live polled checks over the backend's coarse summary", () => {
    useStore.setState({
      checksByWtId: new Map([
        [
          "corral/churning",
          [
            {
              name: "test",
              workflow: "ci.yml",
              bucket: "fail",
              url: "https://x/1",
            },
          ],
        ],
      ]),
    });
    render(
      <WorktreeRow
        wt={{
          ...wt(cleanStatus),
          pr_checks: { total: 3, pending: 2, failed: 0 },
        }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByTitle("Pull request checks failed")).toBeInTheDocument();
  });

  it("renders no feature-status icon when there's no git status", () => {
    render(
      <WorktreeRow
        wt={wt(null)}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.queryByTitle("New")).not.toBeInTheDocument();
  });

  it("selects the row when clicking the right-slot indicator", () => {
    const status: GitStatus = {
      branch: "feat/churning",
      ahead: 1,
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
    };
    const onSelect = vi.fn();
    render(
      <WorktreeRow
        wt={wt(status)}
        filterText=""
        active={false}
        onSelect={onSelect}
        onRemove={vi.fn()}
      />,
    );
    screen.getByText("↑1").click();
    expect(onSelect).toHaveBeenCalledWith("corral/churning");
  });

  it("does not render a right-slot indicator, feature slug, summary, or note inline", () => {
    useStore.setState({
      sessions: new Map([
        [
          "corral/churning",
          {
            id: "corral/churning",
            project: "corral",
            feature: "churning",
            running: true,
            exit_code: null,
            killed: false,
            waiting: false,
            last_summary: "Fixed the **bug**.",
          },
        ],
      ]),
    });
    render(
      <WorktreeRow
        wt={{ ...wt(null), slug: "My Feature", note: "a note" }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.queryByText("Fixed the bug.")).not.toBeInTheDocument();
    expect(screen.queryByText("churning")).not.toBeInTheDocument();
    expect(screen.queryByText("a note")).not.toBeInTheDocument();
  });

  it("shows the title instead of the slug when a title is set", () => {
    render(
      <WorktreeRow
        wt={{
          ...wt(null),
          title: "Fix Login Timeout",
          slug: "fix-login-timeout",
        }}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByText("Fix Login Timeout")).toBeInTheDocument();
  });

  it("opens the hover card immediately on keyboard focus", () => {
    render(
      <WorktreeRow
        wt={wt(null)}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    // A bare `.focus()` call isn't wrapped in `act()`, and React 19 batches
    // state updates from native (non-act-wrapped) DOM events, so the
    // assertion below would run before the update flushes. `fireEvent`
    // wraps in `act()` for us and exercises the same focus event handler.
    fireEvent.focus(screen.getByRole("button", { name: "churning" }));
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
  });

  it("does not select the row when clicking inside the hover card", () => {
    const onSelect = vi.fn();
    render(
      <WorktreeRow
        wt={wt(null)}
        filterText=""
        active={false}
        onSelect={onSelect}
        onRemove={vi.fn()}
      />,
    );
    fireEvent.focus(screen.getByRole("button", { name: "churning" }));
    const noteInput = screen.getByPlaceholderText("Add a note…");
    fireEvent.click(noteInput);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("lets Space and Enter reach the note field instead of the row's own shortcuts", () => {
    const onSelect = vi.fn();
    render(
      <WorktreeRow
        wt={wt(null)}
        filterText=""
        active={false}
        onSelect={onSelect}
        onRemove={vi.fn()}
      />,
    );
    fireEvent.focus(screen.getByRole("button", { name: "churning" }));
    const noteInput = screen.getByPlaceholderText("Add a note…");
    const spaceEvent = new KeyboardEvent("keydown", {
      key: " ",
      bubbles: true,
      cancelable: true,
    });
    noteInput.dispatchEvent(spaceEvent);
    expect(spaceEvent.defaultPrevented).toBe(false);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("gets an exiting class while its removal animation plays", () => {
    useStore.setState({ removingIds: new Set(["corral/churning"]) });
    render(
      <WorktreeRow
        wt={wt(null)}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "churning" })).toHaveClass(
      "exiting",
    );
  });

  it("does not get an exiting class otherwise", () => {
    render(
      <WorktreeRow
        wt={wt(null)}
        filterText=""
        active={false}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "churning" })).not.toHaveClass(
      "exiting",
    );
  });
});
