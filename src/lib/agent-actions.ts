import { useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";
import { invoke } from "./tauri";
import { showError } from "./toasts";

export const agentButtonLabel = (wt: Worktree): string =>
  wt.has_resumable_session ? "Resume agent" : "Start agent";

export async function startAgent(wt: Worktree, fresh = false): Promise<void> {
  const id = wtId(wt);
  // Mirrors `mergeWorktree`'s guard: the button disables itself once
  // `startingIds` includes `id`, but that's a render away — a click landing
  // before the DOM updates would otherwise re-enter this function and fire
  // a second concurrent `start_session` for the same worktree.
  if (useStore.getState().startingIds.has(id)) return;
  if (useStore.getState().sessions.get(id)?.running) return;
  const startingIds = new Set(useStore.getState().startingIds);
  startingIds.add(id);
  useStore.setState({ startingIds });
  try {
    const info = await invoke("start_session", {
      project: wt.project,
      feature: wt.feature,
      fresh,
    });
    // Every start_session spawns a brand-new backend PTY — clear the
    // frontend's cached terminal size so it re-announces the real size to
    // this fresh PTY instead of assuming it's already correctly sized from
    // a previous, now-dead session (see `resetSizeCache`'s doc comment).
    useStore.getState().sessionMgr?.resetSizeCache(id);
    const sessions = new Map(useStore.getState().sessions);
    sessions.set(id, info);
    const notifiedWaiting = new Set(useStore.getState().notifiedWaiting);
    notifiedWaiting.delete(id);
    // start_session persists a session id immediately, so this worktree is
    // resumable from now on even before the next loadAll() confirms it.
    const worktrees = useStore
      .getState()
      .worktrees.map((w) =>
        wtId(w) === id ? { ...w, has_resumable_session: true } : w,
      );
    useStore.setState({ sessions, notifiedWaiting, worktrees });
  } catch (err) {
    showError(`Wouldn't start. Check your agent executable PATH. (${err})`);
  } finally {
    const stillStarting = new Set(useStore.getState().startingIds);
    stillStarting.delete(id);
    useStore.setState({ startingIds: stillStarting });
  }
}

export async function stopAgent(id: string): Promise<void> {
  await useStore.getState().sessionMgr?.stop(id);
}

/** Resume a session that exited without a deliberate stop — same as the
 * toolbar's "Resume agent" button, looked up by session id (e.g. from the
 * terminal pane's inline Restart button). */
export async function restartAgent(id: string): Promise<void> {
  const wt = useStore.getState().worktrees.find((w) => wtId(w) === id);
  if (wt) await startAgent(wt);
}

/** Start a brand new session for a worktree whose `--resume` attempt just
 * failed (the stored session id no longer exists) — called automatically
 * from `SessionManager`'s `onResumeFailed`, so retrying doesn't loop forever
 * on the same stale id. */
export async function restartFresh(id: string): Promise<void> {
  const wt = useStore.getState().worktrees.find((w) => wtId(w) === id);
  if (wt) await startAgent(wt, true);
}

export async function openZed(wt: Worktree): Promise<void> {
  try {
    await invoke("open_in_zed", {
      path: wt.path,
      editorCommand: useStore.getState().settings.editor_command,
    });
  } catch (err) {
    showError(`Zed wouldn't open: ${err}`);
  }
}

export async function openTerminal(wt: Worktree): Promise<void> {
  try {
    await invoke("open_terminal", {
      path: wt.path,
      terminalCommand: useStore.getState().settings.terminal_command,
    });
  } catch (err) {
    showError(`Terminal wouldn't open: ${err}`);
  }
}
