import { listen } from "@tauri-apps/api/event";
import { useStore } from "../store";
import { wtId } from "../utils";
import { loadChanges } from "./changes";
import { invoke } from "./tauri";

interface WorktreeChangedPayload {
  project: string;
  feature: string;
  /** True when the event was a git-metadata change (commit/checkout/stage)
   * — those move the merge/PR-button state, so they warrant a full
   * `git::status` refresh. Ordinary file edits only move the Changes pane,
   * so they skip that (expensive) refresh. */
  git_changed: boolean;
}

/** Backend emits this whenever the selected worktree's watched files change
 * (debounced ~200ms) — see `src-tauri/src/worktree_watch.rs`. Refresh the
 * Changes pane immediately rather than waiting on the PTY-output debounce,
 * which misses changes made outside the agent's own session. */
export async function initWorktreeWatch(): Promise<void> {
  await listen<WorktreeChangedPayload>("worktree-changed", (event) => {
    const { project, feature, git_changed } = event.payload;
    const s = useStore.getState();
    const id = `${project}/${feature}`;
    if (s.selectedId !== id) return;
    const wt = s.worktrees.find((w) => wtId(w) === id);
    if (wt) {
      loadChanges(wt);
      if (git_changed) refreshStatus(wt.project, wt.feature);
    }
  });
}

/** Commits (unlike ordinary file edits) also change a worktree's
 * `GitStatus` — `ahead`/`commits_ahead_of_base`/etc. — which drives the
 * header's merge/PR button. That status only otherwise refreshes via
 * `loadAll()` (app start, worktree create/rename/remove, or a manual
 * sidebar refresh), so without this a commit made inside an already-open
 * worktree could leave that button stale indefinitely. Cheap by design:
 * `refresh_worktree_status` re-fetches just this one worktree instead of
 * `loadAll`'s full re-scan of every worktree in every project. */
async function refreshStatus(project: string, feature: string): Promise<void> {
  const id = `${project}/${feature}`;
  const isStillSelected = () => useStore.getState().selectedId === id;
  try {
    const status = await invoke("refresh_worktree_status", {
      project,
      feature,
    });
    if (!isStillSelected()) return;
    const worktrees = useStore
      .getState()
      .worktrees.map((w) => (wtId(w) === id ? { ...w, status } : w));
    useStore.setState({ worktrees });
  } catch {
    // Best-effort, matches `loadChanges`' failure handling — a stale
    // status is preferable to surfacing an error for a background refresh.
  }
}
