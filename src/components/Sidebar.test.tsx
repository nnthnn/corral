import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { Sidebar } from "./Sidebar";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("0.1.0"),
}));

const wt = (project: string, feature: string): Worktree => ({
  project,
  feature,
  slug: feature,
  branch: `feat/${feature}`,
  path: `/path/${project}/${feature}`,
  status: null,
  has_resumable_session: false,
  note: "",
});

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset().mockResolvedValue({ base: null, files: [] });
});

describe("Sidebar", () => {
  it("renders a row per worktree, grouped by project", () => {
    useStore.setState({
      worktrees: [wt("corral", "hulling"), wt("corral", "churning")],
    });
    render(<Sidebar />);
    expect(screen.getByText("hulling")).toBeInTheDocument();
    expect(screen.getByText("churning")).toBeInTheDocument();
    expect(screen.getByText("corral")).toBeInTheDocument();
  });

  it("filters rows as you type", () => {
    useStore.setState({
      worktrees: [wt("corral", "hulling"), wt("corral", "churning")],
    });
    render(<Sidebar />);
    fireEvent.change(screen.getByPlaceholderText("Filter worktrees…"), {
      target: { value: "hull" },
    });
    expect(screen.getByText(/hull/)).toBeInTheDocument();
    expect(screen.queryByText("churning")).not.toBeInTheDocument();
  });

  it("selects a worktree on click", async () => {
    useStore.setState({ worktrees: [wt("corral", "hulling")] });
    render(<Sidebar />);
    fireEvent.click(screen.getByText("hulling"));
    expect(useStore.getState().selectedId).toBe("corral/hulling");
  });

  it("collapses a group via the .collapsed class (rows stay in the DOM, hidden by CSS)", () => {
    useStore.setState({ worktrees: [wt("corral", "hulling")] });
    render(<Sidebar />);
    const group = screen.getByText("corral").closest(".wt-group");
    expect(group).not.toHaveClass("collapsed");
    fireEvent.click(screen.getByText("corral"));
    expect(group).toHaveClass("collapsed");
    // Row is still in the DOM — `.wt-group.collapsed .wt-item { @apply hidden }`
    // in styles.css hides it visually, jsdom doesn't compute that layout.
    expect(screen.getByText("hulling")).toBeInTheDocument();
  });

  it("collapses a group via keyboard activation (Enter) on the focused row", () => {
    useStore.setState({ worktrees: [wt("corral", "hulling")] });
    render(<Sidebar />);
    const group = screen.getByText("corral").closest(".wt-group");
    const collapseTarget = screen
      .getByText("corral")
      .closest('[role="button"]');
    expect(collapseTarget).toHaveAttribute("tabIndex", "0");
    fireEvent.keyDown(collapseTarget as Element, { key: "Enter" });
    expect(group).toHaveClass("collapsed");
  });

  it("reorders projects via Alt+ArrowDown on the focused row, persisting the new order", () => {
    useStore.setState({ projects: ["corral", "typescript-sdk"] });
    const { container } = render(<Sidebar />);
    const collapseTarget = screen
      .getByText("corral")
      .closest('[role="button"]');
    fireEvent.keyDown(collapseTarget as Element, {
      key: "ArrowDown",
      altKey: true,
    });
    expect(useStore.getState().settings.project_order).toEqual([
      "typescript-sdk",
      "corral",
    ]);
    const groupNames = Array.from(
      container.querySelectorAll(".wt-group-head"),
    ).map((el) => el.textContent);
    expect(groupNames[0]).toContain("typescript-sdk");
    expect(groupNames[1]).toContain("corral");
  });

  it("keeps a project's row even once it has no active worktrees", () => {
    useStore.setState({
      worktrees: [wt("corral", "hulling")],
      projects: ["corral", "typescript-sdk"],
    });
    render(<Sidebar />);
    expect(screen.getByText("typescript-sdk")).toBeInTheDocument();
    expect(
      screen.getByText("typescript-sdk").closest(".wt-group"),
    ).not.toBeNull();
  });

  it("renders a resize handle when not collapsed", () => {
    render(<Sidebar />);
    expect(
      screen.getByTestId("resize-handle-sidebar_width"),
    ).toBeInTheDocument();
  });

  it("hides the resize handle when collapsed", () => {
    useStore.setState((s) => ({
      settings: { ...s.settings, sidebar_collapsed: true },
    }));
    render(<Sidebar />);
    expect(
      screen.queryByTestId("resize-handle-sidebar_width"),
    ).not.toBeInTheDocument();
  });

  it("shows the app version in the footer once it resolves", async () => {
    render(<Sidebar />);
    expect(await screen.findByText("v0.1.0")).toBeInTheDocument();
  });

  it("shows a running-session count in the header once a session is running", () => {
    useStore.setState({
      worktrees: [wt("corral", "hulling")],
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
    render(<Sidebar />);
    expect(screen.getByText("1 running")).toBeInTheDocument();
  });

  it("shows total memory once the poll returns a byte count", async () => {
    vi.mocked(invoke).mockResolvedValue(512 * 1024 * 1024);
    render(<Sidebar />);
    expect(await screen.findByText("512.0 MB")).toBeInTheDocument();
  });

  it("hides the memory counter when the backend reports none (non-Linux)", () => {
    vi.mocked(invoke).mockResolvedValue(null);
    render(<Sidebar />);
    expect(screen.queryByText(/MB|GB|KB|B$/)).not.toBeInTheDocument();
  });
});
