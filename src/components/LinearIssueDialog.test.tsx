import { invoke } from "@tauri-apps/api/core";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { LinearIssue, LinearIssueSummary } from "../types";
import { LinearIssueDialog } from "./LinearIssueDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const summary: LinearIssueSummary = {
  id: "uuid-1",
  identifier: "ENG-123",
  title: "Fix login timeout",
  url: "https://linear.app/acme/issue/ENG-123",
  state: "In Progress",
  team: "Engineering",
  project: "Q3 Auth",
  state_color: "#f2c94c",
  project_color: "#5e6ad2",
};

const recentSummary: LinearIssueSummary = {
  id: "uuid-2",
  identifier: "ENG-119",
  title: "Recently touched ticket",
  url: "https://linear.app/acme/issue/ENG-119",
  state: "Todo",
  team: "Engineering",
  project: null,
  state_color: "#e2e2e2",
  project_color: null,
};

const issue: LinearIssue = {
  ...summary,
  description: "Sessions expire too early.",
  assignee: "Ada",
  priority: "High",
  labels: ["bug"],
};

function openWithKey(): void {
  useStore.setState((s) => ({
    linearDialogOpen: true,
    settings: { ...s.settings, linear_api_key_set: true },
  }));
}

function mockLinear(
  opts: {
    recent?: LinearIssueSummary[];
    search?: LinearIssueSummary[];
    issue?: LinearIssue;
  } = {},
): void {
  vi.mocked(invoke).mockImplementation(((cmd: string) => {
    if (cmd === "linear_recent") return Promise.resolve(opts.recent ?? []);
    if (cmd === "linear_search") return Promise.resolve(opts.search ?? []);
    if (cmd === "linear_issue") return Promise.resolve(opts.issue ?? issue);
    if (cmd === "save_text_attachment")
      return Promise.resolve("/tmp/ticket.txt");
    return Promise.resolve(undefined);
  }) as typeof invoke);
}

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
});

describe("LinearIssueDialog", () => {
  it("points at Settings when no API key is configured", () => {
    useStore.setState({ linearDialogOpen: true });
    render(<LinearIssueDialog />);

    expect(screen.getByText("Linear API key needed")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Open Settings"));
    expect(useStore.getState().linearDialogOpen).toBe(false);
    expect(useStore.getState().settingsOpen).toBe(true);
  });

  it("shows recently updated tickets when it opens", async () => {
    mockLinear({ recent: [recentSummary] });
    openWithKey();
    render(<LinearIssueDialog />);

    expect(
      await screen.findByText("Recently touched ticket"),
    ).toBeInTheDocument();
    expect(screen.getByText("Recent tickets")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("linear_recent", { limit: 10 });
  });

  it("shows a project badge only when the ticket belongs to a project", async () => {
    mockLinear({ recent: [summary, recentSummary] });
    openWithKey();
    render(<LinearIssueDialog />);

    await screen.findByText("Fix login timeout");
    expect(screen.getByText("Q3 Auth")).toBeInTheDocument();
    // `recentSummary.project` is null: the row renders without a chip, and no
    // empty element is left behind to shift the row.
    expect(screen.getAllByText(/Q3 Auth/)).toHaveLength(1);
  });

  it("fills the project and status badges with their own colors", async () => {
    mockLinear({ recent: [summary] });
    openWithKey();
    render(<LinearIssueDialog />);

    const project = (await screen.findByText("Q3 Auth")).closest(
      "[title]",
    ) as HTMLElement;
    const status = screen.getByText("In Progress");
    // Both are solid fills in their own Linear color — the project's indigo
    // and the workflow state's amber, not one shared accent.
    expect(project.getAttribute("style")).toMatch(/5e6ad2|94, 106, 210/i);
    expect(status.getAttribute("style")).toMatch(/f2c94c|242, 201, 76/i);
    // The old dot-before-the-label is gone: the fill carries the color now,
    // and nothing nested inside the badges repeats it.
    expect(project.querySelector("[style*='background-color']")).toBeNull();
    expect(status.querySelector("[style*='background-color']")).toBeNull();
  });

  it("picks ink that stays readable on light and dark badge colors", async () => {
    mockLinear({
      recent: [
        {
          ...summary,
          project: "Amber",
          project_color: "#f2c94c",
          state: "Done",
          state_color: "#5e6ad2",
        },
      ],
    });
    openWithKey();
    render(<LinearIssueDialog />);

    const amber = (await screen.findByText("Amber")).closest(
      "[title]",
    ) as HTMLElement;
    const indigo = screen.getByText("Done");
    // Same helper, opposite outcomes: dark ink on the light amber fill, light
    // ink on the dark indigo one.
    expect(amber.getAttribute("style")).toMatch(/121212|18, 18, 18/i);
    expect(indigo.getAttribute("style")).toMatch(/ffffff|255, 255, 255/i);
  });

  it("falls back to the neutral badge when a color isn't hex", async () => {
    mockLinear({
      recent: [
        {
          ...summary,
          state_color: "rgb(1, 2, 3)",
          project_color: "hotpink",
        },
      ],
    });
    openWithKey();
    render(<LinearIssueDialog />);

    const chip = (await screen.findByText("Q3 Auth")).closest(
      "[title]",
    ) as HTMLElement;
    // No inline colors at all: the neutral border/muted-text pair applies.
    expect(chip.getAttribute("style")).toBeNull();
    expect(screen.getByText("In Progress").getAttribute("style")).toBeNull();
  });

  it("replaces the recent list with search results once you type", async () => {
    mockLinear({ recent: [recentSummary], search: [summary] });
    openWithKey();
    render(<LinearIssueDialog />);
    await screen.findByText("Recently touched ticket");

    fireEvent.change(screen.getByPlaceholderText(/Issue ID \(ENG-123\)/), {
      target: { value: "login" },
    });

    expect(await screen.findByText("Fix login timeout")).toBeInTheDocument();
    expect(
      screen.queryByText("Recently touched ticket"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Recent tickets")).not.toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("linear_search", { query: "login" });
  });

  it("attaches the chosen ticket and closes", async () => {
    mockLinear({ search: [summary] });
    const insertToActive = vi.fn(() => true);
    useStore.setState({ sessionMgr: { insertToActive } as never });
    openWithKey();
    render(<LinearIssueDialog />);

    fireEvent.change(screen.getByPlaceholderText(/Issue ID \(ENG-123\)/), {
      target: { value: "login" },
    });
    fireEvent.click(await screen.findByText("Fix login timeout"));

    await vi.waitFor(() =>
      expect(insertToActive).toHaveBeenCalledWith(
        expect.stringContaining("/tmp/ticket.txt"),
      ),
    );
    expect(invoke).toHaveBeenCalledWith("linear_issue", { id: "uuid-1" });
    expect(invoke).toHaveBeenCalledWith("save_text_attachment", {
      text: expect.stringContaining("## Description"),
    });
    expect(useStore.getState().linearDialogOpen).toBe(false);
  });

  it("selects a result with the keyboard", async () => {
    mockLinear({ search: [summary] });
    const insertToActive = vi.fn(() => true);
    useStore.setState({ sessionMgr: { insertToActive } as never });
    openWithKey();
    render(<LinearIssueDialog />);

    const input = screen.getByPlaceholderText(/Issue ID \(ENG-123\)/);
    fireEvent.change(input, { target: { value: "ENG-123" } });
    await screen.findByText("Fix login timeout");
    fireEvent.keyDown(input, { key: "Enter" });

    await vi.waitFor(() => expect(insertToActive).toHaveBeenCalled());
  });

  it("dismisses when the backdrop is clicked, but not when the dialog content is", async () => {
    mockLinear({ recent: [recentSummary] });
    openWithKey();
    render(<LinearIssueDialog />);
    const dialog = screen.getByRole("dialog");
    await screen.findByText("Recently touched ticket");

    fireEvent.mouseDown(screen.getByPlaceholderText(/Issue ID \(ENG-123\)/));
    expect(useStore.getState().linearDialogOpen).toBe(true);

    fireEvent.mouseDown(dialog);
    expect(useStore.getState().linearDialogOpen).toBe(false);
  });
});

describe("LinearIssueDialog key lifecycle", () => {
  it("does not carry a stale query into a later open", async () => {
    mockLinear({ recent: [recentSummary], search: [summary] });
    openWithKey();
    const { unmount } = render(<LinearIssueDialog />);
    await screen.findByText("Recently touched ticket");

    fireEvent.change(screen.getByPlaceholderText(/Issue ID \(ENG-123\)/), {
      target: { value: "login" },
    });
    await screen.findByText("Fix login timeout");

    // Escape/backdrop close, then reopen: the field must start empty.
    fireEvent.mouseDown(screen.getByRole("dialog"));
    await vi.waitFor(() =>
      expect(useStore.getState().linearDialogOpen).toBe(false),
    );
    act(() => useStore.setState({ linearDialogOpen: true }));

    const input = screen.getByPlaceholderText(/Issue ID \(ENG-123\)/);
    expect(input).toHaveValue("");
    unmount();
  });

  it("reports a ticket-fetch failure as a toast when dismissed mid-request", async () => {
    vi.mocked(invoke).mockImplementation(((cmd: string) => {
      if (cmd === "linear_search") return Promise.resolve([summary]);
      if (cmd === "linear_issue")
        return Promise.reject("Linear returned HTTP 500.");
      return Promise.resolve(undefined);
    }) as typeof invoke);
    openWithKey();
    render(<LinearIssueDialog />);

    fireEvent.change(screen.getByPlaceholderText(/Issue ID \(ENG-123\)/), {
      target: { value: "login" },
    });
    fireEvent.click(await screen.findByText("Fix login timeout"));
    fireEvent.mouseDown(screen.getByRole("dialog"));

    await vi.waitFor(() =>
      expect(
        useStore.getState().toasts.some((t) => t.message.includes("HTTP 500")),
      ).toBe(true),
    );
  });
});

describe("LinearIssueDialog after adding a key", () => {
  it("shows the recent list, not the old query, when reopened after Settings", async () => {
    mockLinear({ recent: [recentSummary], search: [summary] });
    // Open with no key, go to Settings, set a key, then reopen.
    act(() => useStore.setState({ linearDialogOpen: true }));
    render(<LinearIssueDialog />);
    expect(screen.getByText("Linear API key needed")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Open Settings"));

    act(() =>
      useStore.setState((s) => ({
        linearDialogOpen: false,
        settings: { ...s.settings, linear_api_key_set: true },
      })),
    );
    act(() => useStore.setState({ linearDialogOpen: true }));

    expect(
      await screen.findByText("Recently touched ticket"),
    ).toBeInTheDocument();
    expect(screen.getByText("Recent tickets")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("linear_search", expect.anything());
  });
});
