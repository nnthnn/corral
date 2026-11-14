import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { initBrowserPreview } from "./browser-events";

const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));

vi.mock("./actions", () => ({ selectWorktree: vi.fn() }));
vi.mock("./files", () => ({ openFile: vi.fn() }));

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

beforeEach(async () => {
  useStore.setState(useStore.getInitialState());
  listeners.clear();
  const { selectWorktree } = await import("./actions");
  vi.mocked(selectWorktree).mockReset();
  const { openFile } = await import("./files");
  vi.mocked(openFile).mockReset();
});

describe("initBrowserPreview", () => {
  it("selects the session's worktree and opens the previewed file", async () => {
    const { selectWorktree } = await import("./actions");
    const { openFile } = await import("./files");
    useStore.setState({ worktrees: [wt], selectedId: "corral/other" });
    await initBrowserPreview();

    listeners.get("browser-preview")?.({
      payload: { project: "corral", feature: "hulling", path: "docs/shot.png" },
    });

    expect(selectWorktree).toHaveBeenCalledWith("corral/hulling");
    expect(openFile).toHaveBeenCalledWith(wt, "docs/shot.png");
  });

  it("keeps the current selection when the worktree is already active", async () => {
    const { selectWorktree } = await import("./actions");
    const { openFile } = await import("./files");
    useStore.setState({ worktrees: [wt], selectedId: "corral/hulling" });
    await initBrowserPreview();

    listeners.get("browser-preview")?.({
      payload: { project: "corral", feature: "hulling", path: "README.md" },
    });

    expect(selectWorktree).not.toHaveBeenCalled();
    expect(openFile).toHaveBeenCalledWith(wt, "README.md");
  });

  it("ignores a preview for a worktree that is not loaded", async () => {
    const { selectWorktree } = await import("./actions");
    const { openFile } = await import("./files");
    useStore.setState({ worktrees: [], selectedId: null });
    await initBrowserPreview();

    listeners.get("browser-preview")?.({
      payload: { project: "corral", feature: "missing", path: "README.md" },
    });

    expect(selectWorktree).not.toHaveBeenCalled();
    expect(openFile).not.toHaveBeenCalled();
  });
});
