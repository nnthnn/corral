import { beforeEach, describe, expect, it } from "vitest";
import type { SessionInfo } from "./session";
import {
  type AppState,
  currentWt,
  sessionStatus,
  useStore,
  wantsAttention,
  worktreeTabIds,
} from "./store";
import type { Worktree } from "./types";

function makeWorktree(overrides: Partial<Worktree> = {}): Worktree {
  return {
    project: "corral",
    feature: "hulling",
    slug: "hulling",
    branch: "feat/hulling",
    path: "/home/alice/source/worktrees/corral/hulling",
    status: null,
    has_resumable_session: false,
    note: "",
    ...overrides,
  };
}

function makeSession(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    id: "corral/hulling",
    project: "corral",
    feature: "hulling",
    running: true,
    exit_code: null,
    killed: false,
    waiting: true,
    last_summary: null,
    ...overrides,
  };
}

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
});

describe("currentWt", () => {
  it("finds the worktree matching selectedId", () => {
    const wt = makeWorktree();
    const state: AppState = {
      ...useStore.getState(),
      worktrees: [wt],
      selectedId: "corral/hulling",
    };
    expect(currentWt(state)).toBe(wt);
  });

  it("returns undefined when nothing is selected", () => {
    const state: AppState = {
      ...useStore.getState(),
      worktrees: [makeWorktree()],
      selectedId: null,
    };
    expect(currentWt(state)).toBeUndefined();
  });
});

describe("worktreeTabIds", () => {
  const polledOpenPr: Worktree["pr_state"] = {
    number: 7,
    url: "https://github.com/x/y/pull/7",
    state: "open",
    is_draft: false,
  };

  it("includes the PR tab from the polled pr_state before a live fetch has run", () => {
    const wt = makeWorktree({ pr_state: polledOpenPr });
    const state: AppState = {
      ...useStore.getState(),
      worktrees: [wt],
      selectedId: "corral/hulling",
    };
    expect(worktreeTabIds(state)).toContain("pr");
  });

  it("keeps the PR tab when only the polled pr_state knows the PR has merged", () => {
    const wt = makeWorktree({
      pr_state: { ...polledOpenPr, state: "merged" as const },
    });
    const state: AppState = {
      ...useStore.getState(),
      worktrees: [wt],
      selectedId: "corral/hulling",
    };
    expect(worktreeTabIds(state)).toContain("pr");
  });

  it("prefers a confirmed no-PR from the live store over the poll's pr_state", () => {
    const wt = makeWorktree({ pr_state: polledOpenPr });
    const state: AppState = {
      ...useStore.getState(),
      worktrees: [wt],
      selectedId: "corral/hulling",
      prInfoByWtId: new Map([["corral/hulling", null]]),
    };
    expect(worktreeTabIds(state)).not.toContain("pr");
  });
});

describe("sessionStatus", () => {
  it("returns null when there's no session", () => {
    const state = useStore.getState();
    expect(sessionStatus(state, "corral/hulling")).toBeNull();
  });

  it("reports stopped when killed", () => {
    const sessions = new Map([
      ["corral/hulling", makeSession({ running: false, killed: true })],
    ]);
    const state: AppState = { ...useStore.getState(), sessions };
    expect(sessionStatus(state, "corral/hulling")).toEqual({
      cls: "stopped",
      label: "stopped",
    });
  });

  it("reports done on a clean exit", () => {
    const sessions = new Map([
      ["corral/hulling", makeSession({ running: false, exit_code: 0 })],
    ]);
    const state: AppState = { ...useStore.getState(), sessions };
    expect(sessionStatus(state, "corral/hulling")).toEqual({
      cls: "done",
      label: "done",
    });
  });

  it("reports working when the session isn't waiting", () => {
    const sessions = new Map([
      ["corral/hulling", makeSession({ waiting: false })],
    ]);
    const state: AppState = { ...useStore.getState(), sessions };
    expect(sessionStatus(state, "corral/hulling")).toEqual({
      cls: "working",
      label: "working",
    });
  });

  it("reports waiting for input once quiet", () => {
    const sessions = new Map([["corral/hulling", makeSession()]]);
    const state: AppState = { ...useStore.getState(), sessions };
    expect(sessionStatus(state, "corral/hulling")).toEqual({
      cls: "waiting",
      label: "waiting at the gate",
    });
  });
});

describe("wantsAttention", () => {
  it("is true only when waiting and not the selected worktree", () => {
    const sessions = new Map([["corral/hulling", makeSession()]]);
    const state: AppState = {
      ...useStore.getState(),
      sessions,
      selectedId: "other",
    };
    expect(wantsAttention(state, "corral/hulling")).toBe(true);
  });

  it("is false when the waiting worktree is the selected one", () => {
    const sessions = new Map([["corral/hulling", makeSession()]]);
    const state: AppState = {
      ...useStore.getState(),
      sessions,
      selectedId: "corral/hulling",
    };
    expect(wantsAttention(state, "corral/hulling")).toBe(false);
  });

  it("is false once acknowledged, even after deselecting", () => {
    const sessions = new Map([["corral/hulling", makeSession()]]);
    const state: AppState = {
      ...useStore.getState(),
      sessions,
      selectedId: "other",
      acknowledgedWaiting: new Set(["corral/hulling"]),
    };
    expect(wantsAttention(state, "corral/hulling")).toBe(false);
  });
});
