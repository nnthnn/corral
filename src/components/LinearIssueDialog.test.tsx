import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
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
    settings: { ...s.settings, linear_api_key: "lin_api_test" },
  }));
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

  it("searches as you type and attaches the chosen ticket", async () => {
    vi.mocked(invoke).mockImplementation(((cmd: string) => {
      if (cmd === "linear_search") return Promise.resolve([summary]);
      if (cmd === "linear_issue") return Promise.resolve(issue);
      if (cmd === "save_text_attachment")
        return Promise.resolve("/tmp/ticket.txt");
      return Promise.resolve(undefined);
    }) as typeof invoke);
    const insertToActive = vi.fn(() => true);
    useStore.setState({ sessionMgr: { insertToActive } as never });
    openWithKey();
    render(<LinearIssueDialog />);

    fireEvent.change(screen.getByPlaceholderText(/Issue ID \(ENG-123\)/), {
      target: { value: "login" },
    });

    const row = await screen.findByText("Fix login timeout");
    expect(invoke).toHaveBeenCalledWith("linear_search", {
      apiKey: "lin_api_test",
      query: "login",
    });

    fireEvent.click(row);

    await vi.waitFor(() =>
      expect(insertToActive).toHaveBeenCalledWith(
        expect.stringContaining("/tmp/ticket.txt"),
      ),
    );
    expect(invoke).toHaveBeenCalledWith("linear_issue", {
      apiKey: "lin_api_test",
      id: "uuid-1",
    });
    expect(invoke).toHaveBeenCalledWith("save_text_attachment", {
      text: expect.stringContaining("## Description"),
    });
    expect(useStore.getState().linearDialogOpen).toBe(false);
  });

  it("selects a result with the keyboard", async () => {
    vi.mocked(invoke).mockImplementation(((cmd: string) => {
      if (cmd === "linear_search") return Promise.resolve([summary]);
      if (cmd === "linear_issue") return Promise.resolve(issue);
      if (cmd === "save_text_attachment")
        return Promise.resolve("/tmp/ticket.txt");
      return Promise.resolve(undefined);
    }) as typeof invoke);
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
});
