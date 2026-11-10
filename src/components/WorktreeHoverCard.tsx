import { openUrl } from "@tauri-apps/plugin-opener";
import { AlertTriangle, ExternalLink } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { setWorktreeNote } from "../lib/worktrees";
import { useStore } from "../store";
import type { Worktree } from "../types";
import type { FeatureStatus } from "../utils";
import { relativeTime, summarizeText, wtId } from "../utils";
import { FeatureStatusIcon } from "./FeatureStatusIcon";
import { MergeButton } from "./MergeButton";
import { RebaseButton } from "./RebaseButton";

interface WorktreeHoverCardProps {
  wt: Worktree;
  status: FeatureStatus;
  anchorRect: DOMRect;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
}

/** The sidebar row's on-hover detail panel — everything that used to be
 * always-visible inline text (feature slug, activity summary, full git-stat
 * breakdown, editable note) now lives here instead. Positioned with
 * `position: fixed` from the trigger row's `getBoundingClientRect()`, to the
 * right of the sidebar (rows span its full width, so `anchorRect.right` is
 * the sidebar's right edge) — this both escapes the sidebar's
 * `overflow-y-auto` clipping and keeps the card off to the side rather than
 * covering the row below it. */
export function WorktreeHoverCard({
  wt,
  status,
  anchorRect,
  onMouseEnter,
  onMouseLeave,
}: WorktreeHoverCardProps) {
  const rawSummary = useStore(
    (s) => s.sessions.get(`${wt.project}/${wt.feature}`)?.last_summary ?? null,
  );
  const summary = rawSummary ? summarizeText(rawSummary) : null;
  const prInfo = useStore((st) => st.prInfoByWtId.get(wtId(wt))) ?? null;
  const selected = useStore((st) => st.selectedId === wtId(wt));
  const [note, setNote] = useState(wt.note);
  const noteRef = useRef(note);
  noteRef.current = note;
  const savedNoteRef = useRef(wt.note);
  // The card unmounts on hover-out (see `WorktreeRow`'s `useHoverIntent`
  // wiring) without reliably firing the note input's native `blur` first —
  // relying on `onBlur` alone silently drops an in-progress edit. Flush
  // whatever's typed when the card itself goes away, in addition to the
  // normal on-blur save below.
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentional mount/unmount-only effect — a card instance is scoped to one worktree for its whole lifetime, so `wt` doesn't need to be a dependency
  useEffect(() => {
    return () => {
      if (noteRef.current !== savedNoteRef.current) {
        void setWorktreeNote(wt, noteRef.current);
      }
    };
  }, []);
  const s = wt.status;
  const dirty = s ? s.staged + s.modified + s.untracked : 0;

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: onClick only stops propagation to the trigger row's onClick, not an interactive action
    <div
      role="tooltip"
      className="wt-hover-card fixed z-50 w-80 rounded-md border border-border bg-surface p-3 shadow-lg text-body flex flex-col gap-2"
      style={{ top: anchorRect.top, left: anchorRect.right + 8 }}
      onClick={(e) => e.stopPropagation()}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <div className="flex items-center gap-2 text-sub text-muted">
        <span className="font-mono flex-1 min-w-0 whitespace-nowrap overflow-hidden text-ellipsis">
          {wt.feature}
        </span>
        {s && s.base_added > 0 && (
          <span className="stat-add">+{s.base_added}</span>
        )}
        {s && s.base_removed > 0 && (
          <span className="stat-del">-{s.base_removed}</span>
        )}
        {s && s.last_commit_time > 0 && (
          <span>{relativeTime(s.last_commit_time)}</span>
        )}
        <FeatureStatusIcon status={status} />
      </div>
      {s?.conflicts_with_base && (
        <div className="flex items-center gap-1.5 text-sub text-danger">
          <AlertTriangle size={13} className="flex-none" />
          <span>Would conflict with the base branch — rebase to resolve</span>
        </div>
      )}
      {(s?.last_commit_message || prInfo) && (
        <div className="flex flex-col gap-1">
          {s?.last_commit_message && (
            <span className="font-semibold whitespace-nowrap overflow-hidden text-ellipsis">
              {s.last_commit_message}
            </span>
          )}
          {prInfo && (
            <button
              type="button"
              className="inline-flex items-center gap-1 self-start bg-transparent border-none p-0 text-sub text-primary cursor-pointer"
              onClick={() => openUrl(prInfo.url)}
            >
              #{prInfo.number}
              <ExternalLink size={11} />
            </button>
          )}
        </div>
      )}
      {summary && <span className="text-sub text-muted italic">{summary}</span>}
      {s &&
        (dirty > 0 ||
          s.ahead > 0 ||
          s.behind > 0 ||
          s.conflicts > 0 ||
          s.added > 0 ||
          s.removed > 0) && (
          <div className="flex flex-wrap gap-x-4 gap-y-1 font-mono text-sub text-muted">
            {s.staged > 0 && <span>{s.staged} staged</span>}
            {s.modified > 0 && <span>{s.modified} modified</span>}
            {s.untracked > 0 && <span>{s.untracked} untracked</span>}
            {s.ahead > 0 && <span>↑{s.ahead}</span>}
            {s.behind > 0 && <span>↓{s.behind}</span>}
            {s.conflicts > 0 && (
              <span>
                {s.conflicts} conflict{s.conflicts === 1 ? "" : "s"}
              </span>
            )}
            {s.added > 0 && <span className="stat-add">+{s.added}</span>}
            {s.removed > 0 && <span className="stat-del">-{s.removed}</span>}
          </div>
        )}
      <div className="flex flex-col gap-2 pt-2 border-t border-border">
        <input
          type="text"
          className="text-sub bg-surface-2 border border-border rounded-sm px-2 py-1 text-text"
          value={note}
          placeholder="Add a note…"
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={(e) => {
            // Without this, the keydown bubbles up to the trigger row's own
            // onKeyDown (which treats Enter/Space as "select this row" and
            // calls preventDefault on them) — silently swallowing Space and
            // Enter as you type in the note field instead of the row.
            e.stopPropagation();
            if (e.key === "Enter") e.currentTarget.blur();
          }}
          onBlur={() => {
            if (note !== savedNoteRef.current) {
              savedNoteRef.current = note;
              void setWorktreeNote(wt, note);
            }
          }}
        />
        <div className="flex justify-end gap-1.5">
          <RebaseButton wt={wt} />
          <MergeButton wt={wt} allowAgentActions={selected} />
        </div>
      </div>
    </div>
  );
}
