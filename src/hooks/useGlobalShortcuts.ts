import { useEffect } from "react";
import { closeTab, selectWorktree } from "../lib/actions";
import { toggleSetting } from "../lib/settings";
import { useStore, visibleWorktreeIds, worktreeTabIds } from "../store";

const MIN_DIFF_FONT_SIZE = 8;
const MAX_DIFF_FONT_SIZE = 32;
const DEFAULT_DIFF_FONT_SIZE = 16;

function zoomDiffFont(delta: number): void {
  const s = useStore.getState();
  const clamped = Math.min(
    MAX_DIFF_FONT_SIZE,
    Math.max(MIN_DIFF_FONT_SIZE, s.settings.diff_font_size + delta),
  );
  if (clamped === s.settings.diff_font_size) return;
  toggleSetting("diff_font_size", clamped);
}

/**
 * Ctrl/⌘ + 1–9 jumps to the Nth worktree in the sidebar; Ctrl/⌘ + [ / ]
 * steps to the prev/next worktree (wrapping around the ends). Ctrl/⌘ + Tab /
 * Shift+Tab cycles the center-pane tabs (Agent/Run/Setup/PR/Diff/File) within the
 * selected worktree, wrapping (but not while Settings or the command palette
 * is open). Ctrl/⌘ +/-/0 zooms the active terminal's
 * font. Ctrl/⌘ + Shift + R force-repaints the active terminal (fixes rare
 * WebGL rendering glitches). Ctrl/⌘ + K toggles the command palette; Ctrl/⌘
 * + , opens Settings. All fire on `keydown` in the capture phase, so they
 * beat the terminal (which only sees `keypress`-equivalent data). While the
 * diff tab is active, +/-/0 zoom the diff viewer's font instead of the
 * terminal's.
 */
function handleShortcut(e: KeyboardEvent): void {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
  const s = useStore.getState();

  if (e.shiftKey && e.key.toLowerCase() === "r") {
    e.preventDefault();
    s.sessionMgr?.repaintActive();
    return;
  }

  if (!e.shiftKey && e.key >= "1" && e.key <= "9") {
    const idx = Number(e.key) - 1;
    const ids = visibleWorktreeIds(s);
    if (idx >= ids.length) return;
    e.preventDefault();
    selectWorktree(ids[idx]);
    return;
  }

  if (e.key === "[" || e.key === "]") {
    const ids = visibleWorktreeIds(s);
    if (!ids.length) return;
    e.preventDefault();
    const curIdx = ids.indexOf(s.selectedId ?? "");
    const delta = e.key === "[" ? -1 : 1;
    const nextIdx =
      curIdx === -1 ? 0 : (curIdx + delta + ids.length) % ids.length;
    selectWorktree(ids[nextIdx]);
    return;
  }

  if (e.key === "Tab") {
    // Don't cycle the (hidden) center-pane tabs while a full-window overlay
    // is up — Settings and the command palette both own the screen.
    if (s.settingsOpen || s.paletteOpen) return;
    const ids = worktreeTabIds(s);
    if (ids.length === 0) return;
    e.preventDefault();
    const curIdx = ids.indexOf(s.activeTab);
    const delta = e.shiftKey ? -1 : 1;
    const nextIdx =
      curIdx === -1 ? 0 : (curIdx + delta + ids.length) % ids.length;
    useStore.setState({ activeTab: ids[nextIdx] });
    return;
  }

  // "=" is the unshifted key for "+" on US layouts; accept both.
  if (e.key === "+" || e.key === "=") {
    e.preventDefault();
    if (s.activeTab === "diff") zoomDiffFont(1);
    else s.sessionMgr?.increaseFontSize();
  } else if (e.key === "-" || e.key === "_") {
    e.preventDefault();
    if (s.activeTab === "diff") zoomDiffFont(-1);
    else s.sessionMgr?.decreaseFontSize();
  } else if (e.key === "0") {
    e.preventDefault();
    if (s.activeTab === "diff")
      toggleSetting("diff_font_size", DEFAULT_DIFF_FONT_SIZE);
    else s.sessionMgr?.resetFontSize();
  } else if (e.key === ",") {
    e.preventDefault();
    useStore.setState({ settingsOpen: true });
  } else if (e.key.toLowerCase() === "k") {
    e.preventDefault();
    useStore.setState({ paletteOpen: !useStore.getState().paletteOpen });
  }
}

/** Escape closes Settings if it's open, else closes the active non-Agent tab. */
function handleEscape(e: KeyboardEvent): void {
  if (e.key !== "Escape") return;
  const s = useStore.getState();
  // Native <dialog> modals close themselves via their own `cancel` handler;
  // they don't stop propagation, so bail here or Escape would also tear down
  // the tab open behind the dialog.
  const modalOpen =
    s.deleteTarget ||
    s.renameTarget ||
    s.projectSettingsTarget ||
    s.removeProjectTarget;
  if (s.paletteOpen) useStore.setState({ paletteOpen: false });
  else if (s.settingsOpen) useStore.setState({ settingsOpen: false });
  else if (modalOpen) return;
  else if (s.activeTab !== "agent") closeTab(s.activeTab);
}

/** Registered once at the app root — see `App`. */
export function useGlobalShortcuts(): void {
  useEffect(() => {
    window.addEventListener("keydown", handleShortcut, true);
    document.addEventListener("keydown", handleEscape);
    return () => {
      window.removeEventListener("keydown", handleShortcut, true);
      document.removeEventListener("keydown", handleEscape);
    };
  }, []);
}
