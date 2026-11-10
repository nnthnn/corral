import {
  AlertTriangle,
  GitBranch,
  GitBranchPlus,
  GitMerge,
  GitPullRequest,
  GitPullRequestClosed,
  GitPullRequestDraft,
} from "lucide-react";
import type { FeatureStatus } from "../utils";
import { BullSpinner } from "./BullSpinner";

const ICON_BY_STATUS: Record<
  FeatureStatus,
  { Icon: typeof GitBranch; className: string; label: string }
> = {
  new: { Icon: GitBranch, className: "text-muted", label: "New" },
  "in-progress": {
    Icon: GitBranchPlus,
    className: "text-muted",
    label: "In progress",
  },
  committed: {
    Icon: GitPullRequest,
    className: "text-muted",
    label: "Committed",
  },
  pr_draft: {
    Icon: GitPullRequestDraft,
    className: "text-muted",
    label: "Draft PR",
  },
  pr_open: {
    Icon: GitPullRequest,
    className: "text-success",
    label: "PR open",
  },
  pr_closed: {
    Icon: GitPullRequestClosed,
    className: "text-danger",
    label: "PR closed",
  },
  merged: { Icon: GitMerge, className: "text-decorative", label: "Merged" },
  broken: {
    Icon: AlertTriangle,
    className: "text-danger",
    label: "Not a git repository — safe to remove",
  },
};

/** Replaces the sidebar's old agent-activity dot — see `deriveLifecycleState()`
 * in `utils.ts` for how the status is derived. `title` doubles as a free native
 * hover tooltip. While the agent is actively working, a running-bull
 * spinner takes over this slot instead — it reverts to the git-status icon
 * the moment the session goes idle/waiting. */
export function FeatureStatusIcon({
  status,
  working = false,
}: {
  status: FeatureStatus;
  working?: boolean;
}) {
  if (working) {
    return (
      <span
        className="inline-flex flex-none size-5.5 items-center justify-center"
        title="Working"
      >
        <BullSpinner />
      </span>
    );
  }
  const { Icon, className, label } = ICON_BY_STATUS[status];
  return (
    <span
      className="inline-flex flex-none size-5.5 items-center justify-center"
      title={label}
    >
      <Icon size={14} className={className} aria-label={label} />
    </span>
  );
}
