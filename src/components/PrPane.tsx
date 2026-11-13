import { openUrl } from "@tauri-apps/plugin-opener";
import clsx from "clsx";
import {
  ArrowUpRight,
  ChevronDown,
  ChevronRight,
  CircleCheck,
  CircleX,
  Clock,
  ExternalLink,
  GitMerge,
  GitPullRequest,
  GitPullRequestClosed,
  GitPullRequestDraft,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
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

const STATUS_ICON = {
  draft: GitPullRequestDraft,
  open: GitPullRequest,
  merged: GitMerge,
  closed: GitPullRequestClosed,
} as const;

/** Colored status chip mirroring GitHub's own PR-state coloring, so the
 * pane's state matches what the user would see on github.com. */
function PrStatusBadge({ info }: { info: PrInfo }) {
  const key = info.is_draft ? "draft" : info.state;
  const Icon = STATUS_ICON[key];
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
        "inline-flex shrink-0 -translate-y-px items-center gap-1.5 rounded-full px-2.5 py-1 text-sub font-medium",
      )}
    >
      <Icon size={13} />
      {PR_STATUS_LABEL[key]}
    </span>
  );
}

/** Round GitHub-style author avatar. We only have logins (no avatar URLs from
 * `gh`), so this renders the login's initials on a warm surface — enough to
 * make the timeline read like GitHub's. */
function Avatar({ name }: { name: string }) {
  const initials =
    name
      .replace(/[^a-zA-Z0-9]/g, "")
      .slice(0, 2)
      .toUpperCase() || "?";
  return (
    <span
      aria-hidden
      className="inline-flex size-5 shrink-0 items-center justify-center rounded-full border border-border bg-surface-2 text-[9px] font-semibold leading-none text-muted"
    >
      {initials}
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
    <div className="flex items-center gap-3 border-t border-border px-3 py-2.5 first:border-t-0 hover:bg-surface-2/60">
      <CheckStatusIcon bucket={check.bucket} />
      <span className="min-w-0 flex-1 truncate text-base">{check.name}</span>
      <span className="text-sub text-muted">{check.workflow}</span>
      {check.bucket === "fail" && (
        <Button
          variant="ghost"
          className="px-2 py-1 text-sub"
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
        className="text-muted hover:text-text"
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
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <div className="border-t border-border first:border-t-0">
      <button
        type="button"
        className="flex w-full cursor-pointer items-center gap-2 border-none bg-transparent px-3 py-2 text-left text-inherit [font:inherit] hover:bg-surface-2/40"
        onClick={() => setOpen((o) => !o)}
      >
        <Chevron size={14} className="text-muted" />
        <span className="text-sub font-medium uppercase tracking-[0.6px] text-muted">
          {label} {groupChecks.length}
        </span>
      </button>
      {open && (
        <div className="flex flex-col">
          {groupChecks.map((c) => (
            <CheckRow key={c.url} wt={wt} check={c} />
          ))}
        </div>
      )}
    </div>
  );
}

/** One GitHub-style timeline card: a header strip with author + timestamp and
 * a markdown body. Used for the PR description itself and every comment. */
function TimelineCard({
  author,
  time,
  body,
  wt,
  isDescription = false,
  showHeader = true,
  scrollBody = false,
  onAddToChat,
}: {
  author: string;
  time?: string;
  body: string;
  wt: Worktree;
  isDescription?: boolean;
  showHeader?: boolean;
  scrollBody?: boolean;
  onAddToChat?: () => void;
}) {
  const isEmpty = body.trim() === "";
  return (
    <article className="overflow-hidden rounded-lg border border-border bg-surface">
      {showHeader && (
        <header className="flex items-center gap-2 border-b border-border bg-surface-2/50 px-3 py-2">
          <Avatar name={author} />
          <span className="text-base font-medium text-text">{author}</span>
          <span className="text-sub text-muted">
            {isDescription ? "opened this pull request" : "commented"}
            {time ? ` ${time}` : ""}
          </span>
          {onAddToChat && (
            <div className="ml-auto">
              <Button
                variant="ghost"
                className="px-2 py-1 text-sub"
                onClick={onAddToChat}
              >
                Add to chat
              </Button>
            </div>
          )}
        </header>
      )}
      <div
        className={clsx(
          "px-4 py-3",
          scrollBody && "max-h-[50vh] overflow-y-auto",
        )}
      >
        {isEmpty ? (
          <p className="text-sub text-muted">
            {isDescription ? "No description provided." : "No content."}
          </p>
        ) : (
          <MarkdownPreview wt={wt} path="" text={body} compact />
        )}
      </div>
    </article>
  );
}

/** A thin progress bar, matching the ones in GitHub's PR summary card. */
function Bar({ children }: { children?: ReactNode }) {
  return (
    <span className="flex h-1.5 w-full overflow-hidden rounded-full bg-surface-2">
      {children}
    </span>
  );
}

function BarFill({
  pct,
  tone,
}: {
  pct: number;
  tone: "success" | "danger" | "muted";
}) {
  if (pct <= 0) return null;
  return (
    <span
      className={clsx(
        "h-full",
        tone === "success" && "bg-success",
        tone === "danger" && "bg-danger",
        tone === "muted" && "bg-muted",
      )}
      style={{ width: `${Math.min(100, pct)}%` }}
    />
  );
}

/** One of the three clickable columns in the summary card. */
function SummarySegment({
  label,
  detail,
  icon,
  ariaLabel,
  onClick,
  children,
}: {
  label: string;
  detail: string;
  icon: ReactNode;
  ariaLabel: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={ariaLabel}
      onClick={onClick}
      className="flex h-full flex-col gap-2 bg-transparent px-4 py-3 text-left text-inherit [font:inherit] hover:bg-surface-2/50"
    >
      <span className="flex w-full items-center gap-2">
        <span className="text-base font-semibold">{label}</span>
        <span className="min-w-0 truncate text-sub text-muted">{detail}</span>
        <span className="ml-auto shrink-0 text-muted">{icon}</span>
      </span>
      {children}
    </button>
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
  const [checksOpen, setChecksOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState(false);
  const commentsRef = useRef<HTMLElement>(null);

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
  const failed = checkList.filter(
    (c) => c.bucket === "fail" || c.bucket === "cancel",
  ).length;
  const pending = checkList.filter((c) => c.bucket === "pending").length;
  const total = checkList.length;
  const checksRunning = checksArePending(checkList);
  const comments = details?.comments ?? [];
  const reviewDecision = details?.review_decision ?? "";
  const approved = reviewDecision === "APPROVED";
  const changesRequested = reviewDecision === "CHANGES_REQUESTED";
  const approvalRequired = reviewDecision === "REVIEW_REQUIRED";

  // The one-line verdict GitHub puts above the Checks/Comments/Approvals
  // columns. Checks are the only signal corral can act on; the review decision
  // just tells you whether GitHub is still waiting on a human.
  let headline = "Open pull request";
  let subtitle = "Open the pull request on GitHub for the full detail.";
  let tone: "success" | "warning" | "danger" | "muted" = "muted";
  if (failed > 0) {
    headline = "Checks failed";
    subtitle = `${failed} check${failed === 1 ? "" : "s"} did not pass.`;
    tone = "danger";
  } else if (changesRequested) {
    headline = "Changes requested";
    subtitle = "A reviewer requested changes before this can merge.";
    tone = "danger";
  } else if (approvalRequired) {
    headline = "Approval required";
    subtitle = "Request another approving review before this can merge.";
    tone = "warning";
  } else if (pending > 0) {
    headline = "Checks in progress";
    subtitle = "Some checks are still running.";
    tone = "warning";
  } else if (approved) {
    headline = total > 0 ? "All checks have passed" : "Approved";
    subtitle = total > 0 ? "Approved and all checks have passed." : "Approved.";
    tone = "success";
  } else if (total > 0) {
    // No review required (the common case) and CI is green.
    headline = "All checks have passed";
    subtitle = "No review required. Ready to merge.";
    tone = "success";
  } else if (details) {
    // No required checks and no required review.
    headline = "Ready to merge";
    subtitle = "No required checks or reviews.";
    tone = "success";
  }

  const SummaryIcon =
    tone === "success"
      ? CircleCheck
      : tone === "danger"
        ? CircleX
        : tone === "warning"
          ? Clock
          : GitPullRequest;
  const toneIconClass =
    tone === "success"
      ? "bg-success/15 text-success"
      : tone === "danger"
        ? "bg-danger/15 text-danger"
        : tone === "warning"
          ? "bg-warning/15 text-warning"
          : "bg-surface-2 text-muted";

  const approvalLabel = approved
    ? "Approved"
    : changesRequested
      ? "Changes requested"
      : approvalRequired
        ? "Review required"
        : "Not required";
  const approvalPct = approved || changesRequested ? 100 : 0;
  const approvalTone = approved
    ? "success"
    : changesRequested
      ? "danger"
      : "muted";

  // When GitHub requires no review, an empty "Approvals: Not required" column
  // is just noise — show Checks + Comments instead.
  const showApprovals = reviewDecision !== "";

  const showSummary = !!details || !!prInfo || total > 0;

  return (
    <div className="pr-pane h-full min-h-0 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-4xl flex-col gap-5 px-6 py-6">
        <header className="flex flex-wrap items-center gap-3 border-b border-border pb-4">
          <h2 className="min-w-0 text-xl font-semibold leading-tight">
            {details ? details.title : "Loading…"}
            {prInfo && (
              <span className="font-normal text-muted"> #{prInfo.number}</span>
            )}
          </h2>
          {prInfo && <PrStatusBadge info={prInfo} />}
          {checksRunning && (
            <span className="inline-flex shrink-0 items-center gap-1.5 text-sub text-warning">
              <LassoSpinner className="size-3.5" />
              Checks running
            </span>
          )}
          <div className="ml-auto flex shrink-0 items-center gap-2">
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
                className="px-3 py-1.5 text-sub"
                disabled={markingReady}
                onClick={() => markPrReady(wt)}
              >
                {markingReady ? "Marking ready…" : "Ready for review"}
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
        </header>

        {refreshError && prExists && (
          <p className="warn text-base">
            Couldn't refresh — check your connection or `gh` auth and try again.
          </p>
        )}

        {showSummary && (
          <section className="overflow-hidden rounded-lg border border-border bg-surface">
            <div className="flex items-center gap-3 px-4 py-3">
              <span
                className={clsx(
                  "flex size-8 shrink-0 items-center justify-center rounded-full",
                  toneIconClass,
                )}
              >
                {tone === "warning" && pending > 0 ? (
                  <LassoSpinner className="size-5" />
                ) : (
                  <SummaryIcon size={18} />
                )}
              </span>
              <div className="min-w-0 flex-1">
                <div className="text-base font-semibold">{headline}</div>
                <div className="text-sub text-muted">{subtitle}</div>
              </div>
              {prInfo && (
                <Button
                  variant="ghost"
                  className="shrink-0 gap-1.5 px-3 py-1.5 text-sub"
                  onClick={() => openUrl(prInfo.url)}
                >
                  Open on GitHub
                  <ExternalLink size={14} />
                </Button>
              )}
            </div>

            <div
              className={clsx(
                "grid grid-cols-1 divide-y divide-border border-t border-border sm:divide-x sm:divide-y-0",
                showApprovals ? "sm:grid-cols-3" : "sm:grid-cols-2",
              )}
            >
              <SummarySegment
                label="Checks"
                detail={total === 0 ? "No checks" : `${passed}/${total} passed`}
                ariaLabel="Checks"
                icon={
                  checksOpen ? (
                    <ChevronDown size={16} />
                  ) : (
                    <ChevronRight size={16} />
                  )
                }
                onClick={() => setChecksOpen((open) => !open)}
              >
                <Bar>
                  <BarFill
                    pct={total === 0 ? 0 : (passed / total) * 100}
                    tone="success"
                  />
                  <BarFill
                    pct={total === 0 ? 0 : (failed / total) * 100}
                    tone="danger"
                  />
                </Bar>
              </SummarySegment>

              <SummarySegment
                label="Comments"
                detail={`${comments.length} comment${comments.length === 1 ? "" : "s"}`}
                ariaLabel="Comments"
                icon={<ArrowUpRight size={16} />}
                onClick={() =>
                  commentsRef.current?.scrollIntoView({
                    behavior: "smooth",
                    block: "start",
                  })
                }
              >
                <Bar>
                  <BarFill pct={comments.length > 0 ? 100 : 0} tone="success" />
                </Bar>
              </SummarySegment>

              {showApprovals && (
                <SummarySegment
                  label="Approvals"
                  detail={approvalLabel}
                  ariaLabel="Approvals"
                  icon={<ChevronRight size={16} />}
                  onClick={() => prInfo && openUrl(prInfo.url)}
                >
                  <Bar>
                    <BarFill pct={approvalPct} tone={approvalTone} />
                  </Bar>
                </SummarySegment>
              )}
            </div>

            {checksOpen && (
              <div className="max-h-96 overflow-y-auto border-t border-border px-2 py-2">
                {CHECK_GROUPS.map((group) => (
                  <CheckGroup
                    key={group.label}
                    wt={wt}
                    label={group.label}
                    checks={checkList.filter((c) =>
                      group.buckets.includes(c.bucket),
                    )}
                  />
                ))}
                {total === 0 && (
                  <p className="px-4 py-6 text-center text-sub text-muted">
                    No checks reported yet.
                  </p>
                )}
              </div>
            )}
          </section>
        )}

        {details && (
          <TimelineCard
            author={details.author}
            body={details.body}
            wt={wt}
            isDescription
            showHeader={false}
            scrollBody
          />
        )}

        <section
          ref={commentsRef}
          className="flex scroll-mt-4 flex-col gap-3"
          aria-label="Comments"
        >
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-base font-semibold">
              {comments.length} comment{comments.length === 1 ? "" : "s"}
            </h3>
            {comments.length > 0 && (
              <Button
                variant="ghost"
                className="px-3 py-1.5 text-sub"
                onClick={() => addCommentsToChat(comments)}
              >
                Add all comments to chat
              </Button>
            )}
          </div>
          {comments.length === 0 ? (
            <p className="text-sub text-muted">No comments yet.</p>
          ) : (
            comments.map((comment: PrComment) => (
              <TimelineCard
                key={comment.url}
                author={comment.author}
                time={relativeTime(
                  new Date(comment.created_at).getTime() / 1000,
                )}
                body={comment.body}
                wt={wt}
                onAddToChat={() => addCommentsToChat([comment])}
              />
            ))
          )}
        </section>
      </div>
    </div>
  );
}
