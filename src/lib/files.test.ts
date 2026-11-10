import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import {
  imageDataUrl,
  isSvgPath,
  loadDir,
  openFile,
  svgDataUrl,
} from "./files";

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
  // Files are only ever opened for the selected worktree, and the stale guard
  // keys on that selection.
  useStore.setState({ selectedId: "corral/hulling" });
  vi.mocked(invoke).mockReset();
});

describe("image helpers", () => {
  it("builds a base64 data URL from mime + data", () => {
    expect(imageDataUrl("image/png", "abc")).toBe("data:image/png;base64,abc");
  });

  it("builds a URL-encoded data URL for SVG source", () => {
    expect(svgDataUrl("<svg></svg>")).toBe(
      `data:image/svg+xml;charset=utf-8,${encodeURIComponent("<svg></svg>")}`,
    );
  });

  it("detects .svg case-insensitively and nothing else", () => {
    expect(isSvgPath("icons/Logo.SVG")).toBe(true);
    expect(isSvgPath("logo.png")).toBe(false);
    expect(isSvgPath("svg")).toBe(false);
  });
});

describe("loadDir", () => {
  it("fetches and caches a directory listing", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      { name: "main.rs", path: "src/main.rs", is_dir: false },
    ]);
    await loadDir(wt, "src");
    expect(invoke).toHaveBeenCalledWith("list_worktree_dir", {
      project: "corral",
      feature: "hulling",
      path: "src",
    });
    expect(useStore.getState().fileTree.get("corral/hulling:src")).toEqual([
      { name: "main.rs", path: "src/main.rs", is_dir: false },
    ]);
  });

  it("does not refetch a directory that's already cached", async () => {
    useStore.setState({
      fileTree: new Map([["corral/hulling:src", []]]),
    });
    await loadDir(wt, "src");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("shows an error toast on failure without caching anything", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("permission denied"));
    await loadDir(wt, "src");
    expect(useStore.getState().fileTree.has("corral/hulling:src")).toBe(false);
    expect(useStore.getState().toasts).toHaveLength(1);
  });
});

describe("openFile", () => {
  it("opens the file tab and loads content on success", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "text", text: "hi" });
    await openFile(wt, "README.md");
    expect(invoke).toHaveBeenCalledWith("read_worktree_file", {
      project: "corral",
      feature: "hulling",
      path: "README.md",
    });
    expect(useStore.getState().fileTab).toEqual({ path: "README.md" });
    expect(useStore.getState().activeTab).toBe("file");
    expect(useStore.getState().fileContent).toEqual({
      kind: "text",
      text: "hi",
    });
    expect(useStore.getState().fileError).toBeNull();
  });

  it("records an error string on failure", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("no such file"));
    await openFile(wt, "gone.txt");
    expect(useStore.getState().fileError).toBe("Error: no such file");
    expect(useStore.getState().fileContent).toBeNull();
  });

  it("ignores a stale response after a different file was opened", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({ kind: "text", text: "stale" })
      .mockResolvedValueOnce({ kind: "text", text: "fresh" });
    const first = openFile(wt, "first.txt");
    await openFile(wt, "second.txt");
    await first;
    expect(useStore.getState().fileTab).toEqual({ path: "second.txt" });
    expect(useStore.getState().fileContent).toEqual({
      kind: "text",
      text: "fresh",
    });
  });

  it("ignores a same-path response that belongs to a different worktree", async () => {
    const other: Worktree = { ...wt, feature: "churning" };
    vi.mocked(invoke)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) =>
            setTimeout(() => resolve({ kind: "text", text: "hulling" }), 0),
          ),
      )
      .mockResolvedValueOnce({ kind: "text", text: "churning" });

    const first = openFile(wt, "README.md");
    useStore.setState({ selectedId: "corral/churning" });
    await openFile(other, "README.md");
    await first;

    expect(useStore.getState().fileContent).toEqual({
      kind: "text",
      text: "churning",
    });
  });
});
