import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../session";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { loadAll } from "./data";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const wt = (project: string, feature: string): Worktree => ({
  project,
  feature,
  slug: feature,
  branch: `feat/${feature}`,
  path: `/path/${project}/${feature}`,
  status: null,
  has_resumable_session: false,
  note: "",
});

const session = (id: string): SessionInfo => ({
  id,
  project: "corral",
  feature: id.split("/")[1],
  running: true,
  exit_code: null,
  killed: false,
  waiting: true,
  last_summary: null,
});

beforeEach(() => {
  useStore.setState(useStore.getInitialState());
  vi.mocked(invoke).mockReset();
});

describe("loadAll", () => {
  it("populates worktrees and sessions, keeping the current selection if still valid", async () => {
    useStore.setState({ selectedId: "corral/hulling" });
    vi.mocked(invoke)
      .mockResolvedValueOnce([
        wt("corral", "hulling"),
        wt("corral", "churning"),
      ])
      .mockResolvedValueOnce([session("corral/hulling")])
      .mockResolvedValueOnce(["corral"])
      .mockResolvedValueOnce({ base: null, files: [] });
    await loadAll();
    expect(useStore.getState().worktrees).toHaveLength(2);
    expect(useStore.getState().sessions.get("corral/hulling")?.running).toBe(
      true,
    );
    expect(useStore.getState().selectedId).toBe("corral/hulling");
  });

  it("selects the first worktree when the previous selection is gone", async () => {
    useStore.setState({ selectedId: "corral/gone" });
    vi.mocked(invoke)
      .mockResolvedValueOnce([wt("corral", "hulling")])
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce(["corral"])
      .mockResolvedValueOnce({ base: null, files: [] });
    await loadAll();
    expect(useStore.getState().selectedId).toBe("corral/hulling");
  });

  it("clears selection when there are no worktrees", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);
    await loadAll();
    expect(useStore.getState().selectedId).toBeNull();
  });

  it("records a load error", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("backend down"));
    await loadAll();
    expect(useStore.getState().loadError).toBe(
      "Couldn't round up your worktrees: Error: backend down",
    );
  });

  it("fetches and caches a project icon per distinct project", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "list_worktrees") {
        return [
          wt("corral", "hulling"),
          wt("corral", "churning"),
          wt("typescript-sdk", "fix-1864"),
        ];
      }
      if (cmd === "list_sessions") return [];
      if (cmd === "list_projects") return ["corral", "typescript-sdk"];
      if (cmd === "branch_changes") return { base: null, files: [] };
      if (cmd === "watch_worktree") return undefined;
      if (cmd === "project_icon") {
        return (args as { project: string }).project === "corral"
          ? "data:image/png;base64,AA=="
          : null;
      }
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    await loadAll();

    // Icon fetching is fire-and-forget, so it may still be in flight when
    // `loadAll()` resolves — poll until it settles.
    await vi.waitFor(() => {
      expect(useStore.getState().projectIcons.get("corral")).toBe(
        "data:image/png;base64,AA==",
      );
    });
    await vi.waitFor(() => {
      expect(useStore.getState().projectIcons.get("typescript-sdk")).toBeNull();
    });
    // One project_icon call per distinct project, not per worktree.
    await vi.waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "project_icon"),
      ).toHaveLength(2);
    });
  });

  it("fetches icons for projects with no worktrees", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "list_worktrees") {
        return [wt("corral", "hulling")];
      }
      if (cmd === "list_sessions") return [];
      // "orchard" is a known project but has no active worktree yet.
      if (cmd === "list_projects") return ["corral", "orchard"];
      if (cmd === "branch_changes") return { base: null, files: [] };
      if (cmd === "watch_worktree") return undefined;
      if (cmd === "project_icon") {
        return (args as { project: string }).project === "orchard"
          ? "data:image/png;base64,orchard=="
          : null;
      }
      throw new Error(`unexpected invoke: ${cmd}`);
    });

    await loadAll();

    await vi.waitFor(() => {
      expect(useStore.getState().projectIcons.get("orchard")).toBe(
        "data:image/png;base64,orchard==",
      );
    });
    // Both projects got a lookup, even though only "corral" has a worktree.
    await vi.waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "project_icon"),
      ).toHaveLength(2);
    });
  });
});
