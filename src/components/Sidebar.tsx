import { getVersion } from "@tauri-apps/api/app";
import { FolderPlus, RefreshCw, Search, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import { loadAll } from "../lib/data";
import { addProject } from "../lib/projects";
import { toggleSetting } from "../lib/settings";
import { invoke } from "../lib/tauri";
import { runningSessionCount, useStore } from "../store";
import { formatBytes } from "../utils";
import { Button } from "./Button";
import { ResizeHandle } from "./ResizeHandle";
import { WorktreeTree } from "./WorktreeTree";

export function toggleSidebar(): void {
  toggleSetting(
    "sidebar_collapsed",
    !useStore.getState().settings.sidebar_collapsed,
  );
}

export function Sidebar() {
  const attentionOnly = useStore((s) => s.attentionOnly);
  const sidebarCollapsed = useStore((s) => s.settings.sidebar_collapsed);
  const runningCount = useStore(runningSessionCount);
  // Kept local (not bound straight to `store.filterText`) so the visible box
  // preserves whatever case/whitespace you typed — only the stored filter
  // used for matching is normalized (trim + lowercase).
  const [inputValue, setInputValue] = useState("");
  const [version, setVersion] = useState("");
  const [memoryUsage, setMemoryUsage] = useState<number | null>(null);
  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  // Total RSS of corral + every agent/PTY child, polled so "is this eating my
  // RAM" stays answerable without a manual check. `null` (non-Linux) hides it.
  useEffect(() => {
    let cancelled = false;
    async function poll() {
      const bytes = await invoke("memory_usage").catch(() => null);
      if (!cancelled && typeof bytes === "number") setMemoryUsage(bytes);
    }
    void poll();
    const interval = window.setInterval(poll, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, []);

  return (
    <aside className="sidebar relative overflow-hidden flex flex-col border-r border-border bg-surface min-h-0">
      <div
        className="sidebar-projects flex items-center justify-between py-2 px-2 border-b border-border"
        data-tauri-drag-region
      >
        <span className="text-label uppercase tracking-[0.6px] text-muted pl-1">
          Projects
        </span>
        <div className="flex gap-1">
          <Button
            variant="ghost"
            icon
            className="size-7 p-0 flex items-center justify-center"
            title="Command palette (Ctrl/Cmd K)"
            onClick={() => useStore.setState({ paletteOpen: true })}
          >
            <Search size={14} />
          </Button>
          <Button
            variant="ghost"
            icon
            active={attentionOnly}
            className="size-7 p-0 flex items-center justify-center"
            title="Show only worktrees waiting at the gate"
            onClick={() => useStore.setState({ attentionOnly: !attentionOnly })}
          >
            <span className="grayscale">🔔</span>
          </Button>
          <Button
            variant="ghost"
            icon
            className="size-7 p-0 flex items-center justify-center"
            title="Refresh"
            onClick={() => loadAll()}
          >
            <RefreshCw size={14} />
          </Button>
          <Button
            variant="ghost"
            icon
            className="size-7 p-0 flex items-center justify-center"
            title="Add project"
            onClick={() => void addProject()}
          >
            <FolderPlus size={14} />
          </Button>
        </div>
      </div>
      <div className="sidebar-filter pt-2 px-2">
        <input
          type="text"
          placeholder="Filter worktrees…"
          autoComplete="off"
          className="w-full py-1.5 px-3 rounded-md border border-border bg-surface-2 text-text text-body focus:outline-none focus:border-primary"
          value={inputValue}
          onChange={(e) => {
            setInputValue(e.target.value);
            useStore.setState({
              filterText: e.target.value.trim().toLowerCase(),
            });
          }}
          onKeyDown={(e) => {
            if (e.key !== "Escape") return;
            e.stopPropagation(); // don't let it bubble to the settings/tab Escape handler
            setInputValue("");
            useStore.setState({ filterText: "" });
            e.currentTarget.blur();
          }}
        />
      </div>
      <div className="tree flex-1 overflow-y-auto p-2 min-h-0">
        <WorktreeTree />
      </div>
      <footer className="sidebar-foot py-2 px-3 border-t border-border flex items-center justify-end gap-2">
        {version && (
          <span className="mr-auto font-mono text-label text-muted opacity-60">
            v{version}
          </span>
        )}
        {runningCount > 0 && (
          <span className="text-label text-muted">{runningCount} running</span>
        )}
        {memoryUsage !== null && (
          <span
            className="font-mono text-label text-muted opacity-70"
            title="Total memory: corral + all agent processes"
          >
            {formatBytes(memoryUsage)}
          </span>
        )}
        <Button
          variant="ghost"
          square
          title="Settings (Ctrl/Cmd ,)"
          onClick={() => useStore.setState({ settingsOpen: true })}
        >
          <Settings size={16} />
        </Button>
      </footer>
      {!sidebarCollapsed && (
        <ResizeHandle
          settingsKey="sidebar_width"
          min={200}
          max={500}
          defaultWidth={320}
          side="right"
        />
      )}
    </aside>
  );
}
