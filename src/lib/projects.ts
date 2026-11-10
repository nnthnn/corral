import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";
import { loadAll } from "./data";
import { invoke } from "./tauri";
import { showError } from "./toasts";
import { createWorktree } from "./worktrees";

/** Opens a native folder picker and registers the chosen directory as a new
 * project (must be a git repo), then immediately creates its first worktree
 * and starts the agent — backs the sidebar's "Add project" button. */
export async function addProject(): Promise<void> {
  const selected = await open({ directory: true, multiple: false });
  if (!selected || Array.isArray(selected)) return;
  try {
    const name = await invoke("add_project", { path: selected });
    await createWorktree(name).catch(async (err) => {
      showError(`Project added, but couldn't build the first pen. (${err})`);
      // createWorktree's own loadAll() never ran since it failed before
      // reaching that point — without this, the now-registered project
      // stays invisible in the sidebar until some unrelated reload.
      await loadAll();
    });
  } catch (err) {
    showError(`Couldn't add project: ${err}`);
  }
}

export function openRemoveProjectDialog(project: string): void {
  useStore.setState({ removeProjectTarget: project });
}
