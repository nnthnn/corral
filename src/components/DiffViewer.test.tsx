import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { ChangedFile, Worktree } from "../types";
import { DiffViewer } from "./DiffViewer";

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

describe("DiffViewer", () => {
  it("shows a loading indicator while diffContent is null", () => {
    useStore.setState({ diffTab: { path: "src/main.rs" } });
    render(<DiffViewer />);
    expect(screen.getByText("Loading…")).toBeInTheDocument();
  });

  it("renders diff lines with add/remove styling", () => {
    useStore.setState({
      diffTab: { path: "src/main.rs" },
      diffContent: "+added line\n-removed line\n context line",
    });
    const { container } = render(<DiffViewer />);
    const add = container.querySelector(".diff-line.add");
    const del = container.querySelector(".diff-line.del");
    expect(add).not.toBeNull();
    expect(add?.textContent).toContain("added line");
    expect(del).not.toBeNull();
    expect(del?.textContent).toContain("removed line");
  });

  it("shows the error line when diffError is set", () => {
    useStore.setState({
      diffTab: { path: "src/main.rs" },
      diffError: "not found",
    });
    render(<DiffViewer />);
    expect(screen.getByText("not found")).toHaveClass("err");
  });

  it("renders at the configured diff_font_size", () => {
    useStore.setState({
      diffTab: { path: "src/main.rs" },
      diffContent: " context line",
      settings: { ...useStore.getState().settings, diff_font_size: 22 },
    });
    const { container } = render(<DiffViewer />);
    expect(container.querySelector("pre.diff-body")).toHaveStyle({
      fontSize: "22px",
    });
  });

  it("syntax-highlights code, keeping the add/del line class", () => {
    useStore.setState({
      diffTab: { path: "src/main.rs" },
      diffContent: "+fn main() {}\n-context line",
    });
    const { container } = render(<DiffViewer />);
    const add = container.querySelector(".diff-line.add");
    // "fn" is a Rust keyword and should be wrapped in a hljs token span.
    expect(add?.querySelector(".hljs-keyword")?.textContent).toBe("fn");
  });

  it("hides unchanged lines when the toggle is on", () => {
    useStore.setState({
      diffTab: { path: "src/main.rs" },
      diffContent: "+added\n unchanged\n-removed",
    });
    const { container } = render(<DiffViewer />);
    expect(container.querySelector(".diff-line.ctx")).not.toBeNull();

    fireEvent.click(screen.getByTitle("Hide unchanged lines"));

    expect(container.querySelector(".diff-line.ctx")).toBeNull();
    expect(container.querySelector(".diff-line.add")).not.toBeNull();
    expect(container.querySelector(".diff-line.del")).not.toBeNull();
  });

  it("shows a note when hiding unchanged lines empties the diff", () => {
    useStore.setState({
      diffTab: { path: "src/main.rs" },
      diffContent: " only context",
    });
    render(<DiffViewer />);
    fireEvent.click(screen.getByTitle("Hide unchanged lines"));
    expect(screen.getByText("No changed lines to show.")).toBeInTheDocument();
  });

  describe("file navigation", () => {
    const files: ChangedFile[] = [
      {
        path: "a.rs",
        status: "modified",
        old_path: null,
        staged: false,
        sync_state: "uncommitted",
        dirty: true,
        added: 1,
        removed: 0,
      },
      {
        path: "b.rs",
        status: "modified",
        old_path: null,
        staged: false,
        sync_state: "uncommitted",
        dirty: true,
        added: 1,
        removed: 0,
      },
      {
        path: "c.rs",
        status: "modified",
        old_path: null,
        staged: false,
        sync_state: "uncommitted",
        dirty: true,
        added: 1,
        removed: 0,
      },
    ];

    function select(diffPath = "b.rs") {
      // Resolve `file_diff` so a successful nav updates diffContent to a
      // string (matching the real backend) rather than `undefined`.
      vi.mocked(invoke).mockResolvedValue("diff");
      useStore.setState({
        worktrees: [wt],
        selectedId: "corral/hulling",
        activeTab: "diff",
        diffTab: { path: diffPath },
        changes: { base: "main", files: [...files] },
      });
    }

    it("steps to the next file on j", () => {
      select();
      render(<DiffViewer />);
      fireEvent.keyDown(window, { key: "j" });
      expect(invoke).toHaveBeenCalledWith("file_diff", {
        project: "corral",
        feature: "hulling",
        path: "c.rs",
      });
    });

    it("steps to the previous file on k", () => {
      select();
      render(<DiffViewer />);
      fireEvent.keyDown(window, { key: "k" });
      expect(invoke).toHaveBeenCalledWith("file_diff", {
        project: "corral",
        feature: "hulling",
        path: "a.rs",
      });
    });

    it("does not wrap past the ends", () => {
      select("c.rs");
      render(<DiffViewer />);
      fireEvent.keyDown(window, { key: "j" });
      expect(invoke).not.toHaveBeenCalled();
    });

    it("ignores j/k typed into an input", () => {
      select();
      render(<DiffViewer />);
      const input = document.createElement("input");
      document.body.appendChild(input);
      fireEvent.keyDown(input, { key: "j" });
      expect(invoke).not.toHaveBeenCalled();
      input.remove();
    });
  });
});
