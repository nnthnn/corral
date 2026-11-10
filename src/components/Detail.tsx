import { FolderPlus, Loader2, Plus } from "lucide-react";
import buckBodyFront from "../assets/buck/buck-body-front.png";
import { agentButtonLabel, startAgent } from "../lib/agent-actions";
import { addProject } from "../lib/projects";
import { focusFirstProjectCreate, openDeleteDialog } from "../lib/worktrees";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { isFullyMerged, shortenPath, wtId } from "../utils";
import { Button } from "./Button";

const WRAPPER_CLASS =
  "detail absolute inset-0 flex items-center justify-center bg-bg";
const INNER_CLASS =
  "detail-inner text-center flex flex-col items-center gap-4 max-w-90";
const ICON_CLASS =
  "empty-icon w-11 h-11 text-muted opacity-60 mb-1 [&>svg]:w-full [&>svg]:h-full";
const MASCOT_CLASS = "w-32 h-32 object-contain mb-1";
const CARD_CLASS =
  "detail-card flex flex-col items-center gap-2 px-5 py-4 rounded-md border border-border bg-transparent hover:bg-surface-2 transition-colors cursor-pointer text-body text-text";

function TerminalIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M7 9l3 3-3 3" />
      <path d="M13 15h4" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M8 12.5l2.5 2.5L16 9.5" />
    </svg>
  );
}

/** Shown inside the agent pane, on top of `TerminalHost`, whenever the
 * selected worktree has no attached or running session — or when nothing is
 * selected at all. `AgentPane` decides *whether* to mount this; `Detail`
 * picks which of its empty states to show: nothing selected, fully merged
 * (only removal left), or plain start/resume. */
export function Detail({
  wt,
  loadingTerminal = false,
}: {
  wt: Worktree | null;
  /** True once `start_session` has resolved with a running session but its
   * xterm entry hasn't been created yet — `AgentPane` keeps `Detail` mounted
   * through that gap instead of briefly rendering nothing (see its own
   * `loadingTerminal` comment), so this needs the same loading treatment as
   * `starting` even though `startingIds` has already cleared by then. */
  loadingTerminal?: boolean;
}) {
  const homeDir = useStore((s) => s.homeDir);
  const justMerged = useStore((s) =>
    wt ? s.justMergedIds.has(wtId(wt)) : false,
  );
  const storedPrInfo = useStore((s) => s.prInfoByWtId);
  const prInfo = wt
    ? storedPrInfo.has(wtId(wt))
      ? (storedPrInfo.get(wtId(wt)) ?? null)
      : (wt.pr_state ?? null)
    : null;
  const projects = useStore((s) => s.projects);
  const starting =
    useStore((s) => (wt ? s.startingIds.has(wtId(wt)) : false)) ||
    loadingTerminal;

  if (!wt) {
    return (
      <div className={WRAPPER_CLASS}>
        <div className={INNER_CLASS}>
          <img src={buckBodyFront} alt="" className={MASCOT_CLASS} />
          <p className="detail-title m-0 text-base font-semibold">
            No worktree selected
          </p>
          <p className="detail-sub -mt-1 text-muted text-sub">
            Pick a project. Buck'll set up the pen.
          </p>
          <div className="detail-actions flex gap-3 mt-2">
            <button
              type="button"
              className={CARD_CLASS}
              onClick={() => void addProject()}
            >
              <FolderPlus size={20} className="text-muted" aria-hidden />
              <span>Add project</span>
            </button>
            {projects.length > 0 && (
              <button
                type="button"
                className={CARD_CLASS}
                onClick={() => focusFirstProjectCreate()}
              >
                <Plus size={20} className="text-muted" aria-hidden />
                <span>New worktree</span>
              </button>
            )}
          </div>
        </div>
      </div>
    );
  }

  if (isFullyMerged(wt.status, prInfo, justMerged)) {
    return (
      <div className={WRAPPER_CLASS}>
        <div className={INNER_CLASS}>
          <div className={ICON_CLASS}>
            <CheckIcon />
          </div>
          <p className="detail-title m-0 text-base font-semibold">
            Merged into main.
          </p>
          <p className="detail-sub -mt-1 text-muted text-sub">
            Handled. Clear it out.
          </p>
          <code className="detail-path -mt-2 text-muted text-code">
            {shortenPath(wt.path, homeDir)}
          </code>
          <Button
            variant="primary"
            className="mt-2"
            onClick={() => openDeleteDialog(wt)}
          >
            Remove
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className={WRAPPER_CLASS}>
      <div className={INNER_CLASS}>
        <div className={ICON_CLASS}>
          <TerminalIcon />
        </div>
        <p className="detail-title m-0 text-base font-semibold">
          No agent in this pen.
        </p>
        <p className="detail-sub -mt-1 text-muted text-sub">
          {starting
            ? "Starting up — this can take a few seconds for opencode."
            : wt.has_resumable_session
              ? "Pick it back up."
              : "Launch one."}
        </p>
        <code className="detail-path -mt-2 text-muted text-code">
          {shortenPath(wt.path, homeDir)}
        </code>
        <Button
          variant="primary"
          className="mt-2 inline-flex items-center gap-1.5"
          disabled={starting}
          onClick={() => startAgent(wt)}
        >
          {starting && <Loader2 size={14} className="animate-spin" />}
          {starting ? "Starting…" : agentButtonLabel(wt)}
        </Button>
      </div>
    </div>
  );
}
