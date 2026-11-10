import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { FileTree } from "./FileTree";

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
  useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
});

describe("FileTree", () => {
  it("renders nothing when no worktree is selected", () => {
    useStore.setState({ selectedId: null });
    const { container } = render(<FileTree />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a loading indicator until the root listing is cached", () => {
    render(<FileTree />);
    expect(screen.getByText("Loading…")).toBeInTheDocument();
  });

  it("renders cached root entries, dirs and files", () => {
    useStore.setState({
      fileTree: new Map([
        [
          "corral/hulling:",
          [
            { name: "src", path: "src", is_dir: true },
            { name: "README.md", path: "README.md", is_dir: false },
          ],
        ],
      ]),
    });
    render(<FileTree />);
    expect(screen.getByText("src")).toBeInTheDocument();
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("fetches a directory's listing on first expand", () => {
    useStore.setState({
      fileTree: new Map([
        ["corral/hulling:", [{ name: "src", path: "src", is_dir: true }]],
      ]),
    });
    vi.mocked(invoke).mockResolvedValueOnce([
      { name: "main.rs", path: "src/main.rs", is_dir: false },
    ]);
    render(<FileTree />);
    fireEvent.click(screen.getByText("src"));
    expect(invoke).toHaveBeenCalledWith("list_worktree_dir", {
      project: "corral",
      feature: "hulling",
      path: "src",
    });
  });

  it("does not refetch an already-cached directory on re-expand", () => {
    useStore.setState({
      fileTree: new Map([
        ["corral/hulling:", [{ name: "src", path: "src", is_dir: true }]],
        [
          "corral/hulling:src",
          [{ name: "main.rs", path: "src/main.rs", is_dir: false }],
        ],
      ]),
    });
    render(<FileTree />);
    fireEvent.click(screen.getByText("src"));
    expect(invoke).not.toHaveBeenCalled();
    expect(screen.getByText("main.rs")).toBeInTheDocument();
  });

  it("collapses an expanded directory on second click", () => {
    useStore.setState({
      fileTree: new Map([
        ["corral/hulling:", [{ name: "src", path: "src", is_dir: true }]],
        [
          "corral/hulling:src",
          [{ name: "main.rs", path: "src/main.rs", is_dir: false }],
        ],
      ]),
    });
    render(<FileTree />);
    fireEvent.click(screen.getByText("src"));
    expect(screen.getByText("main.rs")).toBeInTheDocument();
    fireEvent.click(screen.getByText("src"));
    expect(screen.queryByText("main.rs")).not.toBeInTheDocument();
  });

  it("opens a file preview tab on click", () => {
    useStore.setState({
      fileTree: new Map([
        [
          "corral/hulling:",
          [{ name: "README.md", path: "README.md", is_dir: false }],
        ],
      ]),
    });
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "text", text: "hi" });
    render(<FileTree />);
    fireEvent.click(screen.getByText("README.md"));
    expect(invoke).toHaveBeenCalledWith("read_worktree_file", {
      project: "corral",
      feature: "hulling",
      path: "README.md",
    });
    expect(useStore.getState().fileTab).toEqual({ path: "README.md" });
    expect(useStore.getState().activeTab).toBe("file");
  });
});
