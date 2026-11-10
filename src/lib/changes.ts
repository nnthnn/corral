import { currentWt, useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";
import { invoke } from "./tauri";

export async function loadChanges(wt: Worktree): Promise<void> {
  const id = wtId(wt);
  const isStillSelected = () => useStore.getState().selectedId === id;
  try {
    const changes = await invoke("branch_changes", {
      project: wt.project,
      feature: wt.feature,
    });
    if (isStillSelected()) useStore.setState({ changes });
  } catch {
    if (isStillSelected())
      useStore.setState({ changes: { base: null, files: [] } });
  }
}

/** Discard uncommitted changes to one file (restore to `HEAD`, or delete if
 * untracked/new), then refresh the Changes pane. Destructive — callers must
 * confirm with the user first. */
export async function revertFile(
  wt: Worktree,
  path: string,
  oldPath: string | null,
): Promise<void> {
  await invoke("revert_file", {
    project: wt.project,
    feature: wt.feature,
    path,
    oldPath,
  });
  await loadChanges(wt);
}

let changesTimer: number | null = null;

export function scheduleChangesRefresh(): void {
  if (changesTimer) clearTimeout(changesTimer);
  changesTimer = window.setTimeout(() => {
    const wt = currentWt(useStore.getState());
    if (wt) loadChanges(wt);
  }, 1500);
}
