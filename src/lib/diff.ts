import { currentWt, useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";
import { invoke } from "./tauri";

export async function openDiff(wt: Worktree, path: string): Promise<void> {
  const id = wtId(wt);
  useStore.setState({
    diffTab: { path },
    activeTab: "diff",
    diffContent: null,
    diffError: null,
  });
  // Compare the worktree too, not just the path: two worktrees can each have
  // a `src/main.rs`, and `selectWorktree` clearing `diffTab` isn't enough when
  // the same path is opened again in the new worktree before an in-flight
  // fetch for the old one lands.
  const isStillActive = () => {
    const s = useStore.getState();
    return s.selectedId === id && s.diffTab?.path === path;
  };
  try {
    const diff = await invoke("file_diff", {
      project: wt.project,
      feature: wt.feature,
      path,
    });
    if (isStillActive()) useStore.setState({ diffContent: diff });
  } catch (err) {
    if (isStillActive()) useStore.setState({ diffError: String(err) });
  }
}

/** Steps the open diff to the next (`1`) or previous (`-1`) changed file in
 * the Changes list — the `j`/`k` (or arrow-key) navigation for the diff
 * overlay. No-op at the ends (no wrap), matching GitHub's per-file nav.
 * Uses the currently-selected worktree's `changes.files` order, which is what
 * the Changes pane renders. */
export function stepDiff(delta: -1 | 1): void {
  const s = useStore.getState();
  if (s.activeTab !== "diff") return;
  const wt = currentWt(s);
  if (!wt || !s.diffTab) return;
  const files = s.changes?.files ?? [];
  if (files.length === 0) return;

  const idx = files.findIndex((f) => f.path === s.diffTab?.path);
  const nextIdx = idx === -1 ? 0 : idx + delta;
  const target = files[nextIdx];
  if (!target) return;
  openDiff(wt, target.path);
}
