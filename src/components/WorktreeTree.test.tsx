import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { WorktreeTree } from "./WorktreeTree";

const wt = (feature: string): Worktree => ({
  project: "corral",
  feature,
  slug: feature,
  branch: `feat/${feature}`,
  path: `/path/${feature}`,
  status: null,
  has_resumable_session: false,
  note: "",
});

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  useStore.setState({ projects: ["corral", "other"] });
});

describe("WorktreeTree", () => {
  it("marks each project's add button with a stable data attribute", () => {
    render(<WorktreeTree />);
    expect(screen.getByTitle("New feature in corral")).toHaveAttribute(
      "data-project-add",
      "corral",
    );
    expect(screen.getByTitle("New feature in other")).toHaveAttribute(
      "data-project-add",
      "other",
    );
  });

  it("flashes the targeted project's add button and clears the nudge", async () => {
    render(<WorktreeTree />);
    useStore.setState({ worktreeNudgeTarget: "corral" });
    await waitFor(() =>
      expect(screen.getByTitle("New feature in corral")).toHaveClass(
        "project-add-nudge",
      ),
    );
    await waitFor(
      () => expect(useStore.getState().worktreeNudgeTarget).toBeNull(),
      { timeout: 2000 },
    );
  });

  it("does not flash an untargeted project's add button", () => {
    render(<WorktreeTree />);
    useStore.setState({ worktreeNudgeTarget: "corral" });
    expect(screen.getByTitle("New feature in other")).not.toHaveClass(
      "project-add-nudge",
    );
  });

  it("reveals each row's Ctrl+N number while Ctrl is held", () => {
    vi.useFakeTimers();
    try {
      useStore.setState({
        worktrees: [wt("hulling"), wt("churning")],
      });
      render(<WorktreeTree />);
      expect(screen.queryByTitle("Ctrl+1")).not.toBeInTheDocument();

      act(() => {
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "Control" }));
        vi.advanceTimersByTime(2000);
      });
      expect(screen.getByTitle("Ctrl+1")).toHaveTextContent("1");
      expect(screen.getByTitle("Ctrl+2")).toHaveTextContent("2");

      act(() => {
        window.dispatchEvent(new KeyboardEvent("keyup", { key: "Control" }));
      });
      expect(screen.queryByTitle("Ctrl+1")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});
