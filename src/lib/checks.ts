import { useStore } from "../store";
import type {
  CheckRun,
  ChecksSummary,
  MergePrResult,
  PrComment,
  PrDetails,
  Worktree,
} from "../types";
import { wtId } from "../utils";
import { loadAll } from "./data";
import { invoke } from "./tauri";
import { showError, showSuccess } from "./toasts";

/** Fire-and-forget by default (callers MAY await the returned promise, e.g.
 * to track in-flight state for a manual refresh): fetch and cache CI
 * check-run status for `wt`'s open PR, keyed by `wtId`. Only meaningful for
 * a worktree with an open PR — callers gate on that before calling this
 * (see `usePrChecksPolling`, `PrPane`). Resolves `true`/`false` rather than
 * rejecting so a fire-and-forget caller (the background poll) never needs a
 * `.catch` — `PrPane`'s manual refresh is the one caller that reads the
 * result, to show an inline error instead of failing silently like the
 * poll does. */
export function fetchPrChecks(wt: Worktree): Promise<boolean> {
  const id = wtId(wt);
  return invoke("pr_checks", { project: wt.project, feature: wt.feature })
    .then((checks: CheckRun[]) => {
      const checksByWtId = new Map(useStore.getState().checksByWtId);
      checksByWtId.set(id, checks);
      useStore.setState({ checksByWtId });
      return true;
    })
    .catch((err) => {
      console.error("fetchPrChecks failed:", err);
      return false;
    });
}

/** Fire-and-forget by default (callers MAY await the returned promise, e.g.
 * to track in-flight state for a manual refresh): fetch and cache PR
 * title/body/comments for `wt`, keyed by `wtId`. Only called when the
 * Checks tab is actually opened. Resolves `true`/`false` rather than
 * rejecting — see `fetchPrChecks`'s doc comment for why. A `null` result
 * ("no PR yet") resolves `true` too, since it's the normal state for a
 * fresh PR GitHub hasn't indexed yet, not a failure worth surfacing. */
export function fetchPrDetails(wt: Worktree): Promise<boolean> {
  const id = wtId(wt);
  return invoke("pr_details", { project: wt.project, feature: wt.feature })
    .then((details: PrDetails | null) => {
      if (details) {
        const prDetailsByWtId = new Map(useStore.getState().prDetailsByWtId);
        prDetailsByWtId.set(id, details);
        useStore.setState({ prDetailsByWtId });
      }
      return true;
    })
    .catch((err) => {
      console.error("fetchPrDetails failed:", err);
      return false;
    });
}

/** Whether any known check is still pending or failing — gates the Merge PR
 * button. `undefined`/empty means "no data yet or nothing configured",
 * which never blocks (GitHub's own branch protection is the real
 * enforcement regardless). */
export function checksAreBlocking(checks: CheckRun[] | undefined): boolean {
  if (!checks || checks.length === 0) return false;
  return checks.some((c) => c.bucket !== "pass" && c.bucket !== "skipping");
}

/** Whether any known check is still queued/running — drives
 * `usePrChecksPolling`'s "poll while pending" loop. */
export function checksArePending(checks: CheckRun[] | undefined): boolean {
  return (checks ?? []).some((c) => c.bucket === "pending");
}

/** Fold a live check-run list into the coarse summary the sidebar renders —
 * the frontend mirror of `summarize_checks` in `crates/corral-core/src/checks.rs`,
 * so the selected worktree's live data looks the same as the backend poll's
 * `Worktree.pr_checks`. `cancel` counts as failed (a terminal non-pass), and
 * `pass`/`skipping` count toward neither. */
export function summarizeChecks(checks: CheckRun[]): ChecksSummary {
  const summary: ChecksSummary = {
    total: checks.length,
    pending: 0,
    failed: 0,
  };
  for (const check of checks) {
    if (check.bucket === "pending") summary.pending++;
    else if (check.bucket === "fail" || check.bucket === "cancel") {
      summary.failed++;
    }
  }
  return summary;
}

/** Short muted note explaining why CI is blocking a merge, shown next to the
 * Merge PR action. A terminal check (failed or cancelled) wins over a
 * still-pending one: once something has definitively not passed, saying
 * "waiting" would be misleading even if other checks are still running. Only
 * meaningful when `checksAreBlocking` is true. */
export function checksBlockedNote(checks: CheckRun[] | undefined): string {
  const all = checks ?? [];
  if (all.some((c) => c.bucket === "fail")) return "Checks failed";
  if (all.some((c) => c.bucket === "cancel")) return "Checks cancelled";
  return "Waiting on checks…";
}

/** Squash-merges `wt`'s open PR on GitHub. Mirrors `mergeWorktree`
 * in `lib/worktrees.ts`: guards against a double-click re-entering while a
 * call is already in flight, and reloads worktree state afterward either
 * way (a rejection can still leave `pr_status` worth re-checking). */
export async function mergePr(wt: Worktree): Promise<void> {
  const id = wtId(wt);
  if (useStore.getState().mergingPrIds.has(id)) return;
  const mergingPrIds = new Set(useStore.getState().mergingPrIds);
  mergingPrIds.add(id);
  useStore.setState({ mergingPrIds });
  try {
    const result: MergePrResult = await invoke("merge_pr", {
      project: wt.project,
      feature: wt.feature,
    });
    if (result.outcome === "merged") {
      showSuccess("Merged the PR on GitHub.", { sticky: true });
    } else {
      showError(`GitHub rejected the merge: ${result.reason}`, {
        sticky: true,
      });
    }
  } catch (err) {
    showError(`Merge failed: ${err}`, { sticky: true });
  } finally {
    const stillMerging = new Set(useStore.getState().mergingPrIds);
    stillMerging.delete(id);
    useStore.setState({ mergingPrIds: stillMerging });
    await loadAll();
  }
}

/** Marks `wt`'s draft PR as ready for review — `gh pr ready`. Mirrors
 * `mergePr`'s in-flight guard and post-call `loadAll()` refresh (so the
 * draft badge clears once GitHub confirms the change). */
export async function markPrReady(wt: Worktree): Promise<void> {
  const id = wtId(wt);
  if (useStore.getState().markingPrReadyIds.has(id)) return;
  const markingPrReadyIds = new Set(useStore.getState().markingPrReadyIds);
  markingPrReadyIds.add(id);
  useStore.setState({ markingPrReadyIds });
  try {
    await invoke("mark_pr_ready", { project: wt.project, feature: wt.feature });
    showSuccess("Marked the PR ready for review.");
  } catch (err) {
    showError(`Couldn't mark the PR ready: ${err}`, { sticky: true });
  } finally {
    const stillMarking = new Set(useStore.getState().markingPrReadyIds);
    stillMarking.delete(id);
    useStore.setState({ markingPrReadyIds: stillMarking });
    await loadAll();
  }
}

/** Fetches a failed check's job log, switches to the Agent tab, and pastes +
 * submits a troubleshooting prompt into the active session's terminal — the
 * "Add to chat" action next to a failed check row in `PrPane`'s Checks
 * view. A CI log can run to thousands of lines, so it's written to a temp
 * file and referenced by path (which Claude Code reads itself) rather than
 * pasted inline and flooding the terminal — falls back to an inline paste
 * if the write fails for any reason. */
export async function addCheckToChat(
  wt: Worktree,
  check: CheckRun,
): Promise<void> {
  try {
    const log: string = await invoke("check_log", {
      project: wt.project,
      feature: wt.feature,
      checkUrl: check.url,
    });
    const path = await invoke("save_text_attachment", { text: log }).catch(
      () => null,
    );
    const prompt = path
      ? `Check "${check.name}" failed on this branch. See the failing job's log at "${path}" for details. Please diagnose and fix.`
      : `Check "${check.name}" failed on this branch. Here's the failing job's log:\n\n${log}\n\nPlease diagnose and fix.`;
    useStore.setState({ activeTab: "agent" });
    useStore.getState().sessionMgr?.pasteToActive(prompt);
  } catch (err) {
    showError(`Couldn't fetch the failing log: ${err}`);
  }
}

/** Switches to the Agent tab, then pastes and submits one or more PR
 * comments into the active session's terminal — the per-row "Add to chat" /
 * bulk "Add all comments to chat" actions in `PrPane`. Multiple comments are
 * concatenated into a single paste rather than one paste per comment. */
export function addCommentsToChat(comments: PrComment[]): void {
  if (comments.length === 0) return;
  const prompt =
    comments.length === 1
      ? `PR comment from ${comments[0].author}:\n\n${comments[0].body}\n\nPlease address this feedback.`
      : `PR comments:\n\n${comments
          .map((c) => `From ${c.author}:\n${c.body}`)
          .join("\n\n")}\n\nPlease address this feedback.`;
  useStore.setState({ activeTab: "agent" });
  useStore.getState().sessionMgr?.pasteToActive(prompt);
}
