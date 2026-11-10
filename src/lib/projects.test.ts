import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import * as data from "./data";
import { addProject } from "./projects";
import * as worktrees from "./worktrees";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./worktrees", () => ({ createWorktree: vi.fn() }));
vi.mock("./data", () => ({ loadAll: vi.fn() }));

const wt: Worktree = {
  project: "my-repo",
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
  vi.mocked(open).mockReset();
  vi.mocked(invoke).mockReset();
  vi.mocked(worktrees.createWorktree).mockReset();
  vi.mocked(data.loadAll).mockReset();
});

describe("addProject", () => {
  it("does nothing if the user cancels the folder picker", async () => {
    vi.mocked(open).mockResolvedValueOnce(null);
    await addProject();
    expect(invoke).not.toHaveBeenCalled();
    expect(worktrees.createWorktree).not.toHaveBeenCalled();
  });

  it("registers the project then auto-creates its first worktree", async () => {
    vi.mocked(open).mockResolvedValueOnce("/home/user/source/my-repo");
    vi.mocked(invoke).mockResolvedValueOnce("my-repo");
    vi.mocked(worktrees.createWorktree).mockResolvedValueOnce(wt);
    await addProject();
    expect(invoke).toHaveBeenCalledWith("add_project", {
      path: "/home/user/source/my-repo",
    });
    expect(worktrees.createWorktree).toHaveBeenCalledWith("my-repo");
  });

  it("shows an error toast and does not create a worktree if add_project fails", async () => {
    vi.mocked(open).mockResolvedValueOnce("/home/user/source/my-repo");
    vi.mocked(invoke).mockRejectedValueOnce(new Error("not a git repository"));
    await addProject();
    const messages = useStore.getState().toasts.map((t) => t.message);
    expect(messages[0]).toContain("Couldn't add project");
    expect(messages[0]).toContain("not a git repository");
    expect(worktrees.createWorktree).not.toHaveBeenCalled();
  });

  it("shows a distinct error toast if add_project succeeds but createWorktree fails", async () => {
    vi.mocked(open).mockResolvedValueOnce("/home/user/source/my-repo");
    vi.mocked(invoke).mockResolvedValueOnce("my-repo");
    vi.mocked(worktrees.createWorktree).mockRejectedValueOnce(
      new Error("disk full"),
    );
    await addProject();
    const messages = useStore.getState().toasts.map((t) => t.message);
    expect(messages[0]).toContain("Project added");
    expect(messages[0]).toContain("couldn't build the first pen");
    expect(messages[0]).toContain("disk full");
    expect(data.loadAll).toHaveBeenCalled();
  });
});
