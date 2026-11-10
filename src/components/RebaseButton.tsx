import { Loader2 } from "lucide-react";
import { askClaudeToRebase, rebaseWorktree } from "../lib/worktrees";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";
import { Button } from "./Button";

type RebaseButtonState =
  | { kind: "hidden" }
  | { kind: "note"; text: string }
  | { kind: "rebase" };

export function rebaseButtonState(wt: Worktree): RebaseButtonState {
  const st = wt.status;
  if (!st?.conflicts_with_base) return { kind: "hidden" };
  const dirty = st.staged + st.modified + st.untracked;
  if (dirty > 0) {
    return { kind: "note", text: "Uncommitted changes" };
  }
  return { kind: "rebase" };
}

/** Offers to rebase a worktree's branch onto its base once
 * `GitStatus.conflicts_with_base` says a plain merge would conflict.
 * Rendered alongside `MergeButton` in the same two surfaces.
 *
 * `mode` picks how the rebase actually happens:
 * - `"direct"` (default, `WorktreeHoverCard`): corral runs `git rebase`
 *   itself and auto-aborts on the first conflict.
 * - `"chat"` (`ChangesPane` only): asks the active agent session to do the
 *   rebase and work through conflicts instead. Only safe where `wt` is
 *   always the currently-selected worktree — see `askClaudeToRebase`'s doc
 *   comment. `base` is required in this mode since that's what gets named
 *   in the prompt/instructions. */
export function RebaseButton({
  wt,
  mode = "direct",
  base,
}: {
  wt: Worktree;
  mode?: "direct" | "chat";
  base?: string | null;
}) {
  const id = wtId(wt);
  const rebasing = useStore((s) => s.rebasingIds.has(id));
  const state = rebaseButtonState(wt);

  if (state.kind === "hidden") return null;
  if (state.kind === "note") {
    return (
      <span
        className="text-muted text-label whitespace-nowrap"
        title={state.text}
      >
        {state.text}
      </span>
    );
  }
  if (mode === "chat") {
    return (
      <Button
        variant="ghost"
        disabled={!base}
        title={base ? undefined : "Couldn't detect a base branch"}
        onClick={() => base && askClaudeToRebase(wt, base)}
      >
        Rebase
      </Button>
    );
  }
  if (rebasing) {
    return (
      <Button
        variant="ghost"
        disabled
        className="inline-flex items-center gap-1.5"
      >
        <Loader2 size={14} className="animate-spin" />
        Rebasing…
      </Button>
    );
  }
  return (
    <Button variant="ghost" onClick={() => rebaseWorktree(wt)}>
      Rebase
    </Button>
  );
}
