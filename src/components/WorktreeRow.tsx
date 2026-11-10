import clsx from "clsx";
import { X } from "lucide-react";
import { useRef } from "react";
import { useHoverIntent } from "../hooks/useHoverIntent";
import { summarizeChecks } from "../lib/checks";
import { sessionStatus, useStore, wantsAttention } from "../store";
import type { ChecksSummary, Worktree } from "../types";
import {
  deriveLifecycleState,
  rightSlotIndicator,
  worktreeTitle,
  wtId,
} from "../utils";
import { Button } from "./Button";
import { FeatureStatusIcon } from "./FeatureStatusIcon";
import { Highlight } from "./Highlight";
import { RightSlotIndicator } from "./RightSlotIndicator";
import { WorktreeHoverCard } from "./WorktreeHoverCard";

interface WorktreeRowProps {
  wt: Worktree;
  filterText: string;
  active: boolean;
  onSelect: (id: string) => void;
  onRemove: (wt: Worktree) => void;
}

export function WorktreeRow({
  wt,
  filterText,
  active,
  onSelect,
  onRemove,
}: WorktreeRowProps) {
  const id = wtId(wt);
  const storedPrInfo = useStore((s) => s.prInfoByWtId);
  const justMerged = useStore((s) => s.justMergedIds.has(id));
  // Prefer the live per-worktree fetch (`prInfoByWtId`, populated when this
  // worktree is/was selected) over the coarser 5-minute background poll's
  // `wt.pr_state` — but only when the live fetch has actually run for this
  // id (`.has`, not a falsy check: a confirmed "no PR" is stored as an
  // explicit `null`, which must not fall through to possibly-stale poll
  // data via `??`).
  const prInfo = storedPrInfo.has(id)
    ? (storedPrInfo.get(id) ?? null)
    : (wt.pr_state ?? null);
  const status = deriveLifecycleState(wt.status, prInfo, justMerged);
  const working = useStore((s) => sessionStatus(s, id)?.cls === "working");
  const attention = useStore((s) => wantsAttention(s, id));
  const running = useStore((s) => s.run.get(id)?.running ?? false);
  // Live per-check data, when this worktree has been selected (the PR tab /
  // `usePrChecksPolling` populate it) — summarised on the fly so the sidebar
  // shows the freshest state. Falls back to the backend's coarse 5-minute
  // poll summary for rows never selected this app-run.
  const liveChecks = useStore((s) => s.checksByWtId.get(id));
  const checks: ChecksSummary | null = liveChecks
    ? summarizeChecks(liveChecks)
    : (wt.pr_checks ?? null);
  const unread = useStore((s) => s.unreadOutput.has(id) && id !== s.selectedId);
  const removing = useStore((s) => s.removingIds.has(id));
  const indicator = rightSlotIndicator(wt.status, attention, running, checks);
  const rowRef = useRef<HTMLDivElement>(null);
  const anchorRectRef = useRef<DOMRect | null>(null);
  const hover = useHoverIntent();

  function captureAnchor() {
    if (rowRef.current)
      anchorRectRef.current = rowRef.current.getBoundingClientRect();
  }

  return (
    // biome-ignore lint/a11y/useSemanticElements: nests the real remove <button> below, invalid inside a <button>
    <div
      ref={rowRef}
      className={clsx(
        "wt-item group cursor-pointer",
        active && "active",
        attention && "attention",
        unread && "unread",
        removing && "exiting",
      )}
      role="button"
      aria-label={worktreeTitle(wt)}
      tabIndex={0}
      onClick={() => onSelect(id)}
      onKeyDown={(e) => {
        // Ignore keys bubbled from the nested remove button — otherwise our
        // preventDefault() cancels its Enter/Space activation.
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect(id);
        }
      }}
      onMouseEnter={() => {
        captureAnchor();
        hover.scheduleOpen();
      }}
      onMouseLeave={hover.scheduleClose}
      onFocus={() => {
        captureAnchor();
        hover.openNow();
      }}
      onBlur={hover.scheduleClose}
    >
      <FeatureStatusIcon status={status} working={working} />
      <span className="wt-name flex-1 min-w-0 whitespace-nowrap overflow-hidden text-ellipsis">
        <Highlight text={worktreeTitle(wt)} needle={filterText} />
      </span>
      <span className="wt-side grid flex-none justify-items-end items-center">
        <span className="wt-side-status col-start-1 row-start-1 flex items-center gap-1 transition-opacity duration-100 group-hover:opacity-0">
          <RightSlotIndicator indicator={indicator} />
        </span>
        <Button
          variant="ghost-danger"
          square
          className="col-start-1 row-start-1 text-muted opacity-0 pointer-events-none transition-opacity duration-100 group-hover:opacity-100 group-hover:pointer-events-auto hover:text-danger"
          title={`Remove ${id}`}
          onClick={(e) => {
            e.stopPropagation();
            onRemove(wt);
          }}
        >
          <X size={14} />
        </Button>
      </span>
      {hover.open && anchorRectRef.current && (
        <WorktreeHoverCard
          wt={wt}
          status={status}
          anchorRect={anchorRectRef.current}
          onMouseEnter={hover.cancelClose}
          onMouseLeave={hover.scheduleClose}
        />
      )}
    </div>
  );
}
