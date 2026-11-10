import clsx from "clsx";
import { useShallow } from "zustand/react/shallow";
import { currentWt, sessionStatus, useStore } from "../store";
import { wtId } from "../utils";
import { Detail } from "./Detail";
import { TerminalHost } from "./TerminalHost";

export function AgentPane({ hidden }: { hidden: boolean }) {
  const wt = useStore(currentWt);
  const sessions = useStore((s) => s.sessions);
  const sessionMgr = useStore((s) => s.sessionMgr);
  const id = wt ? wtId(wt) : null;
  // sessionStatus builds a fresh { cls, label } object per call — useShallow
  // compares its fields instead of object identity, or Zustand's default
  // reference check would never consider two calls "equal" and loop forever
  // re-rendering once the store actually updates (see React's
  // useSyncExternalStore tearing-check retry behavior).
  const status = useStore(
    useShallow((s) => (id ? sessionStatus(s, id) : null)),
  );
  // Never read directly — `sessionMgr.has(id)` below reads a plain `Map`
  // owned by a class instance, invisible to React on its own. Subscribing
  // to this counter (bumped by `SessionManager` on every entry create/
  // dispose, see its own doc comment) is what actually forces `hasTerminal`
  // to be recomputed once the entry exists, instead of however long it
  // takes some unrelated store update to trigger the next render.
  useStore((s) => s.terminalsVersion);

  const running = id ? (sessions.get(id)?.running ?? false) : false;
  const hasTerminal = id ? (sessionMgr?.has(id) ?? false) : false;
  // A session can be `running` before its xterm entry exists — `start_session`
  // resolving (or a prior-life session discovered via `list_sessions` on
  // load) flips `running` true, but the terminal itself is created lazily
  // and asynchronously by `TerminalHost`'s effect right after. Keying
  // `showDetail` off `running` alone left that gap rendering neither Detail
  // nor a terminal — a blank pane the user saw as "the xterm disappears."
  // Requiring `hasTerminal` keeps Detail mounted (with its loading state,
  // see `loadingTerminal` below) through that whole gap instead.
  const showDetail = !wt || !hasTerminal;
  const loadingTerminal = running && !hasTerminal;

  return (
    <div id="agent-pane" className="tab-pane absolute inset-0" hidden={hidden}>
      <div className="relative flex-1 min-h-0">
        <TerminalHost />
        {/* Only while stopped/done/exited — a running session's working/waiting
         * state already shows via the Agent tab's lasso spinner and the
         * sidebar's status dot + attention cue, so this floating chip would
         * be redundant on top of the terminal. */}
        {status && !running && (
          <span
            className={clsx("status-chip status-chip-floating", status.cls)}
          >
            {status.label}
          </span>
        )}
        {showDetail && (
          <Detail wt={wt ?? null} loadingTerminal={loadingTerminal} />
        )}
      </div>
    </div>
  );
}
