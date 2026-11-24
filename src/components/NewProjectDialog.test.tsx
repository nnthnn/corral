import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as data from "../lib/data";
import * as worktrees from "../lib/worktrees";
import type { SessionManager } from "../session";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { NewProjectDialog } from "./NewProjectDialog";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/worktrees", () => ({ createWorktree: vi.fn() }));
vi.mock("../lib/data", () => ({ loadAll: vi.fn() }));

const wt: Worktree = {
  project: "my-app",
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

describe("NewProjectDialog", () => {
  it("creates the project, builds its first pen, and closes", async () => {
    useStore.setState({ newProjectOpen: true });
    vi.mocked(invoke).mockResolvedValueOnce("my-app");
    vi.mocked(worktrees.createWorktree).mockResolvedValueOnce(wt);
    render(<NewProjectDialog />);

    fireEvent.change(screen.getByPlaceholderText("my-app"), {
      target: { value: "  my-app  " },
    });
    fireEvent.change(screen.getByPlaceholderText("~/source (default)"), {
      target: { value: "/home/user/source" },
    });
    fireEvent.click(screen.getByText("Create"));

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_project", {
        parent: "/home/user/source",
        name: "my-app",
      }),
    );
    expect(worktrees.createWorktree).toHaveBeenCalledWith("my-app");
    expect(useStore.getState().newProjectOpen).toBe(false);
  });

  it("queues the brief on the first session without submitting it", async () => {
    const prefillWhenReady = vi.fn();
    useStore.setState({
      newProjectOpen: true,
      sessionMgr: { prefillWhenReady } as unknown as SessionManager,
    });
    vi.mocked(invoke).mockResolvedValueOnce("my-app");
    vi.mocked(worktrees.createWorktree).mockResolvedValueOnce(wt);
    render(<NewProjectDialog />);

    fireEvent.change(screen.getByPlaceholderText("my-app"), {
      target: { value: "my-app" },
    });
    fireEvent.change(screen.getByPlaceholderText(/A local-first notes app/), {
      target: { value: "  a Rust CLI that syncs notes  " },
    });
    fireEvent.click(screen.getByText("Create"));

    await vi.waitFor(() =>
      expect(prefillWhenReady).toHaveBeenCalledWith(
        "my-app/hulling",
        "a Rust CLI that syncs notes",
      ),
    );
  });

  it("picks the parent folder from the native picker", async () => {
    useStore.setState({ newProjectOpen: true });
    vi.mocked(open).mockResolvedValueOnce("/home/user/code");
    render(<NewProjectDialog />);

    fireEvent.click(screen.getByText("Choose…"));

    await vi.waitFor(() =>
      expect(screen.getByPlaceholderText("~/source (default)")).toHaveValue(
        "/home/user/code",
      ),
    );
  });

  it("shows an error and stays open when creation fails", async () => {
    useStore.setState({ newProjectOpen: true });
    vi.mocked(invoke).mockRejectedValueOnce(
      new Error("project directory is not empty"),
    );
    render(<NewProjectDialog />);

    fireEvent.change(screen.getByPlaceholderText("my-app"), {
      target: { value: "my-app" },
    });
    fireEvent.click(screen.getByText("Create"));

    await screen.findByText(/Error: project directory is not empty/);
    expect(useStore.getState().newProjectOpen).toBe(true);
    expect(worktrees.createWorktree).not.toHaveBeenCalled();
  });
});
