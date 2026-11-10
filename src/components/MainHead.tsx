import {
  Code2,
  Loader2,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Pencil,
  Play,
  Plus,
  RotateCw,
  Square,
  StepForward,
  Terminal,
  Trash2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { usePrChecksPolling } from "../hooks/usePrChecksPolling";
import {
  agentButtonLabel,
  openTerminal,
  openZed,
  startAgent,
  stopAgent,
} from "../lib/agent-actions";
import { toggleSetting } from "../lib/settings";
import { openDeleteDialog, openRenameDialog } from "../lib/worktrees";
import { currentWt, useStore } from "../store";
import { worktreeTitle, wtId } from "../utils";
import { Button } from "./Button";
import { toggleSidebar } from "./Sidebar";

const HEADER_CLASS =
  "main-head flex items-center gap-3 py-2 px-4 border-b border-border bg-surface h-12";

function toggleChanges(): void {
  toggleSetting(
    "changes_collapsed",
    !useStore.getState().settings.changes_collapsed,
  );
}

/** Lives permanently in the main column's header — unlike the changes-pane
 * toggle, the sidebar has no chrome of its own once collapsed, so this is
 * the only way to reopen it (and stays put rather than moving to wherever
 * the sidebar's edge happens to be). */
function SidebarToggle() {
  const sidebarCollapsed = useStore((s) => s.settings.sidebar_collapsed);
  return (
    <Button
      variant="ghost"
      square
      title="Toggle sidebar"
      onClick={toggleSidebar}
    >
      {sidebarCollapsed ? (
        <PanelLeftOpen size={16} />
      ) : (
        <PanelLeftClose size={16} />
      )}
    </Button>
  );
}

function ChangesToggle() {
  const changesCollapsed = useStore((s) => s.settings.changes_collapsed);
  return (
    <Button
      variant="ghost"
      square
      title="Toggle changes pane"
      onClick={toggleChanges}
    >
      {changesCollapsed ? (
        <PanelRightOpen size={16} />
      ) : (
        <PanelRightClose size={16} />
      )}
    </Button>
  );
}

export function MainHead() {
  const wt = useStore(currentWt);
  const sessions = useStore((s) => s.sessions);
  const id = wt ? wtId(wt) : null;

  if (!wt || !id) {
    return (
      <header className={HEADER_CLASS} data-tauri-drag-region>
        <SidebarToggle />
        <div className="ml-auto">
          <ChangesToggle />
        </div>
      </header>
    );
  }

  const running = sessions.get(id)?.running ?? false;

  return <MainHeadActions wt={wt} id={id} running={running} />;
}

function MainHeadActions({
  wt,
  id,
  running,
}: {
  wt: NonNullable<ReturnType<typeof currentWt>>;
  id: string;
  running: boolean;
}) {
  // Stopping the agent removes the Stop button, and the Start/Resume button
  // takes its place at the same spot — suppress it briefly so a click that
  // lands right after Stop can't instantly restart it. This used to clear
  // only on `onMouseLeave`, which left the button permanently invisible
  // whenever that event didn't fire (e.g. the pointer never actually left
  // the button's bounds after the click) — the only way out was switching
  // worktrees and back. A short timer clears it deterministically instead,
  // regardless of pointer movement; `onMouseLeave` still clears it early for
  // the common case of the pointer actually moving away.
  const wasRunningRef = useRef(running);
  const [suppressStart, setSuppressStart] = useState(false);
  useEffect(() => {
    if (wasRunningRef.current && !running) {
      setSuppressStart(true);
      const timer = setTimeout(() => setSuppressStart(false), 400);
      wasRunningRef.current = running;
      return () => clearTimeout(timer);
    }
    wasRunningRef.current = running;
  }, [running]);
  // `id` isn't read in the body — it's a reset trigger for switching worktrees.
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentional reset-on-key-change
  useEffect(() => setSuppressStart(false), [id]);
  const prInfo = useStore((s) => s.prInfoByWtId.get(id)) ?? null;
  usePrChecksPolling(wt, prInfo?.state === "open");
  const starting = useStore((s) => s.startingIds.has(id));

  return (
    <header className={HEADER_CLASS} data-tauri-drag-region>
      <SidebarToggle />
      <div className="crumb flex items-baseline gap-1.5 min-w-0 overflow-hidden">
        <span className="crumb-project text-muted shrink-0">{wt.project}</span>
        <span className="crumb-sep text-muted shrink-0">/</span>
        <span className="crumb-feature min-w-0 font-semibold whitespace-nowrap overflow-hidden text-ellipsis">
          {worktreeTitle(wt)}
        </span>
      </div>
      <div className="head-actions ml-auto flex gap-1.5 items-center shrink-0">
        {running ? (
          <Button
            variant="ghost"
            icon
            title="Stop agent"
            onClick={() => stopAgent(id)}
          >
            <Square size={16} />
          </Button>
        ) : suppressStart ? (
          <Button
            disabled
            icon
            className="opacity-0"
            onMouseLeave={() => setSuppressStart(false)}
          >
            {wt.has_resumable_session ? (
              <StepForward size={16} />
            ) : (
              <Play size={16} />
            )}
          </Button>
        ) : starting ? (
          <Button disabled icon title="Starting…">
            <Loader2 size={16} className="animate-spin" />
          </Button>
        ) : (
          <>
            <Button
              icon
              title={agentButtonLabel(wt)}
              onClick={() => startAgent(wt)}
            >
              {wt.has_resumable_session ? (
                <StepForward size={16} />
              ) : (
                <Play size={16} />
              )}
            </Button>
            <Button
              variant="ghost"
              icon
              title="New session"
              onClick={() => startAgent(wt, true)}
            >
              <Plus size={16} />
            </Button>
          </>
        )}
        <Button
          variant="ghost"
          icon
          title="Rename"
          onClick={() => openRenameDialog(wt)}
        >
          <Pencil size={16} />
        </Button>
        <Button
          variant="ghost"
          icon
          title="Open in Zed"
          onClick={() => openZed(wt)}
        >
          <Code2 size={16} />
        </Button>
        <Button
          variant="ghost"
          icon
          title="Open terminal"
          onClick={() => openTerminal(wt)}
        >
          <Terminal size={16} />
        </Button>
        <Button
          variant="ghost"
          icon
          title="Repaint terminal (Ctrl/Cmd+Shift+R)"
          onClick={() => useStore.getState().sessionMgr?.repaintActive()}
        >
          <RotateCw size={16} />
        </Button>
        <Button
          variant="ghost"
          icon
          title="Remove"
          onClick={() => openDeleteDialog(wt)}
        >
          <Trash2 size={16} />
        </Button>
        <div className="ml-2">
          <ChangesToggle />
        </div>
      </div>
    </header>
  );
}
