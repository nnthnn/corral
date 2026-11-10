import { Loader2 } from "lucide-react";
import type { ReactNode } from "react";
import { checksAreBlocking, checksBlockedNote, mergePr } from "../lib/checks";
import {
  askAgentToPreparePullRequest,
  mergeWorktree,
  openDeleteDialog,
} from "../lib/worktrees";
import { useStore } from "../store";
import type { CheckRun, PrInfo, Worktree } from "../types";
import { isFullyMerged, wtId } from "../utils";
import { Button } from "./Button";

export type MergeButtonState =
  | { kind: "hidden" }
  | { kind: "note"; text: string }
  | { kind: "merge" }
  | { kind: "remove" }
  | { kind: "open_pr" }
  | { kind: "push_updates" }
  | { kind: "merge_pr"; blocked: boolean };

/** The pieces `ChangesPane` needs to lay the header out as
 * PR controls | status text | action buttons. `note` is the muted reason
 * (dirty tree, waiting checks, …), and `action` is the primary button/loader
 * (or `null` when the state has no action, e.g. blocked checks). */
export type MergeButtonContent = {
  state: MergeButtonState;
  note: string | null;
  action: ReactNode;
};

/** When there's nothing to merge or push, the button is hidden — but if the
 * worktree still has work in flight (uncommitted edits, unresolved
 * conflicts), surface a muted note instead of silence so the empty header
 * slot doesn't look broken or hide why there's no action available.
 *
 * Uncommitted changes take precedence over conflicts: either way the tree
 * must be clean before merging or rebasing, and this matches `RebaseButton`,
 * which shows alongside this one. */
function noteOrHidden(st: NonNullable<Worktree["status"]>): MergeButtonState {
  if (st.staged + st.modified + st.untracked > 0)
    return { kind: "note", text: "Uncommitted changes" };
  if (st.conflicts > 0) return { kind: "note", text: "Resolve conflicts" };
  return { kind: "hidden" };
}

function mergeButtonState(
  wt: Worktree,
  justMerged: boolean,
  prInfo: PrInfo | null,
  checks: CheckRun[] | undefined,
): MergeButtonState {
  const st = wt.status;
  if (!st) return { kind: "hidden" };
  const dirty = st.staged + st.modified + st.untracked;

  if (st.has_remote) {
    if (prInfo?.state === "merged") return { kind: "remove" };
    const prOpen = prInfo?.state === "open";
    if (!prOpen && st.commits_ahead_of_base === 0 && dirty === 0)
      return noteOrHidden(st);
    if (prOpen) {
      if (st.ahead > 0 || dirty > 0) {
        return { kind: "push_updates" };
      }
      return {
        kind: "merge_pr",
        blocked: checksAreBlocking(checks),
      };
    }
    return { kind: "open_pr" };
  }

  if (isFullyMerged(st, prInfo, justMerged)) return { kind: "remove" };
  if (st.commits_ahead_of_base === 0) return noteOrHidden(st);
  if (dirty > 0 || st.conflicts > 0) return noteOrHidden(st);
  return { kind: "merge" };
}

/** Shared state + rendered pieces for `MergeButton` and the Changes pane
 * header. Kept as a hook so `ChangesPane` can place the note and action in
 * separate layout slots without duplicating the merge-state logic. */
export function useMergeButtonContent(
  wt: Worktree | null,
  allowAgentActions = true,
): MergeButtonContent {
  const id = wt ? wtId(wt) : null;
  const justMerged = useStore((s) => (id ? s.justMergedIds.has(id) : false));
  const merging = useStore((s) => (id ? s.mergingIds.has(id) : false));
  const mergingPr = useStore((s) => (id ? s.mergingPrIds.has(id) : false));
  const preparingPr = useStore((s) => (id ? s.preparingPr.has(id) : false));
  const prInfo = useStore((s) => (id ? s.prInfoByWtId.get(id) : null)) ?? null;
  const checks = useStore((s) => (id ? s.checksByWtId.get(id) : undefined));

  if (!wt) return { state: { kind: "hidden" }, note: null, action: null };

  const state = mergeButtonState(wt, justMerged, prInfo, checks);
  const note =
    state.kind === "note"
      ? state.text
      : state.kind === "merge_pr" && state.blocked
        ? checksBlockedNote(checks)
        : null;

  let action: ReactNode = null;
  if (state.kind === "remove") {
    action = (
      <Button variant="primary" onClick={() => openDeleteDialog(wt)}>
        Remove
      </Button>
    );
  } else if (state.kind === "open_pr" && allowAgentActions) {
    action = (
      <Button
        variant="primary"
        disabled={preparingPr}
        onClick={() => askAgentToPreparePullRequest(wt)}
      >
        {preparingPr ? "Preparing PR…" : "Open PR"}
      </Button>
    );
  } else if (state.kind === "push_updates" && allowAgentActions) {
    action = (
      <Button
        variant="primary"
        disabled={preparingPr}
        onClick={() => askAgentToPreparePullRequest(wt)}
      >
        {preparingPr ? "Preparing PR…" : "Push updates"}
      </Button>
    );
  } else if (state.kind === "merge_pr") {
    if (mergingPr) {
      action = (
        <Button
          variant="primary"
          disabled
          className="inline-flex items-center gap-1.5"
        >
          <Loader2 size={14} className="animate-spin" />
          Merging…
        </Button>
      );
    } else if (!state.blocked) {
      action = (
        <Button variant="primary" onClick={() => mergePr(wt)}>
          Merge PR
        </Button>
      );
    }
  } else if (state.kind === "merge") {
    action = merging ? (
      <Button variant="primary" disabled>
        Merging…
      </Button>
    ) : (
      <Button variant="primary" onClick={() => mergeWorktree(wt)}>
        Merge to main
      </Button>
    );
  }

  return { state, note, action };
}

/** The worktree's primary git-workflow action (merge / open PR / push
 * updates / remove), derived from its status + PR state. Shared by
 * `ChangesPane` (the right-hand branch-diff pane) and `WorktreeHoverCard`
 * (the sidebar's compact hover panel) so the two surfaces never disagree
 * about what action is available.
 *
 * Renders no "View PR" link of its own: that lives with the PR-number
 * indicator instead, passed in as `children` (see below).
 *
 * `children` render *between* the status note and the action button — so the
 * Changes pane header can slot its PR controls in there, leaving the note
 * ("Waiting on checks…", "Uncommitted changes", …) reading to their left and
 * the action button on the far right. The Changes pane uses
 * `useMergeButtonContent` directly when it needs those pieces in separate
 * layout slots; this component is the compact `note + children + action`
 * composition used elsewhere (and by the hover card).
 */
export function MergeButton({
  wt,
  allowAgentActions = true,
  children,
}: {
  wt: Worktree;
  allowAgentActions?: boolean;
  children?: ReactNode;
}) {
  const { state, note, action } = useMergeButtonContent(wt, allowAgentActions);
  if (state.kind === "hidden") return children ?? null;
  return (
    <>
      {note && (
        <span className="text-muted text-label whitespace-nowrap" title={note}>
          {note}
        </span>
      )}
      {children}
      {action}
    </>
  );
}
