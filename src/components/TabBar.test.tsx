import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { TabBar } from "./TabBar";

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
});

describe("TabBar", () => {
  it("shows no tabs when nothing is selected", () => {
    render(<TabBar />);
    expect(screen.queryByText("Agent")).not.toBeInTheDocument();
  });

  it("shows Agent and Run tabs when a worktree is selected", () => {
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    render(<TabBar />);
    expect(screen.getByText("Agent")).toBeInTheDocument();
    expect(screen.getByText("Run")).toBeInTheDocument();
  });

  it("shows a closable diff tab labeled 'Diff'", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      diffTab: { path: "src/main.rs" },
      activeTab: "diff",
    });
    render(<TabBar />);
    expect(screen.getByText("Diff")).toBeInTheDocument();
  });

  it("switches the active tab on click", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      diffTab: { path: "src/main.rs" },
      activeTab: "diff",
    });
    render(<TabBar />);
    fireEvent.click(screen.getByText("Agent"));
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("switches the active tab when clicking the tab container, not just the label", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      diffTab: { path: "src/main.rs" },
      activeTab: "diff",
    });
    render(<TabBar />);
    const agentTab = screen.getByText("Agent").closest('[role="button"]');
    expect(agentTab).not.toBeNull();
    fireEvent.click(agentTab as Element);
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("does not activate the tab when Enter is pressed on its close button", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      diffTab: { path: "src/main.rs" },
      activeTab: "agent",
    });
    render(<TabBar />);
    const close = screen.getByTitle("Close tab");
    close.focus();
    fireEvent.keyDown(close, { key: "Enter" });
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("closes the diff tab and reverts to Agent", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      diffTab: { path: "src/main.rs" },
      activeTab: "diff",
    });
    render(<TabBar />);
    fireEvent.click(screen.getByTitle("Close tab"));
    expect(useStore.getState().diffTab).toBeNull();
    expect(useStore.getState().activeTab).toBe("agent");
  });

  it("always shows a Run tab once a worktree is selected, even with no diff tab open", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      diffTab: null,
    });
    render(<TabBar />);
    expect(screen.getByText("Run")).toBeInTheDocument();
  });

  it("always shows a Setup tab, even with no provisioning history", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
    });
    render(<TabBar />);
    expect(screen.getByText("Setup")).toBeInTheDocument();
  });

  it("shows a closable file tab labeled 'File'", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      fileTab: { path: "README.md" },
      activeTab: "file",
    });
    render(<TabBar />);
    expect(screen.getByText("File")).toBeInTheDocument();
  });

  it("closes the file tab and reverts to Agent", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      fileTab: { path: "README.md" },
      activeTab: "file",
    });
    render(<TabBar />);
    fireEvent.click(screen.getByTitle("Close tab"));
    expect(useStore.getState().fileTab).toBeNull();
    expect(useStore.getState().activeTab).toBe("agent");
  });
});

const openPrInfo = {
  number: 7,
  url: "https://github.com/x/y/pull/7",
  state: "open" as const,
  is_draft: false,
};

describe("TabBar PR tab", () => {
  it("does not show a PR tab when there's no PR", () => {
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    render(<TabBar />);
    expect(screen.queryByText(/^PR #/)).not.toBeInTheDocument();
  });

  it("shows a PR tab labeled with the PR number once a PR exists", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<TabBar />);
    expect(screen.getByText("PR #7")).toBeInTheDocument();
  });

  it("switches to the PR tab on click", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      prInfoByWtId: new Map([["corral/hulling", openPrInfo]]),
    });
    render(<TabBar />);
    fireEvent.click(screen.getByText("PR #7"));
    expect(useStore.getState().activeTab).toBe("pr");
  });

  it("shows a lasso spinner next to the PR tab label while a check is pending", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
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
    render(<TabBar />);
    const prButton = screen.getByText("PR #7").closest('[role="button"]');
    expect(prButton?.querySelector("svg")).toBeInTheDocument();
  });

  it("does not show a lasso spinner next to the PR tab label once all checks are resolved", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
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
    render(<TabBar />);
    const prButton = screen.getByText("PR #7").closest('[role="button"]');
    expect(prButton?.querySelector("svg")).not.toBeInTheDocument();
  });
});

describe("TabBar Run tab", () => {
  it("shows a spinner next to the Run tab label while the run is active", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      run: new Map([
        ["corral/hulling", { output: [], running: true, exit_code: null }],
      ]),
    });
    render(<TabBar />);
    const runButton = screen.getByText("Run").closest('[role="button"]');
    expect(runButton?.querySelector("svg")).toBeInTheDocument();
  });

  it("does not show a spinner next to the Run tab label once the run stops", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      run: new Map([
        ["corral/hulling", { output: [], running: false, exit_code: 0 }],
      ]),
    });
    render(<TabBar />);
    const runButton = screen.getByText("Run").closest('[role="button"]');
    expect(runButton?.querySelector("svg")).not.toBeInTheDocument();
  });
});

describe("TabBar Setup tab", () => {
  it("shows a spinner next to the Setup tab label while setup is running", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      provisioning: new Map([
        ["corral/hulling", { output: [], running: true, exit_code: null }],
      ]),
    });
    render(<TabBar />);
    const setupButton = screen.getByText("Setup").closest('[role="button"]');
    expect(setupButton?.querySelector("svg")).toBeInTheDocument();
  });

  it("does not show a spinner next to the Setup tab once setup has finished", () => {
    useStore.setState({
      worktrees: [wt],
      selectedId: "corral/hulling",
      provisioning: new Map([
        ["corral/hulling", { output: [], running: false, exit_code: 0 }],
      ]),
    });
    render(<TabBar />);
    const setupButton = screen.getByText("Setup").closest('[role="button"]');
    expect(setupButton?.querySelector("svg")).not.toBeInTheDocument();
  });
});
