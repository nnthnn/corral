import clsx from "clsx";
import { Loader2, Search, Ticket } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { useDialogRef } from "../hooks/useDialogRef";
import { attachIssueToAgent } from "../lib/linear";
import { invoke } from "../lib/tauri";
import { useStore } from "../store";
import type { LinearIssueSummary } from "../types";
import { Button } from "./Button";

/** Wait this long after the last keystroke before hitting Linear — one
 * request per word typed would be rude to the API and the rate limit. */
const SEARCH_DEBOUNCE_MS = 350;
/** How many recently-updated tickets the empty-state list shows. */
const RECENT_LIMIT = 10;

function close(): void {
  useStore.setState({ linearDialogOpen: false });
}

function openSettings(): void {
  useStore.setState({ linearDialogOpen: false, settingsOpen: true });
}

/** Type-ahead picker for the "Attach Linear ticket" action: search Linear,
 * choose an issue, and insert a reference to its full body into the active
 * agent session. Opens on a list of recently-updated tickets so the common
 * case (grab the ticket I'm working on) needs no typing. Native `<dialog>`
 * for the same focus/backdrop behavior every other corral dialog gets (see
 * `useDialogRef`). */
export function LinearIssueDialog() {
  const open = useStore((s) => s.linearDialogOpen);
  const apiKey = useStore((s) => s.settings.linear_api_key.trim());
  const ref = useDialogRef(open);

  const [query, setQuery] = useState("");
  const [recent, setRecent] = useState<LinearIssueSummary[]>([]);
  const [recentLoading, setRecentLoading] = useState(false);
  const [recentError, setRecentError] = useState<string | null>(null);
  const [searchResults, setSearchResults] = useState<LinearIssueSummary[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const [attaching, setAttaching] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  // Bumped on every request; stale responses (out-of-order, or from before
  // the query changed) compare unequal and are dropped.
  const searchRequestRef = useRef(0);
  const recentRequestRef = useRef(0);

  // Reset everything each time the picker opens.
  useEffect(() => {
    if (!open) return;
    searchRequestRef.current += 1;
    recentRequestRef.current += 1;
    setQuery("");
    setRecent([]);
    setRecentLoading(false);
    setRecentError(null);
    setSearchResults([]);
    setSearching(false);
    setSearchError(null);
    setActiveIndex(0);
    setAttaching(null);
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  // The empty-search default list: recently updated tickets.
  useEffect(() => {
    if (!open || !apiKey) return;
    const token = ++recentRequestRef.current;
    setRecentLoading(true);
    setRecentError(null);
    invoke("linear_recent", { apiKey, limit: RECENT_LIMIT })
      .then((rows) => {
        if (token !== recentRequestRef.current) return;
        setRecent(rows);
        setActiveIndex(0);
      })
      .catch((err) => {
        if (token !== recentRequestRef.current) return;
        setRecent([]);
        setRecentError(String(err));
      })
      .finally(() => {
        if (token === recentRequestRef.current) setRecentLoading(false);
      });
  }, [open, apiKey]);

  // Debounced search once the query is long enough to be useful. Shorter
  // queries fall back to the recent list loaded above.
  useEffect(() => {
    if (!open || !apiKey) return;
    const term = query.trim();
    if (term.length < 2) {
      searchRequestRef.current += 1;
      setSearchResults([]);
      setSearching(false);
      setSearchError(null);
      return;
    }
    const token = ++searchRequestRef.current;
    setSearching(true);
    const timer = window.setTimeout(() => {
      invoke("linear_search", { apiKey, query: term })
        .then((rows) => {
          if (token !== searchRequestRef.current) return;
          setSearchResults(rows);
          setActiveIndex(0);
          setSearchError(null);
        })
        .catch((err) => {
          if (token !== searchRequestRef.current) return;
          setSearchResults([]);
          setSearchError(String(err));
        })
        .finally(() => {
          if (token === searchRequestRef.current) setSearching(false);
        });
    }, SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [open, apiKey, query]);

  const term = query.trim();
  // Two characters is the search threshold (Linear's search term is useless
  // below that); until then the dialog shows recent tickets.
  const isSearchMode = term.length >= 2;
  const results = isSearchMode ? searchResults : recent;
  const loading = isSearchMode ? searching : recentLoading;
  const error = isSearchMode ? searchError : recentError;

  async function select(summary: LinearIssueSummary): Promise<void> {
    if (attaching) return;
    setAttaching(summary.id);
    if (isSearchMode) setSearchError(null);
    else setRecentError(null);
    try {
      const issue = await invoke("linear_issue", {
        apiKey,
        id: summary.id,
      });
      if (await attachIssueToAgent(issue)) close();
    } catch (err) {
      if (isSearchMode) setSearchError(String(err));
      else setRecentError(String(err));
    } finally {
      setAttaching(null);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>): void {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = results[Math.min(activeIndex, results.length - 1)];
      if (item) void select(item);
    }
  }

  return (
    <dialog
      ref={ref}
      id="linear-issue-dialog"
      onClose={close}
      // A click on the ::backdrop targets the dialog element itself, never
      // one of its children — the standard native-dialog click-outside test.
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      {!apiKey ? (
        <div className="flex flex-col gap-3 p-5">
          <h2 className="flex items-center gap-2">
            <Ticket size={16} /> Linear API key needed
          </h2>
          <p className="hint">
            Add a personal Linear API key under Settings to look up tickets and
            attach them to the agent.
          </p>
          <menu>
            <Button variant="ghost" onClick={close}>
              Cancel
            </Button>
            <Button variant="primary" onClick={openSettings}>
              Open Settings
            </Button>
          </menu>
        </div>
      ) : (
        <div className="flex max-h-[70vh] flex-col overflow-hidden">
          <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
            <Search size={16} className="shrink-0 text-muted" />
            <input
              ref={inputRef}
              type="text"
              placeholder="Issue ID (ENG-123) or words from the title…"
              autoComplete="off"
              spellCheck={false}
              className="w-full bg-transparent py-1 text-body text-text placeholder:text-muted focus:outline-none"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setActiveIndex(0);
              }}
              onKeyDown={onKeyDown}
            />
            {loading && (
              <Loader2 size={14} className="shrink-0 animate-spin text-muted" />
            )}
          </div>
          <ul className="min-h-0 flex-1 overflow-y-auto p-1.5">
            {error && (
              <li className="px-2 py-2 text-sub text-danger">{error}</li>
            )}
            {!error && results.length === 0 && (
              <li className="px-2 py-2 text-sub text-muted">
                {isSearchMode
                  ? searching
                    ? "Searching…"
                    : "No matching issues."
                  : recentLoading
                    ? "Loading recent tickets…"
                    : "No recent tickets. Type at least two characters to search."}
              </li>
            )}
            {!error && !isSearchMode && results.length > 0 && (
              <li className="px-2 py-1 text-label text-muted">
                Recent tickets
              </li>
            )}
            {results.map((issue, i) => {
              const isActive = i === activeIndex;
              const isAttaching = attaching === issue.id;
              return (
                <li key={issue.id}>
                  <button
                    type="button"
                    disabled={attaching !== null}
                    className={clsx(
                      "flex w-full cursor-pointer items-baseline gap-2 rounded-sm px-2 py-2 text-left disabled:cursor-default",
                      isActive ? "bg-surface-2" : "bg-transparent",
                    )}
                    onMouseEnter={() => setActiveIndex(i)}
                    onClick={() => void select(issue)}
                  >
                    <span className="shrink-0 font-mono text-label text-muted">
                      {issue.identifier}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-body">
                      {issue.title}
                    </span>
                    {isAttaching ? (
                      <Loader2
                        size={14}
                        className="shrink-0 animate-spin text-muted"
                      />
                    ) : (
                      issue.state && (
                        <span className="shrink-0 text-label text-muted">
                          {issue.state}
                        </span>
                      )
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="flex items-center gap-3 border-t border-border px-3 py-1.5 text-label text-muted">
            <span>↑↓ navigate</span>
            <span>↵ attach</span>
            <span>esc close</span>
          </div>
        </div>
      )}
    </dialog>
  );
}
