import { AlertTriangle, CircleX } from "lucide-react";
import type { RightSlotIndicator as RightSlotIndicatorValue } from "../utils";
import { LassoSpinner } from "./LassoSpinner";
import { WagonWheelSpinner } from "./spinners";

/** Renders the sidebar row's right-hand status slot — whichever single
 * indicator `rightSlotIndicator()` (utils.ts) picked for this worktree, in
 * priority order: an active run command, an agent waiting for input, failed
 * CI, a rebase conflict, CI still running, a diff stat, or an ahead/behind
 * count. Pure presentational — see `preview.tsx` for a gallery of every
 * `kind`. */
export function RightSlotIndicator({
  indicator,
}: {
  indicator: RightSlotIndicatorValue;
}) {
  switch (indicator.kind) {
    case "running":
      return (
        <span title="Run command active">
          <WagonWheelSpinner className="size-3.5" />
        </span>
      );
    case "attention":
      return (
        <span
          className="wt-bell text-sub leading-none grayscale"
          title="Waiting for input"
        >
          🔔
        </span>
      );
    case "checks-failed":
      return (
        <span title="Pull request checks failed">
          <CircleX size={14} className="text-danger" />
        </span>
      );
    case "checks-running":
      return (
        <span title="Pull request checks running">
          <LassoSpinner className="size-3.5" />
        </span>
      );
    case "conflict":
      return (
        <span title="Would conflict with the base branch">
          <AlertTriangle size={14} className="mini conflict" />
        </span>
      );
    case "diff":
      return (
        <>
          {indicator.added > 0 && (
            <span className="mini stat-add">+{indicator.added}</span>
          )}
          {indicator.removed > 0 && (
            <span className="mini stat-del">-{indicator.removed}</span>
          )}
        </>
      );
    case "sync":
      return (
        <>
          {indicator.ahead > 0 && (
            <span className="mini ahead">↑{indicator.ahead}</span>
          )}
          {indicator.behind > 0 && (
            <span className="mini behind">↓{indicator.behind}</span>
          )}
        </>
      );
    case "none":
      return null;
  }
}
