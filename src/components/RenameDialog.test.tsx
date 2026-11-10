import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { RenameDialog } from "./RenameDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const wt: Worktree = {
  project: "corral",
  feature: "hulling",
  slug: "hulling",
  branch: "feat/hulling",
  path: "/path/hulling",
  status: null,
  has_resumable_session: false,
  note: "",
};

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
});

describe("RenameDialog", () => {
  it("prefills the display name and branch", () => {
    useStore.setState({ renameTarget: wt });
    render(<RenameDialog />);
    expect(screen.getByDisplayValue("hulling")).toBeInTheDocument();
    expect(screen.getByText("feat/hulling")).toBeInTheDocument();
  });

  it("renames and reloads on submit", async () => {
    useStore.setState({ renameTarget: wt });
    vi.mocked(invoke)
      .mockResolvedValueOnce(undefined) // rename_worktree
      .mockResolvedValueOnce([]) // list_worktrees (loadAll)
      .mockResolvedValueOnce([]); // list_sessions (loadAll)
    render(<RenameDialog />);
    fireEvent.change(screen.getByDisplayValue("hulling"), {
      target: { value: "hull-2" },
    });
    fireEvent.click(screen.getByText("Rename"));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("rename_worktree", {
        project: "corral",
        feature: "hulling",
        newName: "hull-2",
      }),
    );
    expect(useStore.getState().renameTarget).toBeNull();
  });

  it("prefills the title instead of the slug when a title is set", () => {
    useStore.setState({
      renameTarget: { ...wt, title: "Hulling Grain", slug: "hulling" },
    });
    render(<RenameDialog />);
    expect(screen.getByDisplayValue("Hulling Grain")).toBeInTheDocument();
  });

  it("shows an error and stays open on failure", async () => {
    useStore.setState({ renameTarget: wt });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("name taken"));
    render(<RenameDialog />);
    fireEvent.click(screen.getByText("Rename"));
    await screen.findByText("Error: name taken");
    expect(useStore.getState().renameTarget).toBe(wt);
  });
});
