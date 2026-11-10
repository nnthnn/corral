import { openUrl } from "@tauri-apps/plugin-opener";
import clsx from "clsx";
import {
  CircleCheck,
  CircleX,
  ExternalLink,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { useEffect, useState } from "react";
import {
  addCheckToChat,
  addCommentsToChat,
  checksArePending,
  fetchPrChecks,
  fetchPrDetails,
  markPrReady,
} from "../lib/checks";
import { useStore } from "../store";
import type { CheckRun, PrComment, PrInfo, Worktree } from "../types";
import { relativeTime, wtId } from "../utils";
import { Button } from "./Button";
import { LassoSpinner } from "./LassoSpinner";
import { MarkdownPreview } from "./MarkdownPreview";

const PR_STATUS_LABEL: Record<"draft" | PrInfo["state"], string> = {
  draft: "Draft",
  open: "Open",
  merged: "Merged",
  closed: "Closed",
};

/** Colored status chip mirroring GitHub's own PR-state coloring, so the
 * pane's state matches what the user would see on github.com. */
function PrStatusBadge({ info }: { info: PrInfo }) {
  const key = info.is_draft ? "draft" : info.state;
  return (
    <span
      className={clsx(
        "pr-status-badge",
        key,
        // The title next to this badge is a larger heading font with
        // descenders (e.g. the "g"s in "backlog grooming"), which visually
        // shift its glyphs upward within its line box — a symmetric-padding
        // badge centered via flex `items-center` (which centers line boxes,
        // not glyph outlines) reads as sitting slightly too low against it.
        // A small upward nudge corrects for that without touching the
        // title's own line-height.
        "shrink-0 -translate-y-px text-sub font-medium py-1 px-2.5 rounded-full",
      )}
    >
      {PR_STATUS_LABEL[key]}
    </span>
  );
}

function CheckStatusIcon({ bucket }: { bucket: CheckRun["bucket"] }) {
  if (bucket === "pass")
    return <CircleCheck size={16} className="text-success" />;
  if (bucket === "fail") return <CircleX size={16} className="text-danger" />;
  if (bucket === "pending") return <LassoSpinner className="size-4" />;
  return <span className="text-muted">—</span>;
}

function CheckRow({ wt, check }: { wt: Worktree; check: CheckRun }) {
  return (
    <div className="check-row flex items-center gap-3 py-2.5 px-3 rounded-md hover:bg-surface-2">
      <CheckStatusIcon bucket={check.bucket} />
      <span className="flex-1 min-w-0 truncate text-base">{check.name}</span>
      <span className="text-muted text-sub">{check.workflow}</span>
      {check.bucket === "fail" && (
        <Button
          variant="ghost"
          className="text-sub py-1.5 px-3"
          onClick={() => addCheckToChat(wt, check)}
        >
          Add to chat
        </Button>
      )}
      <a
        href={check.url}
        target="_blank"
        rel="noreferrer"
        title="View on GitHub"
      >
        <ExternalLink size={16} />
      </a>
    </div>
  );
}

const CHECK_GROUPS: { label: string; buckets: CheckRun["bucket"][] }[] = [
  { label: "Failed", buckets: ["fail", "cancel"] },
  { label: "In progress", buckets: ["pending"] },
  { label: "Passed", buckets: ["pass"] },
  { label: "Skipped", buckets: ["skipping"] },
];

function CheckGroup({
  wt,
  label,
  checks: groupChecks,
}: {
  wt: Worktree;
  label: string;
  checks: CheckRun[];
}) {
  const [open, setOpen] = useState(true);
  if (groupChecks.length === 0) return null;
  return (
    <div className="check-group">
      <button
        type="button"
        className="check-group-head w-full flex items-center gap-2 py-2 px-3 bg-transparent border-none cursor-pointer text-left [font:inherit] text-inherit"
        onClick={() => setOpen((o) => !o)}
      >
        <span className="text-sub font-medium uppercase tracking-[0.6px] text-muted">
          {label} {groupChecks.length}
        </span>
      </button>
      {open && (
        <div className="flex flex-col gap-1">
          {groupChecks.map((c) => (
            <CheckRow key={c.url} wt={wt} check={c} />
          ))}
        </div>
      )}
    </div>
  );
}

function CommentRow({ wt, comment }: { wt: Worktree; comment: PrComment }) {
  return (
    <div className="comment-row flex items-start gap-3 py-2.5 px-3">
      <div className="flex-1 min-w-0">
        <div className="text-sub text-muted">
          {comment.author}
          {" · "}
          {relativeTime(new Date(comment.created_at).getTime() / 1000)}
        </div>
        <MarkdownPreview wt={wt} path="" text={comment.body} compact />
      </div>
      <Button
        variant="ghost"
        className="text-sub py-1.5 px-3"
        onClick={() => addCommentsToChat([comment])}
      >
        Add to chat
      </Button>
    </div>
  );
}

export function PrPane({
  wt,
  active = true,
}: {
  wt: Worktree;
  active?: boolean;
}) {
  const id = wtId(wt);
  const details = useStore((s) => s.prDetailsByWtId.get(id));
  const checkList = useStore((s) => s.checksByWtId.get(id)) ?? [];
  const prInfo = useStore((s) => s.prInfoByWtId.get(id));
  const markingReady = useStore((s) => s.markingPrReadyIds.has(id));
  // `null` until the user explicitly picks a pill — until then the shown
  // tab is derived from what's actually loaded (see `view` below), so e.g.
  // a PR with no activity but pending checks opens straight to Checks
  // instead of an empty Activity tab.
  const [manualView, setManualView] = useState<
    "activity" | "description" | "checks" | null
  >(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState(false);

  function refresh() {
    setRefreshing(true);
    Promise.all([fetchPrDetails(wt), fetchPrChecks(wt)])
      .then(([detailsOk, checksOk]) => setRefreshError(!detailsOk || !checksOk))
      .finally(() => setRefreshing(false));
  }

  // `active` (not just `id`): the pane stays mounted while its tab is hidden,
  // so without this it fetched PR details + checks for every selected worktree
  // even when the PR tab was never opened. It refetches when the tab becomes
  // active. See `MainPane`'s `hidden` panes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: refresh when the worktree identity or tab activity changes, not when this render creates a new refresh function
  useEffect(() => {
    if (!active) return;
    refresh();
  }, [id, active]);

  // A failed fetch before a PR exists is normal, not a user-facing error.
  const prExists = !!prInfo;

  const passed = checkList.filter((c) => c.bucket === "pass").length;
  const checksRunning = checksArePending(checkList);
  const hasActivity = (details?.comments.length ?? 0) > 0;
  const view =
    manualView ??
    (!hasActivity && checkList.length > 0 ? "checks" : "activity");
  const setView = setManualView;

  return (
    <div className="pr-pane flex flex-col h-full min-h-0 overflow-y-auto p-6 gap-6">
      <div className="min-w-0">
        {details ? (
          <div className="flex items-center gap-3">
            <h2 className="text-xl font-semibold">
              {details.title}
              {prInfo && (
                <span className="text-muted font-normal">
                  {" "}
                  #{prInfo.number}
                </span>
              )}
            </h2>
            {prInfo && <PrStatusBadge info={prInfo} />}
            {checksRunning && (
              <span className="inline-flex items-center gap-1.5 text-sub text-warning shrink-0">
                <LassoSpinner className="size-3.5" />
                Checks running
              </span>
            )}
            {prInfo && (
              <Button
                variant="ghost"
                icon
                title="Open PR in browser"
                onClick={() => openUrl(prInfo.url)}
              >
                <ExternalLink size={16} />
              </Button>
            )}
            {prInfo?.is_draft && (
              <Button
                variant="ghost"
                className="text-sub py-1.5 px-3"
                disabled={markingReady}
                onClick={() => markPrReady(wt)}
              >
                {markingReady ? "Marking ready…" : "Ready for review"}
              </Button>
            )}
          </div>
        ) : (
          <p className="text-muted">Loading…</p>
        )}
      </div>

      <div className="flex items-center justify-between gap-3">
        <nav
          className="pr-pills flex gap-0.5 self-start rounded-lg bg-bg p-0.5"
          aria-label="Panel"
        >
          <button
            type="button"
            className={clsx(
              "cursor-pointer rounded-md px-5 py-2.5 text-base font-medium transition-colors duration-150",
              view === "activity"
                ? "bg-surface-2 text-text shadow-sm"
                : "text-muted hover:text-text",
            )}
            onClick={() => setView("activity")}
          >
            Activity
          </button>
          <button
            type="button"
            className={clsx(
              "cursor-pointer rounded-md px-5 py-2.5 text-base font-medium transition-colors duration-150",
              view === "description"
                ? "bg-surface-2 text-text shadow-sm"
                : "text-muted hover:text-text",
            )}
            onClick={() => setView("description")}
          >
            Description
          </button>
          <button
            type="button"
            className={clsx(
              "cursor-pointer rounded-md px-5 py-2.5 text-base font-medium transition-colors duration-150",
              view === "checks"
                ? "bg-surface-2 text-text shadow-sm"
                : "text-muted hover:text-text",
            )}
            onClick={() => setView("checks")}
          >
            Checks {passed}/{checkList.length}
          </button>
        </nav>

        <div className="flex items-center gap-2 shrink-0">
          {details && details.comments.length > 0 && (
            <Button
              variant="ghost"
              className="text-sub py-1.5 px-3"
              onClick={() => addCommentsToChat(details.comments)}
            >
              Add all comments to chat
            </Button>
          )}
          <Button
            variant="ghost"
            square
            title="Refresh"
            onClick={refresh}
            disabled={refreshing}
          >
            {refreshing ? (
              <Loader2 size={14} className="animate-spin text-muted" />
            ) : (
              <RefreshCw size={14} />
            )}
          </Button>
        </div>
      </div>

      {refreshError && prExists && (
        <p className="warn text-base">
          Couldn't refresh — check your connection or `gh` auth and try again.
        </p>
      )}

      {view === "checks" && (
        <div className="checks-list flex flex-col gap-2">
          {CHECK_GROUPS.map((g) => (
            <CheckGroup
              key={g.label}
              wt={wt}
              label={g.label}
              checks={checkList.filter((c) => g.buckets.includes(c.bucket))}
            />
          ))}
        </div>
      )}

      {view === "description" &&
        (details ? (
          // Capped independent of the pane's own scroll — an AI-written
          // description can run long, and without a cap it pushes the pane's
          // own scroll affordance out of easy reach. `vh` (not a `%` of this
          // flex item, whose own height is content-driven, so a `%` height
          // would resolve against nothing) keeps this reliably a fraction of
          // the actual viewport regardless of ancestor layout.
          <div className="max-h-[60vh] overflow-y-auto">
            <MarkdownPreview wt={wt} path="" text={details.body} compact />
          </div>
        ) : (
          <p className="text-muted">Loading…</p>
        ))}

      {view === "activity" && (
        <div className="comments-list flex flex-col gap-1">
          {(details?.comments ?? []).map((c) => (
            <CommentRow key={c.url} wt={wt} comment={c} />
          ))}
        </div>
      )}
    </div>
  );
}
