import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { LinearIssue } from "../types";
import {
  attachIssueToAgent,
  formatIssueAttachment,
  issueAttachmentPrompt,
} from "./linear";
import { invoke } from "./tauri";
import * as toasts from "./toasts";

vi.mock("./tauri", () => ({ invoke: vi.fn() }));
vi.mock("./toasts", () => ({ showError: vi.fn(), showSuccess: vi.fn() }));

const issue: LinearIssue = {
  id: "uuid-1",
  identifier: "ENG-123",
  title: "Fix login timeout",
  description: "Sessions expire too early.\n\nRepro steps…",
  url: "https://linear.app/acme/issue/ENG-123",
  state: "In Progress",
  team: "Engineering",
  project: "Q3 Auth",
  state_color: "#e2e2e2",
  project_color: "#5e6ad2",
  assignee: "Ada",
  priority: "High",
  labels: ["bug", "auth"],
};

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
  vi.mocked(toasts.showError).mockReset();
  vi.mocked(toasts.showSuccess).mockReset();
});

describe("formatIssueAttachment", () => {
  it("renders the ticket header, metadata, and body as markdown", () => {
    const text = formatIssueAttachment(issue);
    expect(text).toContain("# ENG-123: Fix login timeout");
    expect(text).toContain("- **URL:** https://linear.app/acme/issue/ENG-123");
    expect(text).toContain("- **State:** In Progress");
    expect(text).toContain("- **Project:** Q3 Auth");
    expect(text).toContain("- **Labels:** bug, auth");
    expect(text).toContain("## Description");
    expect(text).toContain("Sessions expire too early.");
  });

  it("falls back to a placeholder body and omits missing metadata", () => {
    const text = formatIssueAttachment({
      ...issue,
      description: null,
      state: null,
      team: null,
      project: null,
      state_color: null,
      project_color: null,
      assignee: null,
      priority: null,
      labels: [],
    });
    expect(text).toContain("(no description)");
    expect(text).not.toContain("**State:**");
    expect(text).not.toContain("**Project:**");
    expect(text).not.toContain("**Assignee:**");
  });
});

describe("issueAttachmentPrompt", () => {
  it("references the attachment path and leaves room to type an instruction", () => {
    const prompt = issueAttachmentPrompt(issue, "/tmp/corral-attach-1.txt");
    expect(prompt).toContain('Linear ticket ENG-123: "Fix login timeout"');
    expect(prompt).toContain("/tmp/corral-attach-1.txt");
    expect(prompt.endsWith(" ")).toBe(true);
  });
});

describe("attachIssueToAgent", () => {
  it("saves the body to a temp file and inserts a reference without submitting", async () => {
    vi.mocked(invoke).mockResolvedValue("/tmp/corral-attach-1.txt");
    const insertToActive = vi.fn(() => true);
    useStore.setState({ sessionMgr: { insertToActive } as never });

    expect(await attachIssueToAgent(issue)).toBe(true);

    expect(invoke).toHaveBeenCalledWith("save_text_attachment", {
      text: formatIssueAttachment(issue),
    });
    expect(insertToActive).toHaveBeenCalledWith(
      issueAttachmentPrompt(issue, "/tmp/corral-attach-1.txt"),
    );
    expect(useStore.getState().activeTab).toBe("agent");
    expect(toasts.showSuccess).toHaveBeenCalled();
  });

  it("explains that a running session is required", async () => {
    vi.mocked(invoke).mockResolvedValue("/tmp/corral-attach-1.txt");
    useStore.setState({ sessionMgr: { insertToActive: () => false } as never });

    expect(await attachIssueToAgent(issue)).toBe(false);
    expect(toasts.showSuccess).not.toHaveBeenCalled();
    expect(toasts.showError).toHaveBeenCalledWith(
      "Start the agent first, then attach a ticket.",
    );
  });

  it("reports a failed temp-file write instead of inserting a broken path", async () => {
    vi.mocked(invoke).mockRejectedValue("disk full");
    const insertToActive = vi.fn(() => true);
    useStore.setState({ sessionMgr: { insertToActive } as never });

    expect(await attachIssueToAgent(issue)).toBe(false);
    expect(insertToActive).not.toHaveBeenCalled();
    expect(toasts.showError).toHaveBeenCalled();
  });
});
