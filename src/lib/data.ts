import type { SessionInfo } from "../session";
import { useStore } from "../store";
import { wtId } from "../utils";
import {
  clearSelection,
  selectWorktree,
  syncSelectedWorktree,
} from "./actions";
import { invoke } from "./tauri";
import { showError } from "./toasts";

export async function loadAll(): Promise<void> {
  try {
    const [worktrees, sessList, projects] = await Promise.all([
      invoke("list_worktrees"),
      invoke("list_sessions"),
      invoke("list_projects"),
    ]);
    const sessions = new Map<string, SessionInfo>();
    for (const s of sessList) sessions.set(s.id, s);
    useStore.setState({ worktrees, sessions, projects, loadError: null });
    // Fire-and-forget: icon lookups resolve asynchronously and "pop in" once
    // ready; `loadAll` doesn't wait on them so unrelated callers (create/
    // rename/delete flows) aren't delayed by icon-fetch latency. Driven by
    // the full `projects` list — not `worktrees` — so a project with no
    // active worktrees still gets its icon fetched instead of the fallback
    // glyph until its first worktree appears.
    void ensureProjectIcons(projects);

    const { selectedId } = useStore.getState();
    if (selectedId && worktrees.some((w) => wtId(w) === selectedId)) {
      // Already selected — just refresh its data, don't reset activeTab/
      // diffTab (that reset is for genuine user-initiated selection
      // changes, see `selectWorktree`).
      syncSelectedWorktree(selectedId);
    } else if (worktrees.length) {
      selectWorktree(wtId(worktrees[0]));
    } else {
      clearSelection();
    }
  } catch (err) {
    useStore.setState({
      loadError: `Couldn't round up your worktrees: ${err}`,
    });
  }
}

async function ensureProjectIcons(projects: string[]): Promise<void> {
  const known = useStore.getState().projectIcons;
  const missing = [...new Set(projects)].filter((p) => !known.has(p));
  if (!missing.length) return;

  const results = await Promise.all(
    missing.map(async (project) => {
      try {
        const icon = await invoke("project_icon", { project });
        return [project, icon] as const;
      } catch (err) {
        showError(`Icon wouldn't load for ${project}: ${err}`);
        return [project, null] as const;
      }
    }),
  );

  const projectIcons = new Map(useStore.getState().projectIcons);
  for (const [project, icon] of results) projectIcons.set(project, icon);
  useStore.setState({ projectIcons });
}
