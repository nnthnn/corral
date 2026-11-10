import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import clsx from "clsx";
import { AlertTriangle, Copy, ExternalLink, Undo2 } from "lucide-react";
import { useEffect, useState } from "react";
import { revertFile } from "../lib/changes";
import { openDiff } from "../lib/diff";
import { loadDir } from "../lib/files";
import { showError } from "../lib/toasts";
import { currentWt, useStore } from "../store";
import type { ChangedFile, PrInfo, Worktree } from "../types";
import { wtId } from "../utils";
import { Button } from "./Button";
import { ChangeStat, SyncIcon } from "./ChangeStatusBadges";
import { FileTree } from "./FileTree";
import { useMergeButtonContent } from "./MergeButton";
import { PathLabel } from "./PathLabel";
import { RebaseButton } from "./RebaseButton";
import { ResizeHandle } from "./ResizeHandle";

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

/** Three-part Changes header: PR controls at the left, the merge/status note
 * in the remaining space, and the git actions pinned to the right. */
function ChangesHeaderActions({
  wt,
  base,
  prInfo,
}: {
  wt: Worktree;
  base: string | null | undefined;
  prInfo: PrInfo | null;
}) {
  const { note, action } = useMergeButtonContent(wt);

  return (
    <>
      {prInfo && (
        <div className="changes-pr-controls flex items-center gap-0.5 shrink-0">
          <Button
            variant="ghost"
            title={`Open PR #${prInfo.number} tab`}
            onClick={() => useStore.setState({ activeTab: "pr" })}
          >
            PR #{prInfo.number}
          </Button>
          <Button
            variant="ghost"
            icon
            title="Open PR in browser"
            onClick={() => openUrl(prInfo.url)}
          >
            <ExternalLink size={16} />
          </Button>
        </div>
      )}
      <div className="changes-action-note flex-1 min-w-0">
        {note && (
          <span className="block truncate text-muted text-label" title={note}>
            {note}
          </span>
        )}
      </div>
      <div className="changes-action-buttons flex items-center gap-1.5 shrink-0">
        <RebaseButton wt={wt} mode="chat" base={base} />
        {action}
      </div>
    </>
  );
}

export function ChangesPane() {
  const wt = useStore(currentWt);
  const worktreeId = wt ? wtId(wt) : null;
  const changes = useStore((s) => s.changes);
  const changesCollapsed = useStore((s) => s.settings.changes_collapsed);
  const panelTab = useStore((s) => s.changesPanelTab);
  /** Known PR for this worktree, if the backend poll has fetched one — the
   * same condition that makes the center pane's PR tab appear, so this header
   * shortcut and that tab can't disagree about whether a PR exists. */
  const prInfo = useStore((s) =>
    worktreeId ? (s.prInfoByWtId.get(worktreeId) ?? null) : null,
  );
  const files = changes?.files ?? [];
  const base = changes?.base;
  /** Branch-wide line totals across every listed file — committed and
   * uncommitted alike, since the list is the whole branch diff. */
  const totals = files.reduce(
    (acc, f) => ({
      added: acc.added + f.added,
      removed: acc.removed + f.removed,
    }),
    { added: 0, removed: 0 },
  );
  /** Files left unmerged by a merge/rebase — must be resolved before the
   * branch can move on, so they get a distinct warning above the list. */
  const conflictCount = files.filter((f) => f.status === "conflict").length;
  /** Path of the file whose discard confirmation is currently open. */
  const [revertPath, setRevertPath] = useState<string | null>(null);
  const [reverting, setReverting] = useState(false);

  // A discard confirmation belongs to a file *in one worktree*, but the pane
  // isn't keyed by worktree — switching while one is open would leave it
  // pointing at a same-named file in the newly selected worktree, so a
  // confirm would discard the wrong file. Close it on any worktree change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset-on-worktree-change
  useEffect(() => {
    setRevertPath(null);
  }, [worktreeId]);

  function switchPanelTab(tab: "changes" | "files") {
    useStore.setState({ changesPanelTab: tab });
    if (tab === "files" && wt) loadDir(wt, "");
  }

  async function confirmRevert(f: ChangedFile) {
    if (!wt || reverting) return;
    setReverting(true);
    try {
      await revertFile(wt, f.path, f.old_path);
      setRevertPath(null);
    } catch (err) {
      showError(`Couldn't discard ${f.path}: ${err}`);
    } finally {
      setReverting(false);
    }
  }

  function openRowDiff(f: ChangedFile) {
    setRevertPath(null);
    if (wt) openDiff(wt, f.path);
  }

  return (
    <aside className="changes relative overflow-hidden flex flex-col border-l border-border bg-surface min-h-0">
      {!changesCollapsed && (
        <ResizeHandle
          settingsKey="changes_width"
          min={240}
          max={600}
          defaultWidth={384}
          side="left"
        />
      )}
      <header
        className="changes-actions flex items-center gap-2 py-2 px-4 h-12 border-b border-border"
        data-tauri-drag-region
      >
        {wt && <ChangesHeaderActions wt={wt} base={base} prInfo={prInfo} />}
      </header>
      <div className="changes-head flex items-center gap-3 py-2 px-4 border-b border-border">
        <nav
          className="changes-panel-tabs flex gap-0.5 rounded-lg bg-bg p-0.5"
          aria-label="Panel"
        >
          <button
            type="button"
            className={clsx(
              "inline-flex cursor-pointer items-center justify-center gap-1.5 rounded-md px-4 py-2 text-sub font-medium transition-colors duration-150",
              panelTab === "changes"
                ? "bg-surface-2 text-text shadow-sm"
                : "text-muted hover:text-text",
            )}
            onClick={() => switchPanelTab("changes")}
          >
            Changes
            {wt && files.length > 0 && (
              <span className="changes-tab-count inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-text/15 px-1.5 font-mono text-[10px] font-semibold leading-none text-text/80">
                {files.length}
              </span>
            )}
          </button>
          <button
            type="button"
            className={clsx(
              "inline-flex cursor-pointer items-center justify-center rounded-md px-4 py-2 text-sub font-medium transition-colors duration-150",
              panelTab === "files"
                ? "bg-surface-2 text-text shadow-sm"
                : "text-muted hover:text-text",
            )}
            onClick={() => switchPanelTab("files")}
          >
            Files
          </button>
        </nav>
      </div>
      {panelTab === "changes" && wt && (
        <div className="changes-summary flex items-center justify-between gap-2.5 px-4 py-1.5 border-b border-border font-mono text-label">
          <span className="changes-base whitespace-nowrap normal-case tracking-normal opacity-50">
            {base ? `vs ${base}` : "working tree"}
          </span>
          {files.length > 0 && (
            <span
              className="changes-total flex gap-1.5 whitespace-nowrap"
              title={`Total lines changed across ${files.length} file${
                files.length === 1 ? "" : "s"
              }${base ? ` vs ${base}` : ""} — committed and uncommitted`}
            >
              <ChangeStat file={totals} />
            </span>
          )}
        </div>
      )}
      {panelTab === "changes" && conflictCount > 0 && (
        <div className="changes-conflict-note flex items-center gap-1.5 px-4 py-1.5 border-b border-border text-label text-danger bg-danger/10">
          <AlertTriangle size={12} className="flex-none" />
          <span>
            {conflictCount} conflict{conflictCount === 1 ? "" : "s"} — resolve
            before continuing
          </span>
        </div>
      )}
      <div className="changes-list flex-1 overflow-y-auto p-2 min-h-0">
        {panelTab === "changes" ? (
          <>
            {wt && files.length === 0 && (
              <div className="changes-empty flex flex-col items-center gap-2 text-muted py-5 px-3 text-body text-center">
                <div className="empty-icon small w-6.5 h-6.5 text-muted opacity-60 mb-1 [&>svg]:w-full [&>svg]:h-full">
                  <CheckIcon />
                </div>
                <p className="m-0">Clean. Nothing to show.</p>
              </div>
            )}
            {wt &&
              files.map((f) => (
                // biome-ignore lint/a11y/useSemanticElements: nests the real copy <button> below, invalid inside a <button>
                <div
                  key={f.path}
                  className="change group w-full flex items-center gap-2 py-1 px-1.5 rounded-md hover:bg-surface-2 cursor-pointer"
                  role="button"
                  tabIndex={0}
                  onClick={() => openRowDiff(f)}
                  onKeyDown={(e) => {
                    // Ignore keys bubbled from the nested copy/discard
                    // buttons — otherwise our preventDefault() cancels their
                    // Enter/Space activation.
                    if (e.target !== e.currentTarget) return;
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      openRowDiff(f);
                    }
                  }}
                >
                  <span
                    className={clsx(
                      "change-path flex-1 min-w-0 font-mono text-caption",
                      f.status === "conflict" && "text-danger",
                    )}
                    title={f.old_path ? `${f.old_path} → ${f.path}` : f.path}
                  >
                    <PathLabel path={f.path} oldPath={f.old_path} />
                  </span>
                  <span className="change-slot grid flex-none justify-items-end items-center">
                    {revertPath === f.path ? (
                      <span className="change-revert col-start-1 row-start-1 flex items-center gap-1.5 rounded-md border border-border bg-surface-2 pl-2 pr-1 py-0.5 shadow-sm">
                        <span className="text-caption text-muted whitespace-nowrap">
                          Discard changes?
                        </span>
                        <Button
                          variant="ghost"
                          className="h-6 px-1.5 text-label"
                          disabled={reverting}
                          onClick={(e) => {
                            e.stopPropagation();
                            setRevertPath(null);
                          }}
                        >
                          Cancel
                        </Button>
                        <Button
                          variant="danger"
                          className="h-6 px-2 text-label"
                          disabled={reverting}
                          onClick={(e) => {
                            e.stopPropagation();
                            void confirmRevert(f);
                          }}
                        >
                          Discard
                        </Button>
                      </span>
                    ) : (
                      <>
                        <span className="col-start-1 row-start-1 flex items-center gap-1 whitespace-nowrap transition-opacity duration-100 group-hover:opacity-0">
                          <span className="change-stat flex flex-none gap-1.5 whitespace-nowrap font-mono text-label">
                            <ChangeStat file={f} />
                          </span>
                          {f.status === "conflict" ? (
                            <span
                              role="img"
                              aria-label="Merge conflict — resolve before continuing"
                              className="change-conflict flex-none text-danger"
                            >
                              <AlertTriangle size={12} />
                            </span>
                          ) : (
                            <SyncIcon state={f.sync_state} dirty={f.dirty} />
                          )}
                        </span>
                        <span className="col-start-1 row-start-1 flex items-center gap-0.5 opacity-0 pointer-events-none transition-opacity duration-100 group-hover:opacity-100 group-hover:pointer-events-auto">
                          <Button
                            variant="ghost"
                            square
                            className="size-5 text-muted"
                            title={`Copy path: ${f.path}`}
                            onClick={(e) => {
                              e.stopPropagation();
                              writeText(f.path).catch((err) =>
                                showError(`Wouldn't copy the path: ${err}`),
                              );
                            }}
                          >
                            <Copy size={11} />
                          </Button>
                          {f.dirty && (
                            <Button
                              variant="ghost"
                              square
                              className="size-5 text-muted hover:text-danger"
                              title={`Discard changes: ${f.path}`}
                              onClick={(e) => {
                                e.stopPropagation();
                                setRevertPath(f.path);
                              }}
                            >
                              <Undo2 size={11} />
                            </Button>
                          )}
                        </span>
                      </>
                    )}
                  </span>
                </div>
              ))}
          </>
        ) : (
          wt && <FileTree />
        )}
      </div>
    </aside>
  );
}
