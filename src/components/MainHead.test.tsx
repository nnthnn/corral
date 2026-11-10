import { invoke } from "@tauri-apps/api/core";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { MainHead } from "./MainHead";

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

describe("MainHead", () => {
  it("renders nothing when no worktree is selected", () => {
    const { container } = render(<MainHead />);
    expect(container.querySelector(".crumb")).toBeNull();
  });

  it("shows a changes-pane toggle even when no worktree is selected", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    render(<MainHead />);
    fireEvent.click(screen.getByTitle("Toggle changes pane"));
    expect(useStore.getState().settings.changes_collapsed).toBe(true);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_settings", expect.anything()),
    );
  });

  it("shows a sidebar toggle even when no worktree is selected, and toggles sidebar_collapsed", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    render(<MainHead />);
    fireEvent.click(screen.getByTitle("Toggle sidebar"));
    expect(useStore.getState().settings.sidebar_collapsed).toBe(true);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("save_settings", expect.anything()),
    );
  });

  it("shows the crumb and Start button for a selected idle worktree, without the raw branch name", () => {
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    render(<MainHead />);
    expect(screen.getByText("hulling")).toBeInTheDocument();
    expect(screen.queryByText("feat/hulling")).not.toBeInTheDocument();
    expect(screen.getByTitle("Start agent")).toBeInTheDocument();
  });

  it("shows the title instead of the slug when a title is set", () => {
    useStore.setState({
      worktrees: [
        {
          ...wt,
          title: "Fix Login Timeout",
          slug: "fix-login-timeout",
        },
      ],
      selectedId: "corral/hulling",
    });
    render(<MainHead />);
    expect(screen.getByText("Fix Login Timeout")).toBeInTheDocument();
  });

  it("starts the agent on click", async () => {
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    vi.mocked(invoke).mockResolvedValueOnce({
      id: "corral/hulling",
      project: "corral",
      feature: "hulling",
      running: true,
      exit_code: null,
      killed: false,
      waiting: true,
      last_summary: null,
    });
    render(<MainHead />);
    fireEvent.click(screen.getByTitle("Start agent"));
    expect(invoke).toHaveBeenCalledWith("start_session", {
      project: "corral",
      feature: "hulling",
      fresh: false,
    });
  });

  it("shows Stop instead of Start when the session is running", () => {
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
    render(<MainHead />);
    expect(screen.getByTitle("Stop agent")).toBeInTheDocument();
    expect(screen.queryByTitle("Start agent")).not.toBeInTheDocument();
  });

  it("shows the Start/Resume button again a short while after stopping, even without the pointer ever leaving it", async () => {
    // Regression test: suppressStart used to clear only on `onMouseLeave`,
    // which left the button invisible forever if that event never fired —
    // the only escape was switching worktrees and back.
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
    vi.useFakeTimers();
    try {
      render(<MainHead />);
      expect(screen.getByTitle("Stop agent")).toBeInTheDocument();

      act(() => {
        useStore.setState({
          sessions: new Map([
            [
              "corral/hulling",
              {
                id: "corral/hulling",
                project: "corral",
                feature: "hulling",
                running: false,
                exit_code: 0,
                killed: true,
                waiting: false,
                last_summary: null,
              },
            ],
          ]),
        });
      });
      // Immediately after stopping, the real Start/Resume button is
      // suppressed (an invisible disabled placeholder sits in its place).
      expect(screen.queryByTitle("Start agent")).not.toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(400);
      });
      // No mouseleave ever fired — the timer alone must clear it.
      expect(screen.getByTitle("Start agent")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("renders Rename, Remove, New session, Zed, and Shell as icon-only buttons with tooltips", () => {
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    render(<MainHead />);
    expect(screen.getByTitle("Rename")).toBeInTheDocument();
    expect(screen.getByTitle("Remove")).toBeInTheDocument();
    expect(screen.getByTitle("New session")).toBeInTheDocument();
    expect(screen.getByTitle("Open in Zed")).toBeInTheDocument();
    expect(screen.getByTitle("Open terminal")).toBeInTheDocument();
    expect(screen.queryByText("Rename")).not.toBeInTheDocument();
    expect(screen.queryByText("Remove")).not.toBeInTheDocument();
    expect(screen.queryByText("New session")).not.toBeInTheDocument();
    expect(screen.queryByText("Zed")).not.toBeInTheDocument();
    expect(screen.queryByText("Shell")).not.toBeInTheDocument();
  });
});
