import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../session";
import { useStore } from "../store";
import type { Worktree } from "../types";
import {
  onSessionExit,
  onSessionOutput,
  onSessionStatus,
} from "./session-events";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
}));

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
  vi.mocked(invoke).mockReset().mockResolvedValue({ base: null, files: [] });
});

describe("onSessionOutput", () => {
  it("marks the worktree unread when it isn't selected", () => {
    useStore.setState({
      sessions: new Map([["corral/hulling", session()]]),
      selectedId: null,
    });
    onSessionOutput("corral/hulling");
    expect(useStore.getState().unreadOutput.has("corral/hulling")).toBe(true);
  });

  it("does not mark the selected worktree unread", () => {
    useStore.setState({
      sessions: new Map([["corral/hulling", session()]]),
      selectedId: "corral/hulling",
      worktrees: [wt],
    });
    onSessionOutput("corral/hulling");
    expect(useStore.getState().unreadOutput.has("corral/hulling")).toBe(false);
  });
});

describe("onSessionStatus", () => {
  it("updates the session's waiting flag", () => {
    useStore.setState({
      sessions: new Map([["corral/hulling", session({ waiting: true })]]),
    });
    onSessionStatus("corral/hulling", false);
    expect(useStore.getState().sessions.get("corral/hulling")?.waiting).toBe(
      false,
    );
  });

  it("clears notifiedWaiting when a session starts working again", () => {
    useStore.setState({
      sessions: new Map([["corral/hulling", session()]]),
      notifiedWaiting: new Set(["corral/hulling"]),
    });
    onSessionStatus("corral/hulling", false);
    expect(useStore.getState().notifiedWaiting.has("corral/hulling")).toBe(
      false,
    );
  });

  it("clears acknowledgedWaiting when a session starts working again, so the next waiting period re-alerts", () => {
    useStore.setState({
      sessions: new Map([["corral/hulling", session()]]),
      acknowledgedWaiting: new Set(["corral/hulling"]),
    });
    onSessionStatus("corral/hulling", false);
    expect(useStore.getState().acknowledgedWaiting.has("corral/hulling")).toBe(
      false,
    );
  });

  it("updates last_summary when a non-null summary is provided", () => {
    useStore.setState({
      sessions: new Map([["corral/hulling", session({ last_summary: null })]]),
    });
    onSessionStatus("corral/hulling", true, "Fixed the bug.");
    expect(
      useStore.getState().sessions.get("corral/hulling")?.last_summary,
    ).toBe("Fixed the bug.");
  });

  it("leaves the previous last_summary in place when none is provided", () => {
    useStore.setState({
      sessions: new Map([
        ["corral/hulling", session({ last_summary: "Old summary." })],
      ]),
    });
    onSessionStatus("corral/hulling", false);
    expect(
      useStore.getState().sessions.get("corral/hulling")?.last_summary,
    ).toBe("Old summary.");
  });

  it("clears PR preparation and refreshes PR state when the agent finishes", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      number: 7,
      url: "https://github.com/x/y/pull/7",
      state: "open",
      is_draft: false,
    });
    useStore.setState({
      worktrees: [
        {
          ...wt,
          status: { has_remote: true } as Worktree["status"],
        },
      ],
      sessions: new Map([["corral/hulling", session({ waiting: false })]]),
      preparingPr: new Map([["corral/hulling", { token: 1, kind: "create" }]]),
    });

    onSessionStatus("corral/hulling", true);

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("pr_status", {
        project: "corral",
        feature: "hulling",
      }),
    );
    await vi.waitFor(() =>
      expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false),
    );
  });

  it("does not poll GitHub after unrelated agent turns", () => {
    useStore.setState({
      worktrees: [
        { ...wt, status: { has_remote: true } as Worktree["status"] },
      ],
      sessions: new Map([["corral/hulling", session({ waiting: false })]]),
    });
    vi.mocked(invoke).mockClear();

    onSessionStatus("corral/hulling", true);

    expect(invoke).not.toHaveBeenCalledWith("pr_status", expect.anything());
  });

  it("keeps update preparation active until the branch is no longer ahead", async () => {
    const remoteStatus = {
      has_remote: true,
      ahead: 1,
      staged: 0,
      modified: 0,
      untracked: 0,
    } as Worktree["status"];
    useStore.setState({
      worktrees: [{ ...wt, status: remoteStatus }],
      sessions: new Map([["corral/hulling", session({ waiting: false })]]),
      preparingPr: new Map([["corral/hulling", { token: 4, kind: "update" }]]),
    });
    const pr = {
      number: 7,
      url: "https://github.com/x/y/pull/7",
      state: "open",
      is_draft: false,
    };
    vi.mocked(invoke)
      .mockResolvedValueOnce(pr)
      .mockResolvedValueOnce(remoteStatus);

    onSessionStatus("corral/hulling", true);
    // pr_status + refresh_worktree_status + the waiting notification.
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(3));
    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(true);

    onSessionStatus("corral/hulling", false);
    vi.mocked(invoke)
      .mockResolvedValueOnce(pr)
      .mockResolvedValueOnce({ ...remoteStatus, ahead: 0 });
    onSessionStatus("corral/hulling", true);
    await vi.waitFor(() =>
      expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false),
    );
  });

  it("does not complete creation from a closed historical PR", async () => {
    useStore.setState({
      worktrees: [
        {
          ...wt,
          status: { has_remote: true, ahead: 0 } as Worktree["status"],
        },
      ],
      sessions: new Map([["corral/hulling", session({ waiting: false })]]),
      preparingPr: new Map([["corral/hulling", { token: 5, kind: "create" }]]),
    });
    vi.mocked(invoke).mockResolvedValueOnce({
      number: 6,
      url: "https://github.com/x/y/pull/6",
      state: "closed",
      is_draft: false,
    });

    onSessionStatus("corral/hulling", true);
    // A "create" completion only consults PR status — no worktree-status call
    // (the extra invoke is the waiting notification `send_notification`).
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));

    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(true);
  });

  it("completes a create prep even when the cached worktree status reports no remote", async () => {
    useStore.setState({
      worktrees: [
        {
          ...wt,
          status: { has_remote: false, ahead: 0 } as Worktree["status"],
        },
      ],
      sessions: new Map([["corral/hulling", session({ waiting: false })]]),
      preparingPr: new Map([["corral/hulling", { token: 7, kind: "create" }]]),
    });
    vi.mocked(invoke).mockResolvedValueOnce({
      number: 7,
      url: "https://github.com/x/y/pull/7",
      state: "open",
      is_draft: false,
    });

    onSessionStatus("corral/hulling", true);
    await vi.waitFor(() =>
      expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false),
    );
  });

  it("completes a dirty-only update after clarification even when edits stay excluded", async () => {
    const dirtyStatus = {
      has_remote: true,
      ahead: 0,
      staged: 0,
      modified: 1,
      untracked: 0,
    } as Worktree["status"];
    const pr = {
      number: 7,
      url: "https://github.com/x/y/pull/7",
      state: "open",
      is_draft: false,
    };
    useStore.setState({
      worktrees: [{ ...wt, status: dirtyStatus }],
      sessions: new Map([["corral/hulling", session({ waiting: false })]]),
      preparingPr: new Map([
        [
          "corral/hulling",
          { token: 6, kind: "update", initialAhead: 0, waits: 0 },
        ],
      ]),
    });
    vi.mocked(invoke)
      .mockResolvedValueOnce(pr)
      .mockResolvedValueOnce(dirtyStatus);

    onSessionStatus("corral/hulling", true);
    await vi.waitFor(() =>
      expect(useStore.getState().preparingPr.get("corral/hulling")?.waits).toBe(
        1,
      ),
    );

    onSessionStatus("corral/hulling", false);
    vi.mocked(invoke)
      .mockResolvedValueOnce(pr)
      .mockResolvedValueOnce(dirtyStatus);
    onSessionStatus("corral/hulling", true);
    await vi.waitFor(() =>
      expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false),
    );
  });
});

describe("onSessionExit", () => {
  it("marks the session stopped and clears notifiedWaiting", () => {
    useStore.setState({
      sessions: new Map([["corral/hulling", session()]]),
      notifiedWaiting: new Set(["corral/hulling"]),
      preparingPr: new Map([["corral/hulling", { token: 1, kind: "create" }]]),
    });
    onSessionExit("corral/hulling", 0, false);
    const sess = useStore.getState().sessions.get("corral/hulling");
    expect(sess).toMatchObject({ running: false, exit_code: 0, killed: false });
    expect(useStore.getState().notifiedWaiting.has("corral/hulling")).toBe(
      false,
    );
    expect(useStore.getState().preparingPr.has("corral/hulling")).toBe(false);
  });

  it("reloads changes when the exited session is the selected worktree", async () => {
    useStore.setState({
      worktrees: [wt],
      sessions: new Map([["corral/hulling", session()]]),
      selectedId: "corral/hulling",
    });
    onSessionExit("corral/hulling", 1, false);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("branch_changes", {
        project: "corral",
        feature: "hulling",
      }),
    );
  });
});
