import { sessionStatus, type TabId, useStore } from "../store";
import type { Worktree } from "../types";
import { adjacentWorktreeId, wtId } from "../utils";
import { loadChanges } from "./changes";
import { loadAll } from "./data";
import { invoke } from "./tauri";
import { showError, showSuccess } from "./toasts";
import { fetchPrStatus } from "./worktrees";

/** Refreshes the derived state tied to `id` — changes, the worktree
 * watcher, PR status — without touching `activeTab`/`diffTab`. Split out
 * of `selectWorktree` so `loadAll` can re-affirm an already-selected
 * worktree's data (e.g. after a background refresh) without yanking the
 * user back to the Agent tab or closing their open Diff tab. */
export function syncSelectedWorktree(id: string): void {
  const target = useStore
    .getState()
    .worktrees.find((w) => `${w.project}/${w.feature}` === id);
  if (target) {
    loadChanges(target);
    invoke("watch_worktree", {
      project: target.project,
      feature: target.feature,
    }).catch((err) => showError(`Lost track of that worktree: ${err}`));
    if (target.status?.has_remote) fetchPrStatus(target);
  }
}

export function selectWorktree(id: string): void {
  const s = useStore.getState();
  const unreadOutput = new Set(s.unreadOutput);
  unreadOutput.delete(id);
  const acknowledgedWaiting = new Set(s.acknowledgedWaiting);
  if (sessionStatus(s, id)?.cls === "waiting") acknowledgedWaiting.add(id);

  // Re-selecting the already-selected worktree (e.g. clicking its own sidebar
  // row) should only clear its attention cues — not discard the user's open
  // Diff/File tab or their place in the right pane.
  if (s.selectedId === id) {
    useStore.setState({ unreadOutput, acknowledgedWaiting });
    return;
  }

  useStore.setState({
    selectedId: id,
    unreadOutput,
    acknowledgedWaiting,
    diffTab: null,
    fileTab: null,
    activeTab: "agent",
    changesPanelTab: "changes",
    // Drop the previous worktree's file list immediately. `loadChanges`
    // replaces it only once `branch_changes` resolves (and its stale-response
    // guard keys on `selectedId`), so without this the pane renders the old
    // worktree's files under the new worktree's `vs <base>` label for a beat.
    changes: null,
  });

  syncSelectedWorktree(id);
}

export function clearSelection(): void {
  useStore.setState({
    selectedId: null,
    diffTab: null,
    fileTab: null,
    activeTab: "agent",
    changesPanelTab: "changes",
  });
}

export function closeTab(id: TabId): void {
  if (id === "diff") useStore.setState({ diffTab: null });
  if (id === "file") useStore.setState({ fileTab: null });
  if (useStore.getState().activeTab === id)
    useStore.setState({ activeTab: "agent" });
}

const ROW_EXIT_MS = 200;

/** Removes a worktree: calls the backend, then closes the confirm dialog
 * and marks the sidebar row as exiting (`removingIds`) so it can play its
 * collapse animation. The row stays marked "exiting" (and mounted) until
 * `loadAll()` has actually dropped it from `worktrees` — clearing
 * `removingIds` any earlier would let the row's collapse transition run in
 * reverse for the gap between the animation finishing and `loadAll()`
 * resolving. Rejects (without touching any store state) if the backend call
 * itself fails. */
export async function removeWorktree(
  wt: Worktree,
  deleteBranch: boolean,
): Promise<void> {
  const id = wtId(wt);
  await invoke("remove_worktree", {
    project: wt.project,
    feature: wt.feature,
    deleteBranch,
  });
  useStore.setState({ deleteTarget: null });
  const removingIds = new Set(useStore.getState().removingIds);
  removingIds.add(id);
  useStore.setState({ removingIds });
  await new Promise<void>((resolve) => setTimeout(resolve, ROW_EXIT_MS));
  finishRowExit(id);
  await loadAll();
  const finalRemovingIds = new Set(useStore.getState().removingIds);
  finalRemovingIds.delete(id);
  useStore.setState({ removingIds: finalRemovingIds });
  showSuccess("Cleared out.");
}

/** Drops `key` from `map`, returning the *same* map when it's already absent
 * so a removal doesn't churn every subscriber for an unchanged map. */
function withoutKey<K, V>(map: Map<K, V>, key: K): Map<K, V> {
  if (!map.has(key)) return map;
  const next = new Map(map);
  next.delete(key);
  return next;
}

function withoutMember<T>(set: Set<T>, value: T): Set<T> {
  if (!set.has(value)) return set;
  const next = new Set(set);
  next.delete(value);
  return next;
}

/** Same as [`withoutKey`] for the maps/sets keyed `<wtId>:<relativePath>`
 * (the Files tree's directory cache and expansion set). */
function withoutKeyPrefix<V>(
  map: Map<string, V>,
  prefix: string,
): Map<string, V> {
  let next: Map<string, V> | null = null;
  for (const key of map.keys()) {
    if (!key.startsWith(prefix)) continue;
    next ??= new Map(map);
    next.delete(key);
  }
  return next ?? map;
}

function withoutMemberPrefix(set: Set<string>, prefix: string): Set<string> {
  let next: Set<string> | null = null;
  for (const value of set) {
    if (!value.startsWith(prefix)) continue;
    next ??= new Set(set);
    next.delete(value);
  }
  return next ?? set;
}

function finishRowExit(id: string): void {
  const s = useStore.getState();
  s.sessionMgr?.closeTerminal(id);
  // Every map/set below is keyed by `<project>/<feature>`, an id a future
  // worktree can legitimately reuse. Anything left behind then belongs to the
  // *old* worktree — most visibly a PR tab (tab membership is derived from
  // `prInfoByWtId`) plus dead checks and Run/Setup output — so sweep the whole
  // set rather than the handful this used to delete. `removingIds` is
  // deliberately kept: the sidebar row's exit animation still depends on it.
  useStore.setState({
    sessions: withoutKey(s.sessions, id),
    unreadOutput: withoutMember(s.unreadOutput, id),
    acknowledgedWaiting: withoutMember(s.acknowledgedWaiting, id),
    notifiedWaiting: withoutMember(s.notifiedWaiting, id),
    justMergedIds: withoutMember(s.justMergedIds, id),
    mergingIds: withoutMember(s.mergingIds, id),
    startingIds: withoutMember(s.startingIds, id),
    rebasingIds: withoutMember(s.rebasingIds, id),
    mergingPrIds: withoutMember(s.mergingPrIds, id),
    markingPrReadyIds: withoutMember(s.markingPrReadyIds, id),
    preparingPr: withoutKey(s.preparingPr, id),
    prInfoByWtId: withoutKey(s.prInfoByWtId, id),
    checksByWtId: withoutKey(s.checksByWtId, id),
    prDetailsByWtId: withoutKey(s.prDetailsByWtId, id),
    provisioning: withoutKey(s.provisioning, id),
    run: withoutKey(s.run, id),
    fileTree: withoutKeyPrefix(s.fileTree, `${id}:`),
    expandedDirs: withoutMemberPrefix(s.expandedDirs, `${id}:`),
  });

  if (s.selectedId === id) {
    const next = adjacentWorktreeId(
      s.worktrees,
      s.projects,
      s.settings.project_order,
      id,
    );
    if (next) selectWorktree(next);
    else clearSelection();
    useStore.setState({ reselectFlash: true });
  }
}
