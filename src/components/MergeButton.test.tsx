import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { GitStatus, Worktree } from "../types";
import { MergeButton } from "./MergeButton";

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
  vi.mocked(invoke).mockReset();
  // Default fallback so background calls this suite doesn't care about
  // (e.g. usePrChecksPolling's fetchPrChecks, fired whenever a rendered
  // worktree has an open PR) resolve instead of throwing on a bare
  // `undefined` return — individual tests still override via
  // mockResolvedValueOnce/mockResolvedValue for the calls they assert on.
  vi.mocked(invoke).mockResolvedValue(undefined);
});

const dirtyStatus: GitStatus = {
  branch: "feat/hulling",
  ahead: 0,
  behind: 0,
  staged: 1,
  modified: 0,
  untracked: 0,
  added: 0,
  removed: 0,
  conflicts: 0,
  commits_ahead_of_base: 2,
  merged_into_base: false,
  conflicts_with_base: false,
  has_remote: false,
  last_commit_message: "",
  last_commit_time: 0,
  base_added: 0,
  base_removed: 0,
};

const dirtyWt: Worktree = { ...wt, status: dirtyStatus };

const readyStatus: GitStatus = {
  ...dirtyStatus,
  staged: 0,
  commits_ahead_of_base: 2,
};

const readyWt: Worktree = { ...wt, status: readyStatus };

const mergedStatus: GitStatus = {
  ...readyStatus,
  commits_ahead_of_base: 0,
  merged_into_base: true,
};

const mergedWt: Worktree = { ...wt, status: mergedStatus };

const justMergedStatus: GitStatus = {
  ...readyStatus,
  commits_ahead_of_base: 0,
  merged_into_base: false,
};

const justMergedWt: Worktree = { ...wt, status: justMergedStatus };

describe("MergeButton merge/remove button", () => {
  it("hides the button when there's no git status", () => {
    render(<MergeButton wt={wt} />);
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("hides the button when there's nothing ahead of base", () => {
    const neverDiverged: Worktree = {
      ...wt,
      status: { ...readyStatus, commits_ahead_of_base: 0 },
    };
    render(<MergeButton wt={neverDiverged} />);
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("shows a note instead of the button when dirty but nothing ahead of base", () => {
    const dirtyNoAhead: Worktree = {
      ...wt,
      status: { ...readyStatus, commits_ahead_of_base: 0, staged: 1 },
    };
    render(<MergeButton wt={dirtyNoAhead} />);
    expect(screen.getByText("Uncommitted changes")).toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("shows a conflict note when conflicted but nothing ahead of base", () => {
    const conflictedNoAhead: Worktree = {
      ...wt,
      status: { ...readyStatus, commits_ahead_of_base: 0, conflicts: 1 },
    };
    render(<MergeButton wt={conflictedNoAhead} />);
    expect(screen.getByText("Resolve conflicts")).toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("shows a note instead of the button when dirty with commits ahead of base", () => {
    render(<MergeButton wt={dirtyWt} />);
    expect(screen.getByText("Uncommitted changes")).toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("prefers the uncommitted-changes note when both dirty and conflicted", () => {
    const dirtyConflicted: Worktree = {
      ...wt,
      status: { ...dirtyStatus, conflicts: 1 },
    };
    render(<MergeButton wt={dirtyConflicted} />);
    expect(screen.getByText("Uncommitted changes")).toBeInTheDocument();
    expect(screen.queryByText("Resolve conflicts")).not.toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("enables the button and merges on click when clean and ahead of base", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      outcome: "merged",
      pushed: false,
    });
    vi.mocked(invoke).mockResolvedValue([]);
    render(<MergeButton wt={readyWt} />);
    fireEvent.click(screen.getByText("Merge to main"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("merge_worktree", {
        project: "corral",
        feature: "hulling",
      }),
    );
  });

  it("shows a Remove button once merged_into_base is true", () => {
    render(<MergeButton wt={mergedWt} />);
    expect(screen.getByText("Remove")).toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("opens the delete dialog on click", async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    render(<MergeButton wt={mergedWt} />);
    fireEvent.click(screen.getByText("Remove"));
    await vi.waitFor(() =>
      expect(useStore.getState().deleteTarget).toBe(mergedWt),
    );
  });

  it("shows Remove for the ephemeral just-merged bridge state", () => {
    useStore.setState({
      justMergedIds: new Set(["corral/hulling"]),
    });
    render(<MergeButton wt={justMergedWt} />);
    expect(screen.getByText("Remove")).toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("hides the merge-adjacent button for the same status when not in justMergedIds", () => {
    render(<MergeButton wt={justMergedWt} />);
    expect(screen.queryByText("Remove")).not.toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });
});

const remoteAheadStatus: GitStatus = {
  ...readyStatus,
  has_remote: true,
  last_commit_message: "",
  last_commit_time: 0,
  base_added: 0,
  base_removed: 0,
};
const remoteAheadWt: Worktree = { ...wt, status: remoteAheadStatus };

const remoteNoPrNoAheadWt: Worktree = {
  ...wt,
  status: { ...remoteAheadStatus, commits_ahead_of_base: 0 },
};

const remoteDirtyWt: Worktree = {
  ...wt,
  status: { ...remoteAheadStatus, staged: 1 },
};

const remoteDirtyNoPrNoAheadWt: Worktree = {
  ...wt,
  status: { ...remoteAheadStatus, commits_ahead_of_base: 0, staged: 1 },
};

const openPrInfo = {
  number: 7,
  url: "https://github.com/x/y/pull/7",
  state: "open" as const,
  is_draft: false,
};

const mergedPrInfo = { ...openPrInfo, state: "merged" as const };

describe("MergeButton push+PR button (has_remote)", () => {
  it("hides the button when there's no PR yet and nothing ahead of base", () => {
    render(<MergeButton wt={remoteNoPrNoAheadWt} />);
    expect(screen.queryByText("Open PR")).not.toBeInTheDocument();
  });

  it("shows a conflict note when clean but conflicted with no PR and nothing ahead", () => {
    const conflictedRemote: Worktree = {
      ...wt,
      status: { ...remoteAheadStatus, commits_ahead_of_base: 0, conflicts: 1 },
    };
    render(<MergeButton wt={conflictedRemote} />);
    expect(screen.getByText("Resolve conflicts")).toBeInTheDocument();
    expect(screen.queryByText("Open PR")).not.toBeInTheDocument();
  });

  it("offers PR preparation when only uncommitted changes exist", () => {
    render(<MergeButton wt={remoteDirtyNoPrNoAheadWt} />);
    expect(screen.getByText("Open PR")).toBeInTheDocument();
  });

  it("shows Open PR when ahead of base with no PR yet", () => {
    render(<MergeButton wt={remoteAheadWt} />);
    expect(screen.getByText("Open PR")).toBeInTheDocument();
    expect(screen.queryByText("Merge to main")).not.toBeInTheDocument();
  });

  it("asks the active agent to prepare the PR on Open PR click", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    useStore.setState({
      selectedId: "corral/hulling",
      sessions: new Map([
        ["corral/hulling", { running: true, waiting: true } as never],
      ]),
      sessionMgr: { submitTo: vi.fn(async () => true) } as never,
    });
    render(<MergeButton wt={remoteAheadWt} />);
    fireEvent.click(screen.getByText("Open PR"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_text_attachment", {
        text: expect.stringContaining("Prepare a pull request"),
      }),
    );
  });

  it("disables duplicate requests while the agent is preparing the PR", () => {
    useStore.setState({
      preparingPr: new Map([["corral/hulling", { token: 1, kind: "create" }]]),
    });
    render(<MergeButton wt={remoteAheadWt} />);
    expect(screen.getByText("Preparing PR…").closest("button")).toBeDisabled();
  });

  it("allows the agent to decide how to handle dirty changes", () => {
    render(<MergeButton wt={remoteDirtyWt} />);
    const button = screen.getByText("Open PR");
    expect(button.closest("button")).not.toBeDisabled();
  });

  it("shows Push updates once a PR is open with unpushed commits", () => {
    const unpushedWt: Worktree = {
      ...remoteAheadWt,
      status: { ...remoteAheadStatus, ahead: 1 },
    };
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<MergeButton wt={unpushedWt} />);
    expect(screen.getByText("Push updates")).toBeInTheDocument();
    expect(screen.queryByText("Open PR")).not.toBeInTheDocument();
  });

  it("uses agent-driven updates instead of Merge PR when an open PR has dirty files", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<MergeButton wt={remoteDirtyWt} />);
    expect(screen.getByText("Push updates")).toBeInTheDocument();
    expect(screen.queryByText("Merge PR")).not.toBeInTheDocument();
  });

  it("shows Remove once the PR is merged", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", mergedPrInfo]]),
    });
    render(<MergeButton wt={remoteAheadWt} />);
    expect(screen.getByText("Remove")).toBeInTheDocument();
    expect(screen.queryByText("Push updates")).not.toBeInTheDocument();
    expect(screen.queryByText("Open PR")).not.toBeInTheDocument();
  });
});

describe("MergeButton Merge PR button (open PR, has_remote)", () => {
  // ahead: 0 (inherited from remoteAheadStatus) so mergeButtonState reaches
  // the merge_pr branch instead of push_updates, which takes priority
  // whenever there are unpushed commits.
  const openRemoteWt: Worktree = { ...wt, status: remoteAheadStatus };

  it("hides the Merge PR button and shows a waiting note while a check is pending", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
      checksByWtId: new Map([
        [
          "corral/hulling",
          [
            {
              name: "build",
              workflow: "ci.yml",
              bucket: "pending",
              url: "https://x/1",
            },
          ],
        ],
      ]),
    });
    render(<MergeButton wt={openRemoteWt} />);
    expect(screen.queryByText("Merge PR")).not.toBeInTheDocument();
    expect(screen.getByText("Waiting on checks…")).toBeInTheDocument();
  });

  it("hides the Merge PR button and shows a failed-checks note while a check is failing", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
      checksByWtId: new Map([
        [
          "corral/hulling",
          [
            {
              name: "test",
              workflow: "ci.yml",
              bucket: "fail",
              url: "https://x/2",
            },
          ],
        ],
      ]),
    });
    render(<MergeButton wt={openRemoteWt} />);
    expect(screen.queryByText("Merge PR")).not.toBeInTheDocument();
    expect(screen.getByText("Checks failed")).toBeInTheDocument();
  });

  it("reports a failure even when another required check is still pending", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
      checksByWtId: new Map([
        [
          "corral/hulling",
          [
            {
              name: "build",
              workflow: "ci.yml",
              bucket: "pending",
              url: "https://x/1",
            },
            {
              name: "test",
              workflow: "ci.yml",
              bucket: "fail",
              url: "https://x/2",
            },
          ],
        ],
      ]),
    });
    render(<MergeButton wt={openRemoteWt} />);
    expect(screen.getByText("Checks failed")).toBeInTheDocument();
    expect(screen.queryByText("Waiting on checks…")).not.toBeInTheDocument();
  });

  it("labels a cancelled check distinctly from a failure", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
      checksByWtId: new Map([
        [
          "corral/hulling",
          [
            {
              name: "deploy",
              workflow: "ci.yml",
              bucket: "cancel",
              url: "https://x/1",
            },
          ],
        ],
      ]),
    });
    render(<MergeButton wt={openRemoteWt} />);
    expect(screen.getByText("Checks cancelled")).toBeInTheDocument();
  });

  it("enables Merge PR once all checks pass", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
      checksByWtId: new Map([
        [
          "corral/hulling",
          [
            {
              name: "build",
              workflow: "ci.yml",
              bucket: "pass",
              url: "https://x/1",
            },
          ],
        ],
      ]),
    });
    render(<MergeButton wt={openRemoteWt} />);
    expect(screen.getByText("Merge PR").closest("button")).not.toBeDisabled();
  });

  it("enables Merge PR when no checks are configured yet", () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<MergeButton wt={openRemoteWt} />);
    expect(screen.getByText("Merge PR").closest("button")).not.toBeDisabled();
  });

  it("calls merge_pr on click", async () => {
    useStore.setState({
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    // Keyed by command name rather than call order: usePrChecksPolling's
    // background fetchPrChecks (fired on mount, since this PR is open) races
    // the click's merge_pr call, so a plain mockResolvedValueOnce queue
    // would non-deterministically hand the "merged" result to whichever
    // invoke call happens to land first.
    vi.mocked(invoke).mockImplementation((cmd) =>
      cmd === "merge_pr"
        ? Promise.resolve({ outcome: "merged" })
        : Promise.resolve([]),
    );
    render(<MergeButton wt={openRemoteWt} />);
    fireEvent.click(screen.getByText("Merge PR"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("merge_pr", {
        project: "corral",
        feature: "hulling",
      }),
    );
  });
});
