import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { ChangesPane } from "./ChangesPane";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

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
  vi.mocked(openUrl).mockReset();
});

describe("ChangesPane", () => {
  it("shows nothing when no worktree is selected", () => {
    render(<ChangesPane />);
    expect(
      screen.queryByText("Clean. Nothing to show."),
    ).not.toBeInTheDocument();
  });

  it("shows a clean-tree message when there are no changed files", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
    });
    render(<ChangesPane />);
    expect(screen.getByText("Clean. Nothing to show.")).toBeInTheDocument();
    expect(screen.getByText("vs main")).toBeInTheDocument();
  });

  it("badges the Changes tab with the changed-file count", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 3,
            removed: 1,
          },
          {
            path: "src/lib.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 1,
            removed: 0,
          },
        ],
      },
    });
    render(<ChangesPane />);
    expect(screen.getByText("Changes").closest("button")?.textContent).toBe(
      "Changes2",
    );
  });

  it("renders a row per changed file and opens its diff on click", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    vi.mocked(invoke).mockResolvedValueOnce("diff content");
    render(<ChangesPane />);
    expect(screen.getByTitle("src/main.rs")).toBeInTheDocument();
    fireEvent.click(screen.getByText("main.rs"));
    expect(invoke).toHaveBeenCalledWith("file_diff", {
      project: "corral",
      feature: "hulling",
      path: "src/main.rs",
    });
  });

  it("opens the diff when clicking the trailing added/removed stat", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    vi.mocked(invoke).mockResolvedValueOnce("diff content");
    render(<ChangesPane />);
    // The header also shows a +3 total, so scope to the row's own stat.
    fireEvent.click(
      within(document.querySelector(".changes-list") as HTMLElement).getByText(
        "+3",
      ),
    );
    expect(invoke).toHaveBeenCalledWith("file_diff", {
      project: "corral",
      feature: "hulling",
      path: "src/main.rs",
    });
  });

  it("copies the file path without opening the diff", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    render(<ChangesPane />);
    fireEvent.click(screen.getByTitle("Copy path: src/main.rs"));
    expect(writeText).toHaveBeenCalledWith("src/main.rs");
    expect(invoke).not.toHaveBeenCalledWith("file_diff", expect.anything());
  });

  it("discards a file only after an inline confirmation", async () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    vi.mocked(invoke).mockResolvedValue(undefined);
    render(<ChangesPane />);

    fireEvent.click(screen.getByTitle("Discard changes: src/main.rs"));
    expect(screen.getByText("Discard changes?")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("revert_file", expect.anything());

    fireEvent.click(screen.getByText("Discard"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("revert_file", {
        project: "corral",
        feature: "hulling",
        path: "src/main.rs",
        oldPath: null,
      }),
    );
  });

  it("closes an open discard confirmation when the worktree changes", () => {
    const other: Worktree = { ...wt, feature: "churning" };
    useStore.setState({
      worktrees: [wt, other],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    render(<ChangesPane />);
    fireEvent.click(screen.getByTitle("Discard changes: src/main.rs"));
    expect(screen.getByText("Discard changes?")).toBeInTheDocument();

    act(() => useStore.setState({ selectedId: "corral/churning" }));

    expect(screen.queryByText("Discard changes?")).not.toBeInTheDocument();
  });

  it("does not offer discard for a file with no uncommitted changes", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "pushed",
            dirty: false,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    render(<ChangesPane />);
    expect(
      screen.queryByTitle("Discard changes: src/main.rs"),
    ).not.toBeInTheDocument();
  });

  it("offers discard for a pushed file that has local edits", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "pushed",
            dirty: true,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    render(<ChangesPane />);
    expect(
      screen.getByTitle("Discard changes: src/main.rs"),
    ).toBeInTheDocument();
  });

  it("summarizes total added/removed lines across all files", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "pushed",
            dirty: false,
            added: 3,
            removed: 1,
          },
          {
            path: "src/lib.rs",
            status: "modified",
            old_path: null,
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 1,
            removed: 0,
          },
        ],
      },
    });
    render(<ChangesPane />);
    // 3 + 1 added across both files; the header total is distinct from the
    // per-row stats (which show +3, +1).
    expect(screen.getByText("+4")).toBeInTheDocument();
    // removed total is 1 — same value as the first row's own stat.
    expect(screen.getAllByText("-1")).toHaveLength(2);
    expect(
      screen.getByTitle(
        "Total lines changed across 2 files vs main — committed and uncommitted",
      ),
    ).toBeInTheDocument();
  });

  it("warns about conflicted files at the top and replaces the sync icon", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/main.rs",
            status: "conflict",
            old_path: null,
            staged: true,
            sync_state: "uncommitted",
            dirty: true,
            added: 3,
            removed: 1,
          },
        ],
      },
    });
    render(<ChangesPane />);
    expect(
      screen.getByText("1 conflict — resolve before continuing"),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("Merge conflict — resolve before continuing"),
    ).toBeInTheDocument();
    // The conflict marker takes the sync icon's place in the row.
    expect(document.querySelector(".change-sync")).toBeNull();
  });

  it("shows old → new filenames for a same-directory rename, with the shared dir shown once", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/components/new.tsx",
            status: "renamed",
            old_path: "src/components/old.tsx",
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 0,
            removed: 0,
          },
        ],
      },
    });
    render(<ChangesPane />);
    expect(
      screen.getByTitle("src/components/old.tsx → src/components/new.tsx"),
    ).toBeInTheDocument();
    expect(screen.getByText("old.tsx")).toBeInTheDocument();
    expect(screen.getByText("new.tsx")).toBeInTheDocument();
    // The shared directory should appear exactly once, not repeated.
    expect(screen.getAllByText("src/components/")).toHaveLength(1);
  });

  it("falls back to full old/new paths when the directory itself changed", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: {
        base: "main",
        files: [
          {
            path: "src/utils/newHelpers.ts",
            status: "renamed",
            old_path: "src/lib/oldutils.ts",
            staged: false,
            sync_state: "uncommitted",
            dirty: true,
            added: 0,
            removed: 0,
          },
        ],
      },
    });
    render(<ChangesPane />);
    expect(screen.getByText("src/lib/oldutils.ts")).toBeInTheDocument();
    expect(screen.getByText("src/utils/newHelpers.ts")).toBeInTheDocument();
  });

  it("renders a resize handle when not collapsed", () => {
    render(<ChangesPane />);
    expect(
      screen.getByTestId("resize-handle-changes_width"),
    ).toBeInTheDocument();
  });

  it("hides the resize handle when collapsed", () => {
    useStore.setState((s) => ({
      settings: { ...s.settings, changes_collapsed: true },
    }));
    render(<ChangesPane />);
    expect(
      screen.queryByTestId("resize-handle-changes_width"),
    ).not.toBeInTheDocument();
  });

  it("defaults to the Changes view", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
    });
    render(<ChangesPane />);
    expect(screen.getByText("Clean. Nothing to show.")).toBeInTheDocument();
  });

  it("switches to the Files view and loads the root listing on click", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
    });
    vi.mocked(invoke).mockResolvedValueOnce([
      { name: "README.md", path: "README.md", is_dir: false },
    ]);
    render(<ChangesPane />);
    fireEvent.click(screen.getByText("Files"));
    expect(useStore.getState().changesPanelTab).toBe("files");
    expect(invoke).toHaveBeenCalledWith("list_worktree_dir", {
      project: "corral",
      feature: "hulling",
      path: "",
    });
  });

  it("switching back to Changes hides the file tree", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
      changesPanelTab: "files",
      fileTree: new Map([["corral/hulling:", []]]),
    });
    render(<ChangesPane />);
    fireEvent.click(screen.getByText("Changes"));
    expect(useStore.getState().changesPanelTab).toBe("changes");
    expect(screen.getByText("Clean. Nothing to show.")).toBeInTheDocument();
  });
});

describe("ChangesPane PR button", () => {
  const openPrInfo = {
    number: 7,
    url: "https://github.com/x/y/pull/7",
    state: "open" as const,
    is_draft: false,
  };

  it("shows no PR button when the worktree has no known PR", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
    });
    render(<ChangesPane />);
    expect(screen.queryByText(/^PR #/)).not.toBeInTheDocument();
  });

  it("shows a button labeled with the PR number once a PR exists", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<ChangesPane />);
    expect(screen.getByText("PR #7")).toBeInTheDocument();
  });

  it("opens the PR tab on click", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      activeTab: "agent",
      changes: { base: "main", files: [] },
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<ChangesPane />);
    fireEvent.click(screen.getByText("PR #7"));
    expect(useStore.getState().activeTab).toBe("pr");
  });

  it("shows an open-in-browser link next to the PR button", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<ChangesPane />);
    expect(screen.getByTitle("Open PR in browser")).toBeInTheDocument();
  });

  it("opens the PR url in the browser from the link", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<ChangesPane />);
    fireEvent.click(screen.getByTitle("Open PR in browser"));
    expect(vi.mocked(openUrl)).toHaveBeenCalledWith(openPrInfo.url);
  });

  it("shows no open-in-browser link without a PR", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
    });
    render(<ChangesPane />);
    expect(screen.queryByTitle("Open PR in browser")).not.toBeInTheDocument();
  });

  it("lays out PR controls, the blocked-checks note, then actions", () => {
    const remoteWt: Worktree = {
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
    useStore.setState({
      worktrees: [remoteWt],
      selectedId: "corral/hulling",
      changes: { base: "main", files: [] },
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
    render(<ChangesPane />);
    const header = document.querySelector(".changes-actions");
    expect(header).not.toBeNull();
    const groups = Array.from(header?.children ?? []);
    expect(groups).toHaveLength(3);
    expect(groups[0].className).toContain("changes-pr-controls");
    expect(groups[1].className).toContain("changes-action-note");
    expect(groups[2].className).toContain("changes-action-buttons");

    const text = header?.textContent ?? "";
    const prAt = text.indexOf("PR #7");
    const noteAt = text.indexOf("Waiting on checks…");
    expect(prAt).toBeGreaterThanOrEqual(0);
    expect(noteAt).toBeGreaterThanOrEqual(0);
    expect(prAt).toBeLessThan(noteAt);
  });
});
