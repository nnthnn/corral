import { useEffect, useRef } from "react";
import { restartAgent, restartFresh } from "../lib/agent-actions";
import {
  onSessionExit,
  onSessionOutput,
  onSessionStatus,
} from "../lib/session-events";
import { scheduleSettingsSave } from "../lib/settings";
import { SessionManager } from "../session";
import { useStore } from "../store";

/**
 * Owns `#terminals` and the one `SessionManager` for the app's lifetime.
 * Deliberately outside the render-reactive part of the tree: xterm.js writes
 * PTY output straight to its own canvas via `term.write()` on every
 * `session-output` event (see `session.ts`), so high-frequency output never
 * touches React's reconciler — this component only calls `show`/`hideAll`
 * when the *selection* changes, not on every byte of terminal output.
 */
export function TerminalHost() {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const selectedId = useStore((s) => s.selectedId);
  const sessions = useStore((s) => s.sessions);
  const activeTab = useStore((s) => s.activeTab);
  const settingsOpen = useStore((s) => s.settingsOpen);

  useEffect(() => {
    if (!containerRef.current || useStore.getState().sessionMgr) return;
    const sessionMgr = new SessionManager({
      container: containerRef.current,
      fontSize: useStore.getState().settings.terminal_font_size,
      onFontSizeChange: (size) => {
        useStore.setState((s) => ({
          settings: { ...s.settings, terminal_font_size: size },
        }));
        scheduleSettingsSave();
      },
      fontFamily: useStore.getState().settings.terminal_font_family,
      onFontFamilyChange: (family) => {
        useStore.setState((s) => ({
          settings: { ...s.settings, terminal_font_family: family },
        }));
        scheduleSettingsSave();
      },
      onOutput: onSessionOutput,
      onExit: onSessionExit,
      onRestart: restartAgent,
      onResumeFailed: restartFresh,
      onStatus: onSessionStatus,
      onInput: (id) => {
        const notifiedWaiting = new Set(useStore.getState().notifiedWaiting);
        notifiedWaiting.delete(id); // your input starts a new turn
        useStore.setState({ notifiedWaiting });
      },
      onTerminalsChanged: () => {
        useStore.setState((s) => ({
          terminalsVersion: s.terminalsVersion + 1,
        }));
      },
    });
    useStore.setState({ sessionMgr });
  }, []);

  // `settingsOpen` isn't read in the body — it's a reset trigger so `show()`
  // (and its repaint fix below) reruns when the settings panel closes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentional reset-on-key-change
  useEffect(() => {
    const { sessionMgr } = useStore.getState();
    if (!sessionMgr) return;
    if (!selectedId) {
      sessionMgr.hideAll();
      return;
    }
    if (activeTab !== "agent") return;
    const running = sessions.get(selectedId)?.running ?? false;
    // Re-run `show()` (not just on selection change) whenever the Agent tab
    // becomes active again: switching away hides `#agent-pane` via the
    // `hidden` DOM attribute, and this webkit2gtk build doesn't reliably
    // repaint the xterm canvas when its `display:none` ancestor is
    // unhidden — `show()`'s `fitActive()` forces the resize/redraw that
    // fixes it (same fix already relied on when switching worktrees).
    if (sessionMgr.has(selectedId) || running) sessionMgr.show(selectedId);
    else sessionMgr.hideAll();
  }, [selectedId, sessions, activeTab, settingsOpen]);

  return (
    <div
      id="terminals"
      ref={containerRef}
      className="terminals absolute inset-0"
    />
  );
}
