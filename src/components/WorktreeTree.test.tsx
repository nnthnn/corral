import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useStore } from "../store";
import { WorktreeTree } from "./WorktreeTree";

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
});
