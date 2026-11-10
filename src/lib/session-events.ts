import { useStore } from "../store";
import { loadChanges, scheduleChangesRefresh } from "./changes";
import { maybeNotifyWaiting, notifyExit } from "./notifications";
import { invoke } from "./tauri";
import { fetchPrStatus } from "./worktrees";

function clearPrPreparation(id: string, token: number): void {
  const current = useStore.getState().preparingPr.get(id);
  if (current?.token !== token) return;
  const preparingPr = new Map(useStore.getState().preparingPr);
  preparingPr.delete(id);
  useStore.setState({ preparingPr });
}

export function onSessionOutput(id: string): void {
  const s = useStore.getState();
  const sess = s.sessions.get(id);
  if (sess && !sess.running) return;

  if (id !== s.selectedId && !s.unreadOutput.has(id)) {
    const unreadOutput = new Set(s.unreadOutput);
    unreadOutput.add(id);
    useStore.setState({ unreadOutput });
  }
  if (id === s.selectedId) scheduleChangesRefresh();
}

/** Applied when a Claude Code hook (Stop / Notification / UserPromptSubmit)
 * reports this session's real waiting/working state — see hook_listener.rs
 * and corral_core::hooks. `lastSummary` is only ever non-null from a `Stop`
 * whose transcript parsed cleanly; `null` here means "unchanged," not
 * "clear it," so a session keeps showing its last known summary while
 * working or between notifications. */
export function onSessionStatus(
  id: string,
  waiting: boolean,
  lastSummary: string | null = null,
): void {
  const s = useStore.getState();
  const sess = s.sessions.get(id);
  if (sess) {
    const sessions = new Map(s.sessions);
    sessions.set(id, {
      ...sess,
      waiting,
      last_summary: lastSummary ?? sess.last_summary,
    });
    useStore.setState({ sessions });
  }

  if (waiting) {
    const latest = useStore.getState();
    const preparation = latest.preparingPr.get(id);
    // Only evaluate completion on a real working -> waiting edge. Repeated
    // idle notifications must not count as additional clarification turns.
    if (preparation && sess && !sess.waiting) {
      const wt = latest.worktrees.find(
        (worktree) => `${worktree.project}/${worktree.feature}` === id,
      );
      // Gate on the worktree existing, not on `has_remote`: preparation only
      // exists when a remote was present at trigger time, and re-reading the
      // (possibly stale) cached status here could strand a finished prep in
      // "Preparing PR…" until the 30-minute timeout.
      if (wt) {
        // A "create" completion only consults PR status, so skip the wasted
        // worktree-status git call (refresh_worktree_status) for it.
        void Promise.all([
          fetchPrStatus(wt),
          preparation.kind === "create"
            ? Promise.resolve(null)
            : invoke("refresh_worktree_status", {
                project: wt.project,
                feature: wt.feature,
              }).catch(() => null),
        ]).then(([info, status]) => {
          if (status) {
            const worktrees = useStore
              .getState()
              .worktrees.map((worktree) =>
                `${worktree.project}/${worktree.feature}` === id
                  ? { ...worktree, status }
                  : worktree,
              );
            useStore.setState({ worktrees });
          }
          // An "update" is done once the branch is no longer ahead and the
          // worktree is clean — or once at least one clarifying round-trip
          // has happened (`waits > 0`). That last clause is what completes a
          // "dirty-only" update where the user told the agent to leave the
          // uncommitted edits out (so the tree stays dirty forever), and it's
          // the reason a rare second mid-task question clears the label a
          // little early: both look identical in git status. Accepted —
          // the "Preparing PR…" label just reverts, the agent keeps working.
          const complete =
            preparation.kind === "create"
              ? info?.state === "open"
              : status?.ahead === 0 &&
                ((preparation.initialAhead ?? 0) > 0 ||
                  status.staged + status.modified + status.untracked === 0 ||
                  (preparation.waits ?? 0) > 0);
          if (complete) {
            clearPrPreparation(id, preparation.token);
          } else if (preparation.kind === "update") {
            const current = useStore.getState().preparingPr.get(id);
            if (current?.token === preparation.token) {
              const preparingPr = new Map(useStore.getState().preparingPr);
              preparingPr.set(id, {
                ...current,
                waits: (current.waits ?? 0) + 1,
              });
              useStore.setState({ preparingPr });
            }
          }
        });
      }
    }
    maybeNotifyWaiting(id);
  } else {
    const notifiedWaiting = new Set(s.notifiedWaiting);
    notifiedWaiting.delete(id);
    const acknowledgedWaiting = new Set(s.acknowledgedWaiting);
    acknowledgedWaiting.delete(id);
    useStore.setState({ notifiedWaiting, acknowledgedWaiting });
  }
}

export function onSessionExit(
  id: string,
  code: number | null,
  killed: boolean,
): void {
  const s = useStore.getState();
  const sessions = new Map(s.sessions);
  const sess = sessions.get(id);
  if (sess)
    sessions.set(id, { ...sess, running: false, exit_code: code, killed });

  const notifiedWaiting = new Set(s.notifiedWaiting);
  notifiedWaiting.delete(id);

  const preparation = s.preparingPr.get(id);
  const preparingPr = new Map(s.preparingPr);
  preparingPr.delete(id);

  useStore.setState({ sessions, notifiedWaiting, preparingPr });

  if (sess) notifyExit(id, sess.project, sess.feature, code, killed);

  if (preparation) {
    const wt = s.worktrees.find(
      (worktree) => `${worktree.project}/${worktree.feature}` === id,
    );
    if (wt?.status?.has_remote) void fetchPrStatus(wt);
  }

  if (id === s.selectedId) {
    const wt = s.worktrees.find((w) => `${w.project}/${w.feature}` === id);
    if (wt) loadChanges(wt);
  }
}
