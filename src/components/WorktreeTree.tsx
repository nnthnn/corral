import clsx from "clsx";
import { Plus, Settings } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import buckIcon from "../assets/buck/buck-icon.png";
import buckPointing from "../assets/buck/buck-pointing.png";
import { useCtrlHoldHint } from "../hooks/useCtrlHoldHint";
import { selectWorktree } from "../lib/actions";
import { showError } from "../lib/toasts";
import {
  createWorktree,
  openDeleteDialog,
  openProjectSettingsDialog,
  setProjectOrder,
  toggleGroup,
} from "../lib/worktrees";
import { useStore, visibleWorktreeIds, wantsAttention } from "../store";
import type { Worktree } from "../types";
import {
  matchesWorktreeFilter,
  moveProject,
  orderProjects,
  reorderProjects,
  wtId,
} from "../utils";
import { Button } from "./Button";
import { Highlight } from "./Highlight";
import { ProjectIcon } from "./ProjectIcon";
import { WorktreeRow } from "./WorktreeRow";

type DragOver = { project: string; side: "before" | "after" };

export function WorktreeTree() {
  const worktrees = useStore((s) => s.worktrees);
  const projects = useStore((s) => s.projects);
  const projectOrder = useStore((s) => s.settings.project_order);
  const filterText = useStore((s) => s.filterText);
  const attentionOnly = useStore((s) => s.attentionOnly);
  const collapsedGroups = useStore((s) => s.collapsedGroups);
  const selectedId = useStore((s) => s.selectedId);
  const worktreeNudgeTarget = useStore((s) => s.worktreeNudgeTarget);
  const loadError = useStore((s) => s.loadError);
  // Not read directly — subscribing so a status flip re-renders this list,
  // since it can add/remove a row from the "waiting only" attention filter.
  useStore((s) => s.sessions);
  const [draggingProject, setDraggingProject] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState<DragOver | null>(null);
  // Reveals each row's `Ctrl+N` number while Ctrl is held — see
  // `useCtrlHoldHint`.
  const shortcutHints = useCtrlHoldHint();

  useEffect(() => {
    if (!worktreeNudgeTarget) return;
    const el = document.querySelector(
      `[data-project-add="${CSS.escape(worktreeNudgeTarget)}"]`,
    );
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
    const t = setTimeout(() => {
      useStore.setState({ worktreeNudgeTarget: null });
    }, 1000);
    return () => clearTimeout(t);
  }, [worktreeNudgeTarget]);

  if (loadError) {
    return (
      <div className="tree-error text-center p-3 text-body">
        <img
          src={buckIcon}
          alt=""
          className="w-14 h-14 object-contain mx-auto mb-1"
        />
        <p className="text-danger m-0">{loadError}</p>
      </div>
    );
  }

  if (projects.length === 0 && worktrees.length === 0) {
    return (
      <div className="tree-empty text-muted p-3 text-center">
        <img
          src={buckPointing}
          alt=""
          className="w-20 h-20 object-contain mx-auto mb-1"
        />
        <p className="mb-1 text-body">Nothing's running. Start something.</p>
        <p className="tree-empty-hint text-caption opacity-75">
          Use "+" above to get started.
        </p>
      </div>
    );
  }

  const isRowVisible = (wt: Worktree) =>
    matchesWorktreeFilter(wt, filterText) &&
    (!attentionOnly || wantsAttention(useStore.getState(), wtId(wt)));

  const orderedProjects = orderProjects(projects, projectOrder);
  const groups = new Map<string, Worktree[]>();
  // Every known project gets a group up front — including ones with no
  // active worktrees — so it stays visible until its repo checkout is
  // actually gone, not just whenever its last worktree is cleaned up.
  for (const project of orderedProjects) groups.set(project, []);
  for (const wt of worktrees) {
    const list = groups.get(wt.project) ?? [];
    list.push(wt);
    groups.set(wt.project, list);
  }

  const narrowed = Boolean(filterText) || attentionOnly;
  // Reordering a partial (filtered) view would drop a project relative to
  // ones currently hidden, in a way that's hard to predict — only allow it
  // over the full, unfiltered list.
  const canDrag = !narrowed;
  // Only the first nine rows have shortcuts. Numbering comes from the same
  // ordered/filtered list the `Ctrl+1–9` handler uses, so a number always
  // matches the row the key would jump to (collapsed groups aside — their
  // rows aren't rendered, leaving harmless gaps).
  const shortcutNumbers = new Map<string, number>();
  if (shortcutHints) {
    for (const [index, id] of visibleWorktreeIds(useStore.getState())
      .slice(0, 9)
      .entries()) {
      shortcutNumbers.set(id, index + 1);
    }
  }
  let matched = 0;
  const groupNodes: ReactNode[] = [];

  for (const [project, items] of groups) {
    const visibleItems = items.filter(isRowVisible);
    matched += visibleItems.length;
    if (narrowed && visibleItems.length === 0) continue;
    const collapsed = !narrowed && collapsedGroups.has(project);
    const rows: ReactNode[] = [];
    for (const wt of items) {
      if (!isRowVisible(wt)) continue;
      rows.push(
        <WorktreeRow
          key={wtId(wt)}
          wt={wt}
          filterText={filterText}
          active={wtId(wt) === selectedId}
          onSelect={selectWorktree}
          onRemove={openDeleteDialog}
          shortcutNumber={shortcutNumbers.get(wtId(wt))}
        />,
      );
    }

    groupNodes.push(
      <div
        key={project}
        className={clsx("wt-group mb-4", collapsed && "collapsed")}
      >
        {/* biome-ignore lint/a11y/noStaticElementInteractions: drag-reorder handlers only; the actual click/keyboard collapse target is the role="button" div nested below */}
        <div
          className={clsx(
            "wt-group-head group flex items-center gap-2 py-2 px-2 min-h-12 cursor-pointer rounded-md select-none hover:bg-surface-2",
            draggingProject === project && "dragging",
            dragOver?.project === project && `drag-over-${dragOver.side}`,
          )}
          draggable={canDrag}
          onDragStart={() => setDraggingProject(project)}
          onDragEnd={() => {
            setDraggingProject(null);
            setDragOver(null);
          }}
          onDragOver={(e) => {
            if (!draggingProject || draggingProject === project) return;
            e.preventDefault();
            const rect = e.currentTarget.getBoundingClientRect();
            const side =
              e.clientY < rect.top + rect.height / 2 ? "before" : "after";
            setDragOver({ project, side });
          }}
          onDragLeave={() =>
            setDragOver((cur) => (cur?.project === project ? null : cur))
          }
          onDrop={(e) => {
            e.preventDefault();
            if (!draggingProject) return;
            const side =
              dragOver?.project === project ? dragOver.side : "before";
            setProjectOrder(
              reorderProjects(orderedProjects, draggingProject, project, side),
            );
            setDraggingProject(null);
            setDragOver(null);
          }}
        >
          {/* A real <button> can't validly nest the "+"/gear <button>s below,
           * so the collapse target is a div — same role="button" + tabIndex
           * pattern as WorktreeRow's row-as-button. A `display: contents`
           * <button> would avoid that nesting problem too, but WebKit drops
           * `display: contents` interactive elements from the tab order
           * entirely, making the whole row keyboard-unreachable. */}
          {/* biome-ignore lint/a11y/useSemanticElements: nests the gear/+ <button>s below, invalid inside a <button> */}
          <div
            role="button"
            tabIndex={0}
            className="flex-1 flex items-center gap-2 min-w-0 cursor-pointer"
            onClick={() => toggleGroup(project)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                toggleGroup(project);
                return;
              }
              if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                e.preventDefault();
                setProjectOrder(
                  moveProject(
                    orderedProjects,
                    project,
                    e.key === "ArrowUp" ? -1 : 1,
                  ),
                );
              }
            }}
          >
            <ProjectIcon project={project} collapsed={collapsed} />
            <span className="flex-1 flex items-center gap-1.5 min-w-0">
              <span className="overflow-hidden text-ellipsis whitespace-nowrap text-body font-semibold text-text">
                <Highlight text={project} needle={filterText} />
              </span>
              {collapsed && items.length > 0 && (
                <span className="font-mono text-label text-muted bg-surface-2 rounded-sm px-1.5 py-1 leading-none">
                  {items.length}
                </span>
              )}
            </span>
          </div>
          <div className="flex items-center gap-1 flex-none">
            <Button
              variant="ghost"
              square
              className="opacity-0 pointer-events-none group-hover:opacity-100 group-hover:pointer-events-auto transition-opacity duration-100 text-muted hover:text-text"
              title={`Provisioning settings for ${project}`}
              onClick={() => openProjectSettingsDialog(project)}
            >
              <Settings size={14} />
            </Button>
            <Button
              variant="ghost"
              square
              className={clsx(
                "text-muted hover:text-text",
                worktreeNudgeTarget === project && "project-add-nudge",
              )}
              title={`New feature in ${project}`}
              data-project-add={project}
              onClick={() =>
                createWorktree(project).catch((err) =>
                  showError(
                    `Couldn't build the pen. Check the path and try again. (${err})`,
                  ),
                )
              }
            >
              <Plus size={14} />
            </Button>
          </div>
        </div>
        {rows}
      </div>,
    );
  }

  if (narrowed && matched === 0) {
    return (
      <div className="tree-empty text-muted p-3 text-body">
        <p>
          {filterText
            ? `No worktrees match "${filterText}".`
            : "Nothing's waiting on you right now."}
        </p>
      </div>
    );
  }

  return <>{groupNodes}</>;
}
