import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { AgentPane } from "./AgentPane";

const show = vi.fn();
const hideAll = vi.fn();
const has = vi.fn();
const refit = vi.fn();

vi.mock("../session", () => ({
  SessionManager: vi.fn().mockImplementation(function SessionManager() {
    return { show, hideAll, has, refit };
  }),
  THEME: {
    background: "#1a1510",
    foreground: "#ede8e0",
    cursor: "#c49a3a",
    selectionBackground: "rgba(237, 232, 224, 0.16)",
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

// jsdom has no real layout/canvas, so xterm.js can't render meaningfully
// here — `TerminalHost` is backed by the mocked session manager, and the
// Setup log terminal lives in its own tab now (see ProvisionPanel.test.tsx).
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    rows = 24;
    cols = 80;
    loadAddon() {}
    open() {}
    write() {}
    dispose() {}
    reset() {}
    attachCustomKeyEventHandler() {}
    unicode = { activeVersion: "6" };
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
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

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  show.mockReset();
  hideAll.mockReset();
  has.mockReset().mockReturnValue(false);
  refit.mockReset();
});

describe("AgentPane", () => {
  it("shows Detail's no-worktree-selected state when nothing is selected", () => {
    render(<AgentPane hidden={false} />);
    expect(screen.getByText("No worktree selected")).toBeInTheDocument();
  });

  it("shows Detail's no-agent-running state for a selected idle worktree", () => {
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    render(<AgentPane hidden={false} />);
    expect(screen.getByText("No agent in this pen.")).toBeInTheDocument();
  });

  it("hides Detail once a terminal is attached", () => {
    has.mockReturnValue(true);
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    render(<AgentPane hidden={false} />);
    expect(screen.queryByText("No agent in this pen.")).not.toBeInTheDocument();
  });

  it("hides the floating status chip while a session is running or waiting — the lasso tab spinner and sidebar cue cover that now", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
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
    render(<AgentPane hidden={false} />);
    expect(screen.queryByText("waiting at the gate")).not.toBeInTheDocument();
    expect(screen.queryByText("working")).not.toBeInTheDocument();
  });

  it("shows a floating status chip once a session has stopped", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      sessions: new Map([
        [
          "corral/hulling",
          {
            id: "corral/hulling",
            project: "corral",
            feature: "hulling",
            running: false,
            exit_code: 0,
            killed: false,
            waiting: false,
            last_summary: null,
          },
        ],
      ]),
    });
    render(<AgentPane hidden={false} />);
    expect(screen.getByText("done")).toBeInTheDocument();
  });

  it("shows no status chip when nothing is selected", () => {
    render(<AgentPane hidden={false} />);
    expect(
      screen.queryByText(/waiting at the gate|working|stopped|exited|done/),
    ).not.toBeInTheDocument();
  });

  it("keeps showing Detail's loading state, not a blank pane, between a session going running and its terminal attaching", () => {
    has.mockReturnValue(false);
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
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
    render(<AgentPane hidden={false} />);
    expect(screen.getByText("Starting…")).toBeInTheDocument();
  });
});
