import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setWorktreeNote } from "../lib/worktrees";
import { useStore } from "../store";
import type { GitStatus, Worktree } from "../types";
import { WorktreeHoverCard } from "./WorktreeHoverCard";

vi.mock("../lib/worktrees", () => ({
  setWorktreeNote: vi.fn().mockResolvedValue(undefined),
}));

function wt(overrides: Partial<Worktree> = {}): Worktree {
  return {
    project: "corral",
    feature: "churning",
    slug: "churning",
    branch: "feat/churning",
    path: "/path",
    status: null,
    has_resumable_session: false,
    note: "",
    ...overrides,
  };
}

function status(overrides: Partial<GitStatus> = {}): GitStatus {
  return {
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
    has_remote: false,
    last_commit_message: "",
    last_commit_time: 0,
    base_added: 0,
    base_removed: 0,
    ...overrides,
  };
}

const anchorRect = {
  top: 10,
  left: 10,
  bottom: 30,
  right: 100,
  width: 90,
  height: 20,
} as DOMRect;

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
});

describe("WorktreeHoverCard", () => {
  it("shows the feature slug in the header", () => {
    render(
      <WorktreeHoverCard
        wt={wt({ slug: "My Feature", feature: "my-feature" })}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("my-feature")).toBeInTheDocument();
  });

  it("shows branch-vs-base diff totals in the header", () => {
    render(
      <WorktreeHoverCard
        wt={wt({ status: status({ base_added: 40, base_removed: 5 }) })}
        status="committed"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("+40")).toBeInTheDocument();
    expect(screen.getByText("-5")).toBeInTheDocument();
  });

  it("shows the last commit message", () => {
    render(
      <WorktreeHoverCard
        wt={wt({
          status: status({ last_commit_message: "Fix the retry logic" }),
        })}
        status="committed"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("Fix the retry logic")).toBeInTheDocument();
  });

  it("shows the last commit's relative time", () => {
    const now = new Date("2026-07-14T00:00:00Z").getTime();
    vi.setSystemTime(now);
    const twoDaysAgo = now / 1000 - 2 * 24 * 60 * 60;
    render(
      <WorktreeHoverCard
        wt={wt({ status: status({ last_commit_time: twoDaysAgo }) })}
        status="committed"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("2 days ago")).toBeInTheDocument();
    vi.useRealTimers();
  });

  it("shows a PR badge linking out when PR info is present", () => {
    const target = wt();
    useStore.setState({
      prInfoByWtId: new Map([
        [
          "corral/churning",
          {
            number: 42,
            url: "https://example.com/pr/42",
            state: "open",
            is_draft: false,
          },
        ],
      ]),
    });
    render(
      <WorktreeHoverCard
        wt={target}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("#42")).toBeInTheDocument();
  });

  it("shows the full git-stat breakdown, including conflicts", () => {
    render(
      <WorktreeHoverCard
        wt={wt({
          status: status({
            staged: 1,
            modified: 2,
            untracked: 3,
            ahead: 1,
            behind: 2,
            conflicts: 1,
            added: 40,
            removed: 5,
          }),
        })}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("1 staged")).toBeInTheDocument();
    expect(screen.getByText("2 modified")).toBeInTheDocument();
    expect(screen.getByText("3 untracked")).toBeInTheDocument();
    expect(screen.getByText("↑1")).toBeInTheDocument();
    expect(screen.getByText("↓2")).toBeInTheDocument();
    expect(screen.getByText("1 conflict")).toBeInTheDocument();
    expect(screen.getByText("+40")).toBeInTheDocument();
    expect(screen.getByText("-5")).toBeInTheDocument();
  });

  it("shows a warning when the branch would conflict with the base branch", () => {
    render(
      <WorktreeHoverCard
        wt={wt({ status: status({ conflicts_with_base: true }) })}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(
      screen.getByText(
        "Would conflict with the base branch — rebase to resolve",
      ),
    ).toBeInTheDocument();
  });

  it("doesn't show the conflict warning when the branch wouldn't conflict", () => {
    render(
      <WorktreeHoverCard
        wt={wt({ status: status({ conflicts_with_base: false }) })}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(
      screen.queryByText(
        "Would conflict with the base branch — rebase to resolve",
      ),
    ).not.toBeInTheDocument();
  });

  it("shows the last activity summary when present", () => {
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
      <WorktreeHoverCard
        wt={wt()}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("Fixed the bug.")).toBeInTheDocument();
  });

  // Directly assigning `input.value` bypasses React's internal value
  // tracking (jsdom doesn't distinguish it from a programmatic set), so the
  // controlled `onChange` never fires. Going through the native setter first
  // — the same trick `@testing-library/user-event` uses internally — makes
  // the synthetic `input` event actually register as a change.
  function typeIntoInput(input: HTMLInputElement, value: string) {
    const nativeValueSetter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    )!.set!;
    input.focus();
    nativeValueSetter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }

  it("saves the note on blur, but not if it's unchanged", async () => {
    const target = wt({ note: "old note" });
    render(
      <WorktreeHoverCard
        wt={target}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    const input = screen.getByPlaceholderText(
      "Add a note…",
    ) as HTMLInputElement;
    expect(input.value).toBe("old note");

    input.focus();
    input.blur();
    expect(setWorktreeNote).not.toHaveBeenCalled();

    typeIntoInput(input, "new note");
    input.blur();
    expect(setWorktreeNote).toHaveBeenCalledWith(target, "new note");
  });

  it("flushes an unsaved note edit when the card unmounts without a blur", () => {
    const target = wt({ note: "old note" });
    const { unmount } = render(
      <WorktreeHoverCard
        wt={target}
        status="in-progress"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    const input = screen.getByPlaceholderText(
      "Add a note…",
    ) as HTMLInputElement;
    typeIntoInput(input, "new note");
    // No blur — this is the hover-close case, where the card's own
    // useHoverIntent-driven unmount doesn't reliably fire the input's blur
    // first.
    unmount();
    expect(setWorktreeNote).toHaveBeenCalledWith(target, "new note");
  });

  it("wires up the shared MergeButton for the card's worktree", () => {
    render(
      <WorktreeHoverCard
        wt={wt({ status: status({ commits_ahead_of_base: 2 }) })}
        status="committed"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("Merge to main")).toBeInTheDocument();
  });

  it("does not offer agent-driven PR actions for a background worktree", () => {
    const remoteWt = wt({
      status: status({ has_remote: true, commits_ahead_of_base: 1 }),
    });
    render(
      <WorktreeHoverCard
        wt={remoteWt}
        status="committed"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.queryByText("Open PR")).not.toBeInTheDocument();
  });

  it("keeps agent-driven PR actions on the selected worktree's card", () => {
    const remoteWt = wt({
      status: status({ has_remote: true, commits_ahead_of_base: 1 }),
    });
    useStore.setState({ selectedId: "corral/churning" });
    render(
      <WorktreeHoverCard
        wt={remoteWt}
        status="committed"
        anchorRect={anchorRect}
        onMouseEnter={vi.fn()}
        onMouseLeave={vi.fn()}
      />,
    );
    expect(screen.getByText("Open PR")).toBeInTheDocument();
  });
});
