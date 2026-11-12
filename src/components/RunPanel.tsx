import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { clsx } from "clsx";
import { Play, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { drainOutput } from "../lib/output-buffer";
import { invoke } from "../lib/tauri";
import { attachCopyOnCtrlC, createLinkedTerminal } from "../lib/terminal";
import { showError } from "../lib/toasts";
import { THEME } from "../session";
import { useStore } from "../store";
import type { RunTarget, RunTargets, Worktree } from "../types";
import { wtId } from "../utils";
import { Button } from "./Button";
import styles from "./RunPanel.module.css";

/** Stable empty row for worktrees with no justfile/package scripts, so the
 * `targets` value doesn't get a new identity on every render. */
const EMPTY_TARGETS: RunTargets = { source: null, targets: [] };

/** The Run tab's content for the currently selected worktree — a persistent
 * command input plus a read-only terminal streaming a long-lived dev-server
 * process (e.g. `pnpm dev`), started/stopped only by explicit user action
 * (never tied to the agent session's own lifecycle). See
 * `src-tauri/src/commands/run.rs` for the backend side, a thin wrapper over
 * the same one-shot-PTY core `provision.rs`'s Setup panel uses
 * (`pty_process.rs`). Unlike Setup, there's no collapse/expand here — this
 * is a dedicated tab, not a strip docked under the agent terminal. */
export function RunPanel({ wt }: { wt: Worktree }) {
  const id = wtId(wt);
  const state = useStore((s) => s.run.get(id));
  // Keyed by worktree id, not a single string — remembers what the user
  // typed (or the auto-detected prefill) per worktree across tab/worktree
  // switches, since RunPanel stays mounted (just hidden) rather than
  // unmounting when the Run tab isn't active.
  const [commandByWtId, setCommandByWtId] = useState(
    () => new Map<string, string>(),
  );
  const command = commandByWtId.get(id) ?? "";
  function setCommand(value: string) {
    setCommandByWtId((prev) => new Map(prev).set(id, value));
  }
  // One-click targets per worktree: the justfile's public recipes, or (when
  // there's no justfile) the package.json scripts. Refetched on every view of
  // a worktree id (not just the first) so edits to either file show up after
  // switching away and back; the map keeps the previous list visible during
  // the refetch instead of flickering.
  const [targetsByWtId, setTargetsByWtId] = useState(
    () => new Map<string, RunTargets>(),
  );
  const targets = targetsByWtId.get(id) ?? EMPTY_TARGETS;
  // The last target clicked in each worktree, so its chip can stay highlighted
  // as a visual bookmark. Kept per worktree id like the command map; purely
  // in-memory (the run_command saved by `start_run` still pre-fills the input
  // after a restart).
  const [lastTargetByWtId, setLastTargetByWtId] = useState(
    () => new Map<string, string>(),
  );
  const lastTarget = lastTargetByWtId.get(id) ?? null;
  const inputRef = useRef<HTMLInputElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<{
    term: Terminal;
    fit: FitAddon;
    written: number;
  } | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: seeds the per-worktree command map only on first view of `id`, not on every `commandByWtId`/`setCommand` identity change
  useEffect(() => {
    // Reattach: a panel mounted after navigating away and back has no
    // in-memory entry yet (the event listeners only ever append to an
    // existing one), so fetch the backend's retained state directly.
    if (!useStore.getState().run.has(id)) {
      invoke("get_run_output", { key: id })
        .then((fetched) => {
          if (!fetched) return;
          // Live events can create the entry while this fetch is in flight;
          // installing the older snapshot would clobber the bytes they added.
          if (useStore.getState().run.has(id)) return;
          const run = new Map(useStore.getState().run);
          run.set(id, fetched);
          useStore.setState({ run });
        })
        .catch((err) => showError(`Run output wouldn't load: ${err}`));
    }
    // Always refetch (not just when the map lacks `id`), so a justfile or
    // package.json edited since this worktree was last viewed shows up. Guard
    // the shape: tests and older backends may resolve `undefined`/`null` for
    // this optional command.
    invoke("list_run_targets", { project: wt.project, feature: wt.feature })
      .then((list) => {
        setTargetsByWtId((prev) =>
          new Map(prev).set(
            id,
            list && Array.isArray(list.targets) ? list : EMPTY_TARGETS,
          ),
        );
      })
      .catch(() => {
        setTargetsByWtId((prev) => new Map(prev).set(id, EMPTY_TARGETS));
      });
    if (commandByWtId.has(id)) return;
    invoke("resolve_run_command", { project: wt.project, feature: wt.feature })
      .then((resolved) => setCommand(resolved))
      .catch((err) => showError(`Couldn't resolve a run command: ${err}`));
  }, [id, wt.project, wt.feature]);

  // Create the terminal (once per worktree id) and write any newly-arrived
  // bytes in the same effect, same reasoning as `ProvisionPanel`'s
  // equivalent effect: keying only on `id` would miss writes that arrive
  // without a fresh mount, and keying only on `state.output` would miss
  // creating the terminal for a worktree that already has retained output
  // by the time this component first mounts.
  useEffect(() => {
    if (!containerRef.current) return;
    if (!termRef.current) {
      const { term, fit } = createLinkedTerminal({
        fontSize: useStore.getState().settings.terminal_font_size,
        cursorBlink: false,
        disableStdin: true,
        convertEol: true,
        theme: THEME,
      });
      term.open(containerRef.current);
      attachCopyOnCtrlC(term);
      fit.fit();
      invoke("resize_run", { key: id, rows: term.rows, cols: term.cols }).catch(
        () => {},
      );
      termRef.current = { term, fit, written: 0 };
    }

    const entry = termRef.current;
    if (!state) return;
    // Cursor-based, not length-based: the retained buffer is front-trimmed at
    // its cap (see `output-buffer.ts`), so its length stops growing even
    // while new output keeps arriving. `drainOutput` also reports a rewind
    // (a fresh Run replacing the buffer), which clears the terminal.
    const delta = drainOutput(state, entry.written);
    if (delta.reset) entry.term.reset();
    if (delta.data.length > 0) entry.term.write(new Uint8Array(delta.data));
    entry.written = delta.next;
  }, [id, state]);

  // A plain `window resize` listener misses the case that actually bit us:
  // this panel mounts (and does its first `fit()`) while the Run tab is
  // still `hidden` (`display: none`) behind whatever tab was active on
  // worktree switch, so the container measures 0×0 and the terminal falls
  // back to xterm's default 80×24 — producing the garbled, wide-spaced
  // rendering you get once the pane becomes visible without ever being
  // told to re-fit. `ResizeObserver` fires on *any* box-size change,
  // including a hidden→visible transition once the Run tab becomes active,
  // so it self-heals regardless of when that happens (same fix as
  // the setup panel's equivalent effect). Also disposes the terminal whenever
  // the worktree changes or the component unmounts.
  useEffect(() => {
    if (!containerRef.current) return;
    const observer = new ResizeObserver(() => {
      const entry = termRef.current;
      const el = containerRef.current;
      if (!entry || !el) return;
      // This panel stays mounted while its tab is `hidden` (MainPane), where
      // the box measures 0×0 — fitting then would resize the live Run PTY to
      // xterm's 1×1 minimum for a frame. It refits when the tab is shown.
      if (el.clientWidth === 0 || el.clientHeight === 0) return;
      entry.fit.fit();
      invoke("resize_run", {
        key: id,
        rows: entry.term.rows,
        cols: entry.term.cols,
      }).catch(() => {});
    });
    observer.observe(containerRef.current);
    return () => {
      observer.disconnect();
      termRef.current?.term.dispose();
      termRef.current = null;
    };
  }, [id]);

  const running = state?.running ?? false;

  /** Clear the log before a fresh run, and rewind the replay cursor so new
   * bytes can't be skipped if they arrive before React re-renders. */
  function resetLog() {
    const entry = termRef.current;
    if (!entry) return;
    entry.term.reset();
    entry.written = 0;
  }

  async function startRun(cmd: string) {
    resetLog();
    const run = new Map(useStore.getState().run);
    run.set(id, { output: [], running: true, exit_code: null });
    useStore.setState({ run });
    try {
      await invoke("start_run", {
        project: wt.project,
        feature: wt.feature,
        command: cmd,
      });
    } catch (err) {
      const rolledBack = new Map(useStore.getState().run);
      rolledBack.set(id, { output: [], running: false, exit_code: null });
      useStore.setState({ run: rolledBack });
      showError(`Run wouldn't start: ${err}`);
    }
  }

  function handleRun() {
    return startRun(command);
  }

  /** One-click run target: run it with no arguments when possible. A recipe
   * with required parameters can't run bare, so prefill the command and put
   * the caret in the input for the user to finish (and press ▶). Package
   * scripts never have required parameters, so they always run immediately. */
  function handleTarget(target: RunTarget) {
    setLastTargetByWtId((prev) => new Map(prev).set(id, target.label));
    if (target.required_params.length > 0) {
      setCommand(`${target.command} `);
      inputRef.current?.focus();
      return;
    }
    setCommand(target.command);
    void startRun(target.command);
  }

  function targetTitle(target: RunTarget): string {
    const parts = [target.command];
    if (target.doc) parts.push(target.doc);
    if (target.required_params.length > 0) {
      parts.push(`needs: ${target.required_params.join(", ")}`);
    }
    return parts.join(" — ");
  }

  async function handleStop() {
    try {
      await invoke("stop_run", { key: id });
    } catch (err) {
      showError(`Run wouldn't stop: ${err}`);
    }
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-4 py-2 border-b border-border">
        <input
          ref={inputRef}
          type="text"
          className="flex-1 bg-surface-2 border border-border rounded-md px-2 py-1 text-body"
          placeholder="e.g. pnpm dev"
          value={command}
          disabled={running}
          onChange={(e) => setCommand(e.target.value)}
        />
        {running ? (
          <Button variant="ghost" icon title="Stop run" onClick={handleStop}>
            <Square size={14} />
          </Button>
        ) : (
          <Button
            variant="primary"
            icon
            title="Run"
            disabled={!command.trim()}
            onClick={handleRun}
          >
            <Play size={14} />
          </Button>
        )}
      </div>
      {targets.targets.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 px-4 py-1.5 border-b border-border max-h-24 overflow-y-auto">
          <span className="text-label font-mono text-muted shrink-0 mr-1">
            {targets.source}
          </span>
          {targets.targets.map((target) => (
            <Button
              key={target.command}
              className={clsx(
                "font-mono",
                target.label === lastTarget && styles.selected,
              )}
              title={targetTitle(target)}
              disabled={running}
              onClick={() => handleTarget(target)}
            >
              {target.label}
            </Button>
          ))}
        </div>
      )}
      {!running && state?.exit_code != null && (
        <p className="px-4 py-1 text-body text-muted">
          Exited (code {state.exit_code})
        </p>
      )}
      <div className="relative flex-1 min-h-0 mx-2 mb-2">
        <div
          ref={containerRef}
          data-testid="run-terminal"
          className="absolute inset-0 rounded-md bg-surface-2 p-1 xterm-fit"
        />
        {!state && (
          <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
            <p className="text-sub text-muted text-center max-w-64">
              Nothing running yet. Enter a command above and hit ▶ to start it.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
