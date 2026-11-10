import { useEffect } from "react";
import { ChangesPane } from "./components/ChangesPane";
import { CommandPalette } from "./components/CommandPalette";
import { DeleteDialog } from "./components/DeleteDialog";
import { MainPane } from "./components/MainPane";
import { ProjectSettingsDialog } from "./components/ProjectSettingsDialog";
import { RemoveProjectDialog } from "./components/RemoveProjectDialog";
import { RenameDialog } from "./components/RenameDialog";
import { Sidebar } from "./components/Sidebar";
import { ToastHost } from "./components/ToastHost";
import { WindowControls } from "./components/WindowControls";
import { useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import { useStore } from "./store";

/**
 * Syncs store state to `document.body` classes that drive the collapsed-pane
 * and full-window-settings CSS overrides in `styles.css`. Those rules are
 * deliberately unlayered (outside any `@layer` block), same as `.app`'s own
 * `grid-template-columns` rule, so they win over it via ordinary cascade
 * specificity from a `body.*` ancestor selector — see the comment above them
 * in `styles.css`. Collapsing via width + overflow (not `display: none`)
 * works around a webkit2gtk repaint bug — see root `CLAUDE.md`.
 */
function usePaneCollapseSync(): void {
  const sidebarCollapsed = useStore((s) => s.settings.sidebar_collapsed);
  const changesCollapsed = useStore((s) => s.settings.changes_collapsed);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const needsCustomChrome = useStore((s) => s.needsCustomChrome);

  useEffect(() => {
    document.body.classList.toggle("sidebar-collapsed", sidebarCollapsed);
  }, [sidebarCollapsed]);

  useEffect(() => {
    document.body.classList.toggle("changes-collapsed", changesCollapsed);
  }, [changesCollapsed]);

  useEffect(() => {
    document.body.classList.toggle("settings-full", settingsOpen);
  }, [settingsOpen]);

  useEffect(() => {
    document.body.classList.toggle("has-window-controls", needsCustomChrome);
  }, [needsCustomChrome]);
}

/**
 * Blocks the bogus "already hovering" state webkit2gtk springs on whatever
 * element the OS cursor rests over the instant this window regains focus —
 * even via a purely keyboard/WM-driven focus change with no real mouse
 * movement (e.g. alt-tab, or a Hyprland keybind). Adds `body.suspend-hover`
 * (see `styles.css`, which turns off `pointer-events` — and with it,
 * `:hover` matching — site-wide) the instant focus is lost, and only lifts
 * it on a real `mousemove`, never merely on refocus. Without this, a
 * hover-only control (e.g. a sidebar row's remove button, which crossfades
 * with its status icon on hover) springs open on refocus and stays stuck
 * open indefinitely, since nothing else clears it until the mouse actually
 * moves again.
 */
function useSuspendHoverOnBlur(): void {
  useEffect(() => {
    const suspend = () => document.body.classList.add("suspend-hover");
    const resume = () => document.body.classList.remove("suspend-hover");
    window.addEventListener("blur", suspend);
    window.addEventListener("mousemove", resume);
    return () => {
      window.removeEventListener("blur", suspend);
      window.removeEventListener("mousemove", resume);
    };
  }, []);
}

export function App() {
  usePaneCollapseSync();
  useSuspendHoverOnBlur();
  useGlobalShortcuts();
  const sidebarWidth = useStore((s) => s.settings.sidebar_width);
  const changesWidth = useStore((s) => s.settings.changes_width);

  return (
    <div
      className="app grid h-screen"
      style={
        {
          "--sidebar-width": `${sidebarWidth}px`,
          "--changes-width": `${changesWidth}px`,
        } as React.CSSProperties
      }
    >
      <Sidebar />
      <MainPane />
      <ChangesPane />
      <RenameDialog />
      <DeleteDialog />
      <ProjectSettingsDialog />
      <RemoveProjectDialog />
      <CommandPalette />
      <ToastHost />
      <WindowControls />
    </div>
  );
}
