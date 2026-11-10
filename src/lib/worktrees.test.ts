import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../session";
import { useStore } from "../store";
import type { Worktree } from "../types";
import * as data from "./data";
import * as toasts from "./toasts";
import {
  askAgentToPreparePullRequest,
  createWorktree,
  fetchPrStatus,
  focusFirstProjectCreate,
  mergeWorktree,
  openDeleteDialog,
  openRenameDialog,
  rebaseWorktree,
  toggleGroup,
} from "./worktrees";

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

const session = (overrides: Partial<SessionInfo> = {}): SessionInfo => ({
  id: "corral/hulling",
  project: "corral",
  feature: "hulling",
  running: true,
  exit_code: null,
  killed: false,
  waiting: true,
  last_summary: null,
  ...overrides,
});

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(data.loadAll).mockReset().mockResolvedValue(undefined);
});

describe("askAgentToPreparePullRequest", () => {
  function setRunningAgent(submitTo = vi.fn(async () => true)) {
    useStore.setState({
      selectedId: "corral/hulling",
      sessions: new Map([["corral/hulling", session()]]),
      sessionMgr: { submitTo } as never,
    });
    return submitTo;
  }

  it("sends an attachment-backed prompt to the intended session", async () => {
    const submitTo = setRunningAgent();
    vi.mocked(invoke).mockResolvedValueOnce("/tmp/pr-instructions.md");

    await askAgentToPreparePullRequest(wt);

    expect(submitTo).toHaveBeenCalledWith(
      "corral/hulling",
      expect.stringContaining('/tmp/pr-instructions.md"'),
    );
    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(true);
    expect(invoke).toHaveBeenCalledWith("save_text_attachment", {
      text: expect.stringContaining("Fetch `origin`"),
    });
  });

  it("does not redirect the prompt if selection changes while saving the attachment", async () => {
    let resolveAttachment: (path: string) => void = () => {};
    const submitTo = setRunningAgent();
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveAttachment = resolve as (path: string) => void;
        }),
    );

    const request = askAgentToPreparePullRequest(wt);
    useStore.setState({ selectedId: "corral/milking" });
    resolveAttachment("/tmp/pr-instructions.md");
    await request;

    expect(submitTo).not.toHaveBeenCalled();
    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false);
  });

  it("ignores repeated requests while preparation is active", async () => {
    setRunningAgent();
    let resolveAttachment: (path: string) => void = () => {};
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveAttachment = resolve as (path: string) => void;
        }),
    );

    const first = askAgentToPreparePullRequest(wt);
    const second = askAgentToPreparePullRequest(wt);
    expect(invoke).toHaveBeenCalledTimes(1);
    resolveAttachment("/tmp/pr-instructions.md");
    await Promise.all([first, second]);
  });

  it("does not queue PR preparation behind an agent's current task", async () => {
    useStore.setState({
      selectedId: "corral/hulling",
      sessions: new Map([["corral/hulling", session({ waiting: false })]]),
      sessionMgr: { submitTo: vi.fn(async () => true) } as never,
    });

    await askAgentToPreparePullRequest(wt);

    expect(invoke).not.toHaveBeenCalled();
    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false);
  });

  it("clears preparation when the intended terminal is unavailable", async () => {
    setRunningAgent(vi.fn(async () => false));
    vi.mocked(invoke).mockResolvedValueOnce("/tmp/pr-instructions.md");

    await askAgentToPreparePullRequest(wt);

    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false);
  });

  it("does not let an old failed submission clear a newer request", async () => {
    let resolveFirst: (sent: boolean) => void = () => {};
    const submitTo = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockResolvedValueOnce(true);
    setRunningAgent(submitTo);
    vi.mocked(invoke).mockResolvedValue("/tmp/pr-instructions.md");

    const first = askAgentToPreparePullRequest(wt);
    await vi.waitFor(() => expect(submitTo).toHaveBeenCalledTimes(1));
    useStore.setState({ preparingPr: new Map() });
    const second = askAgentToPreparePullRequest(wt);
    await vi.waitFor(() => expect(submitTo).toHaveBeenCalledTimes(2));

    resolveFirst(false);
    await Promise.all([first, second]);
    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(true);
  });
});

describe("createWorktree", () => {
  it("creates the worktree, selects it, and reloads", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(wt);
    const result = await createWorktree("corral");
    expect(invoke).toHaveBeenCalledWith("create_worktree", {
      project: "corral",
      feature: null,
    });
    expect(useStore.getState().selectedId).toBe("corral/hulling");
    expect(data.loadAll).toHaveBeenCalled();
    expect(result).toBe(wt);
  });

  it("resets activeTab, diffTab, and fileTab left over from the previously selected worktree", async () => {
    useStore.setState({
      selectedId: "corral/other",
      activeTab: "pr",
      diffTab: { path: "some/file.ts" },
      fileTab: { path: "some/file.ts" },
    });
    vi.mocked(invoke).mockResolvedValueOnce(wt);

    await createWorktree("corral");

    expect(useStore.getState().activeTab).toBe("agent");
    expect(useStore.getState().diffTab).toBeNull();
    expect(useStore.getState().fileTab).toBeNull();
  });
});

describe("focusFirstProjectCreate", () => {
  it("targets the first project in sidebar order", () => {
    useStore.setState({
      projects: ["zeta", "corral"],
      settings: {
        ...useStore.getState().settings,
        project_order: ["corral", "zeta"],
      },
    });
    focusFirstProjectCreate();
    expect(useStore.getState().worktreeNudgeTarget).toBe("corral");
  });

  it("expands the sidebar if collapsed", () => {
    useStore.setState({
      projects: ["corral"],
      settings: { ...useStore.getState().settings, sidebar_collapsed: true },
    });
    focusFirstProjectCreate();
    expect(useStore.getState().settings.sidebar_collapsed).toBe(false);
  });

  it("does nothing when there are no projects", () => {
    useStore.setState({ projects: [] });
    focusFirstProjectCreate();
    expect(useStore.getState().worktreeNudgeTarget).toBeNull();
  });
});

describe("openDeleteDialog", () => {
  it("refuses when the agent is still running", async () => {
    useStore.setState({ sessions: new Map([["corral/hulling", session()]]) });
    await openDeleteDialog(wt);
    expect(useStore.getState().toasts.map((t) => t.message)).toContain(
      "Stop the agent before removing this worktree.",
    );
    expect(useStore.getState().deleteTarget).toBeNull();
  });

  it("refreshes and sets deleteTarget when no agent is running", async () => {
    useStore.setState({ worktrees: [wt] });
    await openDeleteDialog(wt);
    expect(data.loadAll).toHaveBeenCalled();
    expect(useStore.getState().deleteTarget).toBe(wt);
  });
});

describe("openRenameDialog", () => {
  it("sets renameTarget", () => {
    openRenameDialog(wt);
    expect(useStore.getState().renameTarget).toBe(wt);
  });
});

describe("toggleGroup", () => {
  it("adds then removes a project from collapsedGroups, persisting each time", () => {
    toggleGroup("corral");
    expect(useStore.getState().collapsedGroups.has("corral")).toBe(true);
    expect(useStore.getState().settings.collapsed_groups).toEqual(["corral"]);
    toggleGroup("corral");
    expect(useStore.getState().collapsedGroups.has("corral")).toBe(false);
    expect(useStore.getState().settings.collapsed_groups).toEqual([]);
  });
});

describe("mergeWorktree", () => {
  it("merges, shows a success toast, marks justMerged, and reloads on success", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      outcome: "merged",
      pushed: true,
    });
    await mergeWorktree(wt);
    expect(invoke).toHaveBeenCalledWith("merge_worktree", {
      project: "corral",
      feature: "hulling",
    });
    const toasts = useStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].variant).toBe("success");
    expect(toasts[0].message).toContain("pushed");
    expect(useStore.getState().justMergedIds.has("corral/hulling")).toBe(true);
    expect(data.loadAll).toHaveBeenCalled();
  });

  it("shows a success toast without 'pushed' when not pushed", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      outcome: "merged",
      pushed: false,
    });
    await mergeWorktree(wt);
    const toasts = useStore.getState().toasts;
    expect(toasts[0].message).not.toContain("pushed");
  });

  it("surfaces a conflict as an error toast and still reloads", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ outcome: "conflict" });
    await mergeWorktree(wt);
    expect(useStore.getState().toasts.map((t) => t.message)).toEqual([
      expect.stringContaining("same ground"),
    ]);
    expect(data.loadAll).toHaveBeenCalled();
  });

  it("surfaces not_on_base_branch with both branch names in the message", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      outcome: "not_on_base_branch",
      current: "wip",
      base: "main",
    });
    await mergeWorktree(wt);
    const messages = useStore.getState().toasts.map((t) => t.message);
    expect(messages[0]).toContain("wip");
    expect(messages[0]).toContain("main");
  });

  it("shows an error toast and still reloads when invoke rejects", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("network error"));
    await mergeWorktree(wt);
    const messages = useStore.getState().toasts.map((t) => t.message);
    expect(messages[0]).toContain("network error");
    expect(data.loadAll).toHaveBeenCalled();
  });

  it("guards against double-clicking via mergingIds", async () => {
    let resolveInvoke: (v: unknown) => void = () => {};
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInvoke = resolve;
      }),
    );
    const promise = mergeWorktree(wt);
    expect(useStore.getState().mergingIds.has("corral/hulling")).toBe(true);
    resolveInvoke({ outcome: "merged", pushed: false });
    await promise;
    expect(useStore.getState().mergingIds.has("corral/hulling")).toBe(false);
  });

  it("ignores a re-entrant call while one is already in flight", async () => {
    let resolveInvoke: (v: unknown) => void = () => {};
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInvoke = resolve;
      }),
    );
    const first = mergeWorktree(wt);
    const second = mergeWorktree(wt);
    expect(invoke).toHaveBeenCalledTimes(1);
    resolveInvoke({ outcome: "merged", pushed: false });
    await Promise.all([first, second]);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});

describe("rebaseWorktree", () => {
  it("rebases, shows a success toast, and reloads on success", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ outcome: "rebased" });
    await rebaseWorktree(wt);
    expect(invoke).toHaveBeenCalledWith("rebase_worktree", {
      project: "corral",
      feature: "hulling",
    });
    const toasts = useStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].variant).toBe("success");
    expect(toasts[0].message).toContain("Rebased");
    expect(data.loadAll).toHaveBeenCalled();
  });

  it("surfaces a conflict as an error toast and still reloads", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ outcome: "conflict" });
    await rebaseWorktree(wt);
    expect(useStore.getState().toasts.map((t) => t.message)).toEqual([
      expect.stringContaining("conflict"),
    ]);
    expect(data.loadAll).toHaveBeenCalled();
  });

  it("surfaces dirty as an error toast", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ outcome: "dirty" });
    await rebaseWorktree(wt);
    expect(useStore.getState().toasts.map((t) => t.message)).toEqual([
      expect.stringContaining("Commit or stash"),
    ]);
  });

  it("surfaces no_base_branch as an error toast", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ outcome: "no_base_branch" });
    await rebaseWorktree(wt);
    expect(useStore.getState().toasts.map((t) => t.message)).toEqual([
      expect.stringContaining("base branch"),
    ]);
  });

  it("shows an error toast and still reloads when invoke rejects", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("network error"));
    await rebaseWorktree(wt);
    const messages = useStore.getState().toasts.map((t) => t.message);
    expect(messages[0]).toContain("network error");
    expect(data.loadAll).toHaveBeenCalled();
  });

  it("guards against double-clicking via rebasingIds", async () => {
    let resolveInvoke: (v: unknown) => void = () => {};
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInvoke = resolve;
      }),
    );
    const promise = rebaseWorktree(wt);
    expect(useStore.getState().rebasingIds.has("corral/hulling")).toBe(true);
    resolveInvoke({ outcome: "rebased" });
    await promise;
    expect(useStore.getState().rebasingIds.has("corral/hulling")).toBe(false);
  });

  it("ignores a re-entrant call while one is already in flight", async () => {
    let resolveInvoke: (v: unknown) => void = () => {};
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInvoke = resolve;
      }),
    );
    const first = rebaseWorktree(wt);
    const second = rebaseWorktree(wt);
    expect(invoke).toHaveBeenCalledTimes(1);
    resolveInvoke({ outcome: "rebased" });
    await Promise.all([first, second]);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});

describe("fetchPrStatus", () => {
  it("stores the fetched PrInfo keyed by wtId", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      number: 7,
      url: "https://github.com/x/y/pull/7",
      state: "open",
      is_draft: false,
    });
    fetchPrStatus(wt);
    await vi.waitFor(() =>
      expect(useStore.getState().prInfoByWtId.get("corral/hulling")).toEqual({
        number: 7,
        url: "https://github.com/x/y/pull/7",
        state: "open",
        is_draft: false,
      }),
    );
  });

  it("stores null when no PR exists yet", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    fetchPrStatus(wt);
    await vi.waitFor(() =>
      expect(useStore.getState().prInfoByWtId.get("corral/hulling")).toBeNull(),
    );
  });

  it("does not show an error toast when the fetch fails — this runs automatically, not from a user action", async () => {
    const showErrorSpy = vi.spyOn(toasts, "showError");
    vi.mocked(invoke).mockRejectedValueOnce(new Error("network error"));
    fetchPrStatus(wt);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalled());
    // Flush the rejected promise's microtask queue before asserting.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(showErrorSpy).not.toHaveBeenCalled();
  });
});
