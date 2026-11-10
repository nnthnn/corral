import { useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";
import { invoke } from "./tauri";
import { showError } from "./toasts";

/** Cache key for `AppState.fileTree`, shared by `loadDir`/`openFile` here
 * and `FileTree.tsx`'s lookups — exported so both stay in sync instead of
 * each hand-rolling the same string format. */
export function dirKey(wt: Worktree, path: string): string {
  return `${wtId(wt)}:${path}`;
}

/** Fetches and caches `path`'s directory listing (keyed by worktree + path
 * in `fileTree`) unless it's already cached — `FileTree`'s expand-on-click
 * relies on this no-op-when-cached behavior to avoid refetching on every
 * collapse/re-expand. */
export async function loadDir(wt: Worktree, path: string): Promise<void> {
  const key = dirKey(wt, path);
  if (useStore.getState().fileTree.has(key)) return;
  try {
    const entries = await invoke("list_worktree_dir", {
      project: wt.project,
      feature: wt.feature,
      path,
    });
    const fileTree = new Map(useStore.getState().fileTree);
    fileTree.set(key, entries);
    useStore.setState({ fileTree });
  } catch (err) {
    showError(`Couldn't list ${path || "the worktree root"}: ${err}`);
  }
}

/** `data:` URL for a raster image returned as `FileContent.image` — the
 * `mime`/`data` split from the backend keeps the encoding decision (base64)
 * on the Rust side, so this is just the URL wrapper `<img src>` needs. */
export function imageDataUrl(mime: string, base64: string): string {
  return `data:${mime};base64,${base64}`;
}

/** `data:` URL for an SVG's *source text*. Unlike raster images SVG isn't
 * returned pre-encoded (it comes back as plain `text` so the preview can
 * also show it as source), so encode it here — `encodeURIComponent` rather
 * than base64 because it survives non-Latin1 characters that `btoa` chokes
 * on. In an `<img>` the SVG is isolated from the page: its scripts don't
 * run and it can't reach the app's DOM, so inline/interactive SVG can't
 * target the shell. */
export function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** True for `.svg` paths. SVG loads as `text` (it's a text format), so the
 * preview has to recognize it by path to offer the rendered/source toggle.
 * Mirrors the backend's raster-only `image_mime` split in
 * `crates/corral-core/src/files.rs`. */
export function isSvgPath(path: string): boolean {
  return /\.svg$/i.test(path);
}

/** True for markdown paths that get a rendered view. `.mdx` is deliberately
 * excluded: it can contain JSX/expressions that a plain GFM renderer would
 * mangle, so those stay in the highlighted source view. */
export function isMarkdownPath(path: string): boolean {
  return /\.(md|markdown)$/i.test(path);
}

/** Opens (or refocuses) the "File" tab for `path` and loads its content —
 * mirrors `openDiff` in `lib/diff.ts`, including the stale-response guard
 * for rapid successive clicks. */
export async function openFile(wt: Worktree, path: string): Promise<void> {
  const id = wtId(wt);
  useStore.setState({
    fileTab: { path },
    activeTab: "file",
    fileContent: null,
    fileError: null,
  });
  // Compare the worktree too — see `openDiff`'s equivalent guard: the same
  // relative path can exist in two worktrees, so a path-only check can let a
  // stale response from the previous worktree land.
  const isStillActive = () => {
    const s = useStore.getState();
    return s.selectedId === id && s.fileTab?.path === path;
  };
  try {
    const content = await invoke("read_worktree_file", {
      project: wt.project,
      feature: wt.feature,
      path,
    });
    if (isStillActive()) useStore.setState({ fileContent: content });
  } catch (err) {
    if (isStillActive()) useStore.setState({ fileError: String(err) });
  }
}
