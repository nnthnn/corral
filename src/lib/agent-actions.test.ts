import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../session";
import { useStore } from "../store";
import type { Worktree } from "../types";
import {
  agentButtonLabel,
  openZed,
  restartAgent,
  restartFresh,
  startAgent,
  stopAgent,
} from "./agent-actions";

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
});

describe("startAgent", () => {
  it("starts a session and marks the worktree resumable", async () => {
    const info: SessionInfo = {
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
      waiting: true,
      last_summary: null,
    };
    useStore.setState({ worktrees: [wt] });
    vi.mocked(invoke).mockResolvedValueOnce(info);
    await startAgent(wt);
    expect(invoke).toHaveBeenCalledWith("start_session", {
      project: "corral",
      feature: "hulling",
      fresh: false,
    });
    expect(useStore.getState().sessions.get("corral/hulling")).toEqual(info);
    expect(useStore.getState().worktrees[0].has_resumable_session).toBe(true);
  });

  it("clears the terminal's cached size on a successful start, since every start_session is a fresh PTY", async () => {
    const info: SessionInfo = {
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
      waiting: true,
      last_summary: null,
    };
    const resetSizeCache = vi.fn();
    useStore.setState({
      worktrees: [wt],
      sessionMgr: { resetSizeCache } as never,
    });
    vi.mocked(invoke).mockResolvedValueOnce(info);
    await startAgent(wt);
    expect(resetSizeCache).toHaveBeenCalledWith("corral/hulling");
  });

  it("doesn't start a second session for an already-running worktree", async () => {
    useStore.setState({
      sessions: new Map([
        [
          "corral/hulling",
          {
            id: "corral/hulling",
            project: "corral",
            feature: "hulling",
            running: true,
            exit_code: null,
            killed: false,
            waiting: true,
            last_summary: null,
          },
        ],
      ]),
    });
    await startAgent(wt);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("shows a toast on failure", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("backend down"));
    await startAgent(wt);
    expect(useStore.getState().toasts.map((t) => t.message)).toContain(
      "Wouldn't start. Check your agent executable PATH. (Error: backend down)",
    );
  });

  it("tracks startingIds while the invoke is in flight, and clears it after", async () => {
    let resolveInvoke!: (info: SessionInfo) => void;
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInvoke = resolve;
      }),
    );
    const started = startAgent(wt);
    expect(useStore.getState().startingIds.has("corral/hulling")).toBe(true);
    resolveInvoke({
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
      waiting: true,
      last_summary: null,
    });
    await started;
    expect(useStore.getState().startingIds.has("corral/hulling")).toBe(false);
  });

  it("clears startingIds even when start_session fails", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("backend down"));
    await startAgent(wt);
    expect(useStore.getState().startingIds.has("corral/hulling")).toBe(false);
  });

  it("doesn't start a second session while one is already starting", async () => {
    const pending = new Promise<SessionInfo>(() => {});
    vi.mocked(invoke).mockReturnValueOnce(pending);
    void startAgent(wt);
    expect(useStore.getState().startingIds.has("corral/hulling")).toBe(true);
    await startAgent(wt);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});

describe("stopAgent", () => {
  it("delegates to sessionMgr.stop", async () => {
    const stop = vi.fn().mockResolvedValue(undefined);
    useStore.setState({ sessionMgr: { stop } as never });
    await stopAgent("corral/hulling");
    expect(stop).toHaveBeenCalledWith("corral/hulling");
  });
});

describe("restartAgent", () => {
  it("resumes the matching worktree's session", async () => {
    const info: SessionInfo = {
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
      waiting: true,
      last_summary: null,
    };
    useStore.setState({ worktrees: [wt] });
    vi.mocked(invoke).mockResolvedValueOnce(info);
    await restartAgent("corral/hulling");
    expect(invoke).toHaveBeenCalledWith("start_session", {
      project: "corral",
      feature: "hulling",
      fresh: false,
    });
  });

  it("no-ops when the id doesn't match a known worktree", async () => {
    useStore.setState({ worktrees: [] });
    await restartAgent("corral/hulling");
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("restartFresh", () => {
  it("starts a brand new session (fresh: true) for the matching worktree", async () => {
    const info: SessionInfo = {
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
      waiting: true,
      last_summary: null,
    };
    useStore.setState({ worktrees: [wt] });
    vi.mocked(invoke).mockResolvedValueOnce(info);
    await restartFresh("corral/hulling");
    expect(invoke).toHaveBeenCalledWith("start_session", {
      project: "corral",
      feature: "hulling",
      fresh: true,
    });
  });

  it("no-ops when the id doesn't match a known worktree", async () => {
    useStore.setState({ worktrees: [] });
    await restartFresh("corral/hulling");
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("openZed", () => {
  it("invokes open_in_zed with the configured editor command", async () => {
    useStore.setState((s) => ({
      settings: { ...s.settings, editor_command: "zed" },
    }));
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await openZed(wt);
    expect(invoke).toHaveBeenCalledWith("open_in_zed", {
      path: "/path",
      editorCommand: "zed",
    });
  });
});

describe("agentButtonLabel", () => {
  it("reads 'Resume agent' when resumable, 'Start agent' otherwise", () => {
    expect(agentButtonLabel(wt)).toBe("Start agent");
    expect(agentButtonLabel({ ...wt, has_resumable_session: true })).toBe(
      "Resume agent",
    );
  });
});
