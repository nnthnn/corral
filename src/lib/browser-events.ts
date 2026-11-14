import { listen } from "@tauri-apps/api/event";
import { useStore } from "../store";
import { selectWorktree } from "./actions";
import { openFile } from "./files";

interface BrowserPreviewPayload {
  project: string;
  feature: string;
  /** Path relative to the worktree root. */
  path: string;
}

/** OpenCode's `browser.preview` tool asks the attached desktop browser host to
 * show a file. Corral has no Review pane, so its thin browser host
 * (`src-tauri/src/browser_host.rs`) emits this event and the existing File tab
 * is the closest equivalent. */
export async function initBrowserPreview(): Promise<void> {
  await listen<BrowserPreviewPayload>("browser-preview", (event) => {
    const { project, feature, path } = event.payload;
    const id = `${project}/${feature}`;
    const wt = useStore
      .getState()
      .worktrees.find(
        (worktree) =>
          worktree.project === project && worktree.feature === feature,
      );
    if (!wt) return;
    if (useStore.getState().selectedId !== id) selectWorktree(id);
    void openFile(wt, path);
  });
}
