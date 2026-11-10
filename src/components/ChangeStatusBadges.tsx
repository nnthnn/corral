import clsx from "clsx";
import { Circle, Square, SquareArrowUp } from "lucide-react";
import type { ChangedFile, SyncState } from "../types";

const SYNC_ICONS: Record<SyncState, typeof Circle> = {
  uncommitted: Circle,
  committed: Square,
  pushed: SquareArrowUp,
};

/** +added/-removed line-count stat for one changed file — omits a zero
 * side, same convention as the sidebar's diff indicator. */
export function ChangeStat({
  file,
}: {
  file: Pick<ChangedFile, "added" | "removed">;
}) {
  return (
    <>
      {file.added > 0 && <span className="stat-add">+{file.added}</span>}
      {file.removed > 0 && <span className="stat-del">-{file.removed}</span>}
    </>
  );
}

/** Whether a changed file's committed copy has been committed, committed but
 * not pushed, or already pushed to the remote — see `SyncState` (types.ts).
 * `dirty` layers a small dot on top for uncommitted working-tree edits
 * on a file that's already committed/pushed (e.g. a pushed file edited
 * again), so the branch state stays visible instead of collapsing to plain
 * "uncommitted". No tooltip: this sits in the row layer that fades out on
 * hover (replaced by the copy/discard actions), so a `title` here would never
 * be reachable. */
export function SyncIcon({
  state,
  dirty = false,
}: {
  state: SyncState;
  dirty?: boolean;
}) {
  const Icon = SYNC_ICONS[state];
  // `uncommitted` already means "no committed copy", so a dot would be
  // redundant there — only committed/pushed files gain information from it.
  const showDot = dirty && state !== "uncommitted";
  return (
    <span
      className={clsx(
        "change-sync relative inline-flex flex-none",
        state,
        showDot && "has-local-edits",
      )}
    >
      <Icon size={12} />
      {showDot && <span className="change-sync-dot" aria-hidden="true" />}
    </span>
  );
}
