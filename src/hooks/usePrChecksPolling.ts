import { useEffect, useRef } from "react";
import { checksArePending, fetchPrChecks, fetchPrDetails } from "../lib/checks";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";

const POLL_MS = 12_000;

/** How long to keep polling on an empty check list before giving up.
 * Right after opening a PR or pushing an update, GitHub hasn't necessarily
 * registered any check runs yet — that can take anywhere from a few seconds
 * to over a minute. An empty list looks identical to "no CI configured" at
 * a glance, so we give it this grace window (10 poll intervals) to let
 * checks show up before assuming there genuinely aren't any. */
const EMPTY_GRACE_MS = 120_000;

/** Keeps `checksByWtId` fresh for `wt` while its PR has any check still
 * pending: fetches immediately on mount, then polls on an interval that
 * clears itself once every known check has resolved. An empty check list
 * counts as "keep polling" too, but only within `EMPTY_GRACE_MS` of the
 * effect starting — long enough for GitHub to register check runs after a
 * fresh PR/push, after which an empty list is treated as "no CI configured"
 * and polling stops. A later manual refresh (see `PrPane`) is the only
 * thing that restarts polling after that point — this hook doesn't
 * re-detect new commits/runs on its own.
 *
 * It also opportunistically refreshes the PR's title/body/comments
 * (`prDetailsByWtId`) on the same cadence while they're still missing, for
 * the same eventual-consistency reason: `gh pr view` can briefly report "no
 * pull requests found" for a just-created PR, and this is the only thing
 * that retries it without a manual refresh.
 *
 * Only meant to be mounted once, for the currently active/focused
 * workspace (`MainHeadActions` renders unconditionally whenever a workspace
 * is selected) — background sidebar entries never poll. */
export function usePrChecksPolling(wt: Worktree | null, prOpen: boolean) {
  const id = wt ? wtId(wt) : null;
  // The store rebuilds each worktree object on every refresh (fs watcher,
  // status poll, commits). Depending on that identity restarted the effect —
  // and re-fired `gh pr checks`/`gh pr view` — on unrelated churn. The effect
  // only depends on `id`/`prOpen`; it reads the latest `wt` through this ref,
  // which is safe because `wt`'s project/feature/path are stable for a given
  // `id`.
  const wtRef = useRef(wt);
  wtRef.current = wt;

  useEffect(() => {
    const wt = wtRef.current;
    if (!wt || !id || !prOpen) return;
    const startedAt = Date.now();
    fetchPrChecks(wt);
    if (!useStore.getState().prDetailsByWtId.get(id)) {
      fetchPrDetails(wt);
    }
    const interval = setInterval(() => {
      const current = useStore.getState().checksByWtId.get(id);
      const empty = !current || current.length === 0;
      const withinGrace = Date.now() - startedAt < EMPTY_GRACE_MS;
      if (!checksArePending(current) && !(empty && withinGrace)) {
        clearInterval(interval);
        return;
      }
      fetchPrChecks(wt);
      if (!useStore.getState().prDetailsByWtId.get(id)) {
        fetchPrDetails(wt);
      }
    }, POLL_MS);
    return () => clearInterval(interval);
  }, [id, prOpen]);
}
