import clsx from "clsx";
import { FolderPlus, RefreshCw, Search, Settings } from "lucide-react";
import {
  type KeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { selectWorktree } from "../lib/actions";
import { loadAll } from "../lib/data";
import { addProject } from "../lib/projects";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { orderProjects, worktreeTitle, wtId } from "../utils";
import { Highlight } from "./Highlight";

type ActionId = "settings" | "refresh" | "add-project";

interface Action {
  kind: "action";
  id: ActionId;
  label: string;
  Icon: typeof Settings;
}

type Item = { kind: "worktree"; wt: Worktree } | Action;

/** Global actions the palette offers after the worktree list — kept to ones
 * with no required arguments, so a single Enter is all any of them needs. */
const ACTIONS: Action[] = [
  { kind: "action", id: "settings", label: "Settings", Icon: Settings },
  { kind: "action", id: "refresh", label: "Refresh", Icon: RefreshCw },
  { kind: "action", id: "add-project", label: "Add project", Icon: FolderPlus },
];

function close(): void {
  useStore.setState({ paletteOpen: false });
}

function runAction(id: ActionId): void {
  if (id === "settings") useStore.setState({ settingsOpen: true });
  else if (id === "refresh") loadAll();
  else if (id === "add-project") void addProject();
}

/** Match the same fields the sidebar filter does, so palette behavior doesn't
 * surprise anyone who already filters the sidebar the same way. */
function matchesWorktree(wt: Worktree, q: string): boolean {
  if (!q) return true;
  return (
    wt.project.toLowerCase().includes(q) ||
    worktreeTitle(wt).toLowerCase().includes(q) ||
    wt.feature.toLowerCase().includes(q) ||
    wt.branch.toLowerCase().includes(q)
  );
}

/** Type-ahead command palette (Ctrl/Cmd+K): jump to any worktree or fire a
 * small set of global actions. Opened/closed via `paletteOpen` in the store,
 * so the global shortcut hook and any button can drive it without owning its
 * query/highlight state (which resets on each open). */
export function CommandPalette() {
  const open = useStore((s) => s.paletteOpen);
  const worktrees = useStore((s) => s.worktrees);
  const projects = useStore((s) => s.projects);
  const projectOrder = useStore((s) => s.settings.project_order);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  const q = query.trim().toLowerCase();

  const items = useMemo<Item[]>(() => {
    // Flatten worktrees into the same order the sidebar renders (custom
    // project order, then the worktrees array order within each project).
    const ordered = orderProjects(projects, projectOrder);
    const flat: Worktree[] = [];
    for (const project of ordered) {
      for (const wt of worktrees) {
        if (wt.project === project) flat.push(wt);
      }
    }
    // Defensive: a worktree whose project isn't in `projects` (shouldn't
    // happen once loaded, but don't drop it if it ever does).
    for (const wt of worktrees) {
      if (!flat.includes(wt)) flat.push(wt);
    }
    const matchedWorktrees = flat
      .filter((wt) => matchesWorktree(wt, q))
      .map((wt) => ({ kind: "worktree", wt }) as const);
    const matchedActions = ACTIONS.filter((action) =>
      action.label.toLowerCase().includes(q),
    );
    return [...matchedWorktrees, ...matchedActions];
  }, [worktrees, projects, projectOrder, q]);

  // Reset the query/highlight and focus the input each time the palette opens.
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveIndex(0);
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  // Keep the highlighted row in view while arrowing through the list. Only the
  // item rows carry a `data-active` attribute (the "No matches." placeholder
  // doesn't), so `rows[activeIndex]` maps 1:1 onto `items`.
  useEffect(() => {
    if (!open) return;
    const rows = listRef.current?.querySelectorAll("[data-active]");
    rows?.[activeIndex]?.scrollIntoView({ block: "nearest" });
  }, [open, activeIndex]);

  if (!open) return null;

  function select(item: Item): void {
    if (item.kind === "worktree") selectWorktree(wtId(item.wt));
    else runAction(item.id);
    close();
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>): void {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, items.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = items[Math.min(activeIndex, items.length - 1)];
      if (item) select(item);
    } else if (e.key === "Escape") {
      e.preventDefault();
      // Stop the global Escape handler (settings/tab close) from also firing.
      e.stopPropagation();
      close();
    }
  }

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop is a click-outside-to-close target, not a focusable control — the dialog inside it carries the real role
    <div
      className="fixed inset-0 z-50 flex justify-center bg-black/40 pt-24"
      onMouseDown={close}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="flex max-h-[70vh] w-[min(560px,90vw)] flex-col overflow-hidden rounded-md border border-border bg-surface text-text shadow-lg"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {/* `py-1.5` (with a matching reduced input padding) leaves room for
            the global 4px focus ring — without it the ring is drawn flush
            against the dialog's top edge and `overflow-hidden` clips its top. */}
        <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
          <Search size={16} className="shrink-0 text-muted" />
          <input
            ref={inputRef}
            type="text"
            placeholder="Jump to a worktree…"
            autoComplete="off"
            spellCheck={false}
            className="w-full bg-transparent py-1 text-body text-text placeholder:text-muted focus:outline-none"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              // Typing reshuffles the list — drop back to the top so the
              // highlight never points past the newly-filtered items.
              setActiveIndex(0);
            }}
            onKeyDown={onKeyDown}
          />
        </div>
        <ul ref={listRef} className="min-h-0 flex-1 overflow-y-auto p-1.5">
          {items.length === 0 && (
            <li className="px-2 py-2 text-sub text-muted">No matches.</li>
          )}
          {items.map((item, i) => {
            const isActive = i === activeIndex;
            if (item.kind === "worktree") {
              const wt = item.wt;
              return (
                <li key={wtId(wt)} data-active={isActive}>
                  <button
                    type="button"
                    className={clsx(
                      "flex w-full cursor-pointer items-baseline gap-2 rounded-sm px-2 py-2 text-left",
                      isActive ? "bg-surface-2" : "bg-transparent",
                    )}
                    onMouseEnter={() => setActiveIndex(i)}
                    onClick={() => select(item)}
                  >
                    <span className="shrink-0 text-label text-muted">
                      <Highlight text={wt.project} needle={q} />
                    </span>
                    <span className="min-w-0 truncate text-body font-semibold">
                      <Highlight text={worktreeTitle(wt)} needle={q} />
                    </span>
                  </button>
                </li>
              );
            }
            return (
              <li key={item.id} data-active={isActive}>
                <button
                  type="button"
                  className={clsx(
                    "flex w-full cursor-pointer items-center gap-2 rounded-sm px-2 py-2 text-left",
                    isActive ? "bg-surface-2" : "bg-transparent",
                  )}
                  onMouseEnter={() => setActiveIndex(i)}
                  onClick={() => select(item)}
                >
                  <item.Icon size={14} className="shrink-0 text-muted" />
                  <span className="text-body">
                    <Highlight text={item.label} needle={q} />
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
        <div className="flex items-center gap-3 border-t border-border px-3 py-1.5 text-label text-muted">
          <span>↑↓ navigate</span>
          <span>↵ open</span>
          <span>esc close</span>
        </div>
      </div>
    </div>
  );
}
