import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { RotateCw, Square } from "lucide-react";
import { useEffect, useRef } from "react";
import { invoke } from "../lib/tauri";
import { attachCopyOnCtrlC, createLinkedTerminal } from "../lib/terminal";
import { showError } from "../lib/toasts";
import { THEME } from "../session";
import { useStore } from "../store";
import type { Worktree } from "../types";
import { wtId } from "../utils";
import { Button } from "./Button";

/** The Setup tab's content for the currently selected worktree — a read-only
 * xterm.js terminal streaming the worktree's post-create setup command (e.g.
 * `pnpm install`), with Re-run/Stop controls. See
 * `src-tauri/src/commands/provision.rs` for the backend side.
 *
 * This used to be a strip docked at the bottom of the Agent pane, shown only
 * when a `provisioning` entry existed. That made it easy to lose: it was
 * invisible on every other tab, and because provisioning output is retained
 * only in memory, it vanished entirely after an app restart — taking the
 * re-run affordance with it. As an always-present tab (see
 * `worktreeTabIds`), the log is reachable this app-run and Re-run is always
 * available, even when there's nothing to replay yet. */
export function ProvisionPanel({ wt }: { wt: Worktree }) {
  const id = wtId(wt);
  const state = useStore((s) => s.provisioning.get(id));
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<{
    term: Terminal;
    fit: FitAddon;
    written: number;
  } | null>(null);

  // Whether the project has a setup command at all — drives whether Re-run is
  // offered. A worktree from before this field existed reads as `false`; the
  // only cost is a slightly conservative empty state, never a broken action
  // (the backend would reject a re-run with no command anyway).
  const hasCommand = wt.has_setup_command === true;

  useEffect(() => {
    // Reattach: a panel mounted after navigating away and back (or after an
    // app restart, for output still retained in the backend's in-memory
    // registry) has no in-memory entry yet — the event listeners only ever
    // append to an existing one — so fetch the backend's retained state
    // directly.
    if (useStore.getState().provisioning.has(id)) return;
    invoke("get_provision_output", { key: id })
      .then((fetched) => {
        if (!fetched) return;
        const provisioning = new Map(useStore.getState().provisioning);
        provisioning.set(id, fetched);
        useStore.setState({ provisioning });
      })
      .catch((err) => showError(`Setup output wouldn't load: ${err}`));
  }, [id]);

  // Create the terminal (once per worktree id) and write any newly-arrived
  // bytes in the same effect — keying only on `id` would miss writes that
  // arrive without a fresh mount, and keying only on `state?.output` would
  // miss creating the terminal for a worktree that already has retained
  // output by the time this tab is first opened. Mirrors `RunPanel`.
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
      invoke("resize_provision", {
        key: id,
        rows: term.rows,
        cols: term.cols,
      }).catch(() => {});
      termRef.current = { term, fit, written: 0 };
    }

    const entry = termRef.current;
    const output = state?.output;
    if (!output) return;
    // A shrink (e.g. `handleRerun` resetting to `output: []`) means a fresh
    // run started, so clear the terminal and start over.
    if (output.length < entry.written) {
      entry.term.reset();
      entry.written = 0;
    }
    if (output.length > entry.written) {
      entry.term.write(new Uint8Array(output.slice(entry.written)));
      entry.written = output.length;
    }
  }, [id, state?.output]);

  // A plain `window resize` listener misses the case that actually bit the
  // Run panel: this tab's pane is mounted while hidden (`display: none`)
  // behind whatever tab was active on a worktree switch, so its first `fit()`
  // measures 0×0 and xterm falls back to 80×24 — the garbled, wide-spaced
  // rendering you get once the pane becomes visible. `ResizeObserver` fires on
  // *any* box-size change, including a hidden→visible transition, so it
  // self-heals. Also disposes the terminal whenever the worktree changes or
  // the component unmounts.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      const entry = termRef.current;
      if (!entry) return;
      // The Setup tab's pane is `display: none` while inactive, so its
      // container measures 0×0. Skip rather than shrink the live PTY to
      // xterm's 1×1 minimum — the observer refires once it's shown again.
      if (el.clientWidth === 0 || el.clientHeight === 0) return;
      entry.fit.fit();
      invoke("resize_provision", {
        key: id,
        rows: entry.term.rows,
        cols: entry.term.cols,
      }).catch(() => {});
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      termRef.current?.term.dispose();
      termRef.current = null;
    };
  }, [id]);

  const title = setupTitle(hasCommand, state);

  async function handleRerun() {
    const provisioning = new Map(useStore.getState().provisioning);
    provisioning.set(id, { output: [], running: true, exit_code: null });
    useStore.setState({ provisioning });
    try {
      await invoke("rerun_provision", {
        project: wt.project,
        feature: wt.feature,
      });
    } catch (err) {
      const rolledBack = new Map(useStore.getState().provisioning);
      rolledBack.set(id, { output: [], running: false, exit_code: null });
      useStore.setState({ provisioning: rolledBack });
      showError(`Setup wouldn't restart: ${err}`);
    }
  }

  async function handleStop() {
    try {
      await invoke("stop_provision", { key: id });
    } catch (err) {
      showError(`Setup wouldn't stop: ${err}`);
    }
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-4 py-2 border-b border-border">
        <span className="flex-1 text-body font-semibold min-w-0 whitespace-nowrap overflow-hidden text-ellipsis">
          {title}
        </span>
        {state?.running ? (
          <Button variant="ghost" icon title="Stop setup" onClick={handleStop}>
            <Square size={14} />
          </Button>
        ) : (
          <Button
            variant="primary"
            icon
            title="Re-run setup"
            disabled={!hasCommand}
            onClick={handleRerun}
          >
            <RotateCw size={14} />
          </Button>
        )}
      </div>
      <div className="relative flex-1 min-h-0 mx-2 mb-2 mt-2">
        <div
          ref={containerRef}
          data-testid="provision-terminal"
          className="absolute inset-0 rounded-md bg-surface-2 p-1 xterm-fit"
        />
        {(!state || state.output.length === 0) && (
          <div
            data-testid="provision-empty"
            className="pointer-events-none absolute inset-0 flex items-center justify-center"
          >
            <p className="text-sub text-muted text-center max-w-64">
              {hasCommand
                ? "No setup output yet. Hit the re-run button above to run setup."
                : `No setup command configured for ${wt.project}. Add one in project settings to use this tab.`}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

/** Header line for the Setup tab — the running/finished/failed states match
 * the old docked panel's wording, plus two states that panel never needed:
 * a project with no setup command, and a worktree whose setup hasn't run
 * this app-run (no retained output). */
function setupTitle(
  hasCommand: boolean,
  state: { running: boolean; exit_code: number | null } | undefined,
): string {
  if (!hasCommand) return "No setup command configured";
  if (!state) return "Setup hasn't run";
  if (state.running) return "Running setup…";
  if (state.exit_code === 0) return "Setup finished";
  return `Setup went down (exit ${state.exit_code ?? "?"})`;
}
