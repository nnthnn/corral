import clsx from "clsx";
import { Loader2, Search, Ticket } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { useDialogRef } from "../hooks/useDialogRef";
import { attachIssueToAgent, LINEAR_KEY_SAVED_EVENT } from "../lib/linear";
import { invoke } from "../lib/tauri";
import { showError } from "../lib/toasts";
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

/** Linear reports colors as `#rrggbb`. Anything else is dropped so a surprise
 * payload can't put arbitrary CSS into an inline style, and so a missing color
 * degrades to the neutral chip instead of an unstyled one. */
function hexColor(color: string | null): string | null {
  return color && /^#[0-9a-f]{6}$/i.test(color) ? color : null;
}

/** Relative luminance, for picking text that stays readable on a solid badge.
 * (WCAG 2.x definition.) */
function luminance(hex: string): number {
  const channel = (i: number) => {
    const c = Number.parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

/** Foreground for a solid badge in `hex`: whichever of near-white and
 * near-black contrasts more. Fixed ink beats `color-mix` here because the
 * project palette runs from `#5E6AD2` to `#F2C94C`, and a mid-tone like the
 * teal would fail against both if it had to share one foreground. */
function readableOn(hex: string): string {
  const bg = luminance(hex);
  const contrastWith = (other: number) =>
    (Math.max(bg, other) + 0.05) / (Math.min(bg, other) + 0.05);
  const white = contrastWith(1);
  const ink = contrastWith(luminance("#121212"));
  return white >= ink ? "#ffffff" : "#121212";
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
  // Gate on "is a key stored", not on the key itself — the webview never
  // receives it (see `Settings.linear_api_key`).
  const hasKey = useStore((s) => s.settings.linear_api_key_set);
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
  const listRef = useRef<HTMLUListElement | null>(null);
  // Bumped on every request; stale responses (out-of-order, or from before
  // the query changed) compare unequal and are dropped.
  const searchRequestRef = useRef(0);
  const recentRequestRef = useRef(0);
  /** Which "load generation" the recent list was last fetched for. The fetch
   * is deliberately *not* keyed on the key's value: `Settings` is re-sent
   * whole on every unrelated save, so keying on it would refetch Linear each
   * time the user toggles a notification preference. A generation is instead
   * bumped on every open and whenever a key is saved (`LinearKeySavedEvent`),
   * and the list refetches once per generation. */
  const recentGenerationRef = useRef(0);
  const recentLoadedGenerationRef = useRef<number | null>(null);

  // Reset everything each time the picker opens. Done during render rather
  // than in an effect on `open`: this dialog stays mounted while closed, so a
  // stale `query` from the previous session is still in state when `open`
  // flips. The fetching effects below (and the derived `isSearchMode` used by
  // the first render) would otherwise see it for one pass and fire a search
  // for the old term — the common path, since the dialog is often opened
  // straight after Settings. Comparing against the previous `open` value also
  // re-runs the reset when a key is added while the dialog sits open in its
  // "API key needed" state (the fields below only mount once a key exists).
  const resetKey = `${open}:${hasKey}`;
  const resetKeyRef = useRef(resetKey);
  if (resetKeyRef.current !== resetKey) {
    resetKeyRef.current = resetKey;
    // Invalidate anything in flight on *close* as well as open, so a response
    // still racing in (e.g. the user reopened from Settings after adding a key)
    // can never land in the next session's state. Safe to mutate refs during
    // render here: it only happens on an `open`/`hasKey` transition, and the
    // fetch effects below observe the bumped tokens.
    searchRequestRef.current += 1;
    recentRequestRef.current += 1;
    // Opening is a new generation (the list must load again). Closing is not:
    // bumping on close would advance the counter without the fetch effect
    // below re-running, which loops forever.
    if (open) {
      recentGenerationRef.current += 1;
      recentLoadedGenerationRef.current = null;
    }
    if (open) {
      setQuery("");
      setRecent([]);
      setRecentLoading(false);
      setRecentError(null);
      setSearchResults([]);
      setSearching(false);
      setSearchError(null);
      setActiveIndex(0);
      setAttaching(null);
    }
  }

  // Focus the search field when it appears — on open, and again when a key
  // added while the dialog is open mounts the fields for the first time.
  useEffect(() => {
    if (open && hasKey) requestAnimationFrame(() => inputRef.current?.focus());
  }, [open, hasKey]);

  // Keep the highlighted row visible while arrowing through a list taller
  // than the dialog — same approach as `CommandPalette`.
  useEffect(() => {
    if (!open) return;
    const rows = listRef.current?.querySelectorAll("[data-active]");
    rows?.[activeIndex]?.scrollIntoView({ block: "nearest" });
  }, [open, activeIndex]);

  // A newly saved or removed key changes what Linear can answer, and the
  // dialog may be gone (Settings replaced it) — so this listens on window
  // rather than through component state.
  useEffect(() => {
    const onKeySaved = () => {
      recentGenerationRef.current += 1;
      recentLoadedGenerationRef.current = null;
      setRecent([]);
    };
    window.addEventListener(LINEAR_KEY_SAVED_EVENT, onKeySaved);
    return () => window.removeEventListener(LINEAR_KEY_SAVED_EVENT, onKeySaved);
  }, []);

  // The empty-search default list: recently updated tickets.
  useEffect(() => {
    if (!open || !hasKey) return;
    if (recentLoadedGenerationRef.current === recentGenerationRef.current) {
      return;
    }
    recentLoadedGenerationRef.current = recentGenerationRef.current;
    const token = ++recentRequestRef.current;
    setRecentLoading(true);
    setRecentError(null);
    invoke("linear_recent", { limit: RECENT_LIMIT })
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
  }, [open, hasKey]);

  // Debounced search once the query is long enough to be useful. Shorter
  // queries fall back to the recent list loaded above.
  useEffect(() => {
    if (!open || !hasKey) return;
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
      invoke("linear_search", { query: term })
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
  }, [open, hasKey, query]);

  const term = query.trim();
  // Two characters is the search threshold (Linear's search term is useless
  // below that); until then the dialog shows recent tickets.
  const isSearchMode = term.length >= 2;
  const results = isSearchMode ? searchResults : recent;
  const loading = isSearchMode ? searching : recentLoading;
  const error = isSearchMode ? searchError : recentError;
  // Linear identifiers vary in width (`DEV-9`, `DEV-10`, `ENG-1234`), and
  // letting each row size its own leaves every title starting at a different
  // x. One shared width — the longest identifier on screen, in monospace
  // `ch` units — keeps the title column straight without truncating the wide
  // ones. The floor keeps a single short result from collapsing the column.
  const identifierChars = Math.max(
    6,
    ...results.map((issue) => issue.identifier.length),
  );

  async function select(summary: LinearIssueSummary): Promise<void> {
    if (attaching) return;
    setAttaching(summary.id);
    if (isSearchMode) setSearchError(null);
    else setRecentError(null);
    // The dialog can be dismissed (Escape/backdrop) mid-request, and errors
    // render inside it — so a failure after that would vanish silently.
    const stillOpen = () => useStore.getState().linearDialogOpen;
    try {
      const issue = await invoke("linear_issue", { id: summary.id });
      const attached = await attachIssueToAgent(issue);
      if (attached && stillOpen()) close();
    } catch (err) {
      const message = String(err);
      if (stillOpen()) {
        if (isSearchMode) setSearchError(message);
        else setRecentError(message);
      } else {
        showError(`Couldn't fetch the ticket: ${message}`);
      }
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
      // Escape fires `cancel` before `close` — block it while the fetch is in
      // flight, so an error still has the dialog to render into (`attach`
      // failures are toasts and are handled regardless).
      onCancel={(e) => {
        if (attaching) e.preventDefault();
      }}
      // A click on the ::backdrop targets the dialog element itself, never
      // one of its children — the standard native-dialog click-outside test.
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      {!hasKey ? (
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
          <ul ref={listRef} className="min-h-0 flex-1 overflow-y-auto p-1.5">
            {error && (
              <li className="px-1.5 py-2 text-sub text-danger">{error}</li>
            )}
            {!error && results.length === 0 && (
              <li className="px-1.5 py-2 text-sub text-muted">
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
              <li className="px-1.5 py-1 text-label text-muted">
                Recent tickets
              </li>
            )}
            {results.map((issue, i) => {
              const isActive = i === activeIndex;
              const isAttaching = attaching === issue.id;
              const projectColor = hexColor(issue.project_color);
              const stateColor = hexColor(issue.state_color);
              return (
                <li key={issue.id}>
                  <button
                    type="button"
                    disabled={attaching !== null}
                    data-active={isActive ? true : undefined}
                    className={clsx(
                      "flex w-full cursor-pointer items-center gap-2 rounded-sm px-1.5 py-2 text-left disabled:cursor-default",
                      isActive ? "bg-surface-2" : "bg-transparent",
                    )}
                    onMouseEnter={() => setActiveIndex(i)}
                    onClick={() => void select(issue)}
                  >
                    <span
                      className="shrink-0 font-mono text-label text-muted"
                      style={{ width: `${identifierChars}ch` }}
                    >
                      {issue.identifier}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-body">
                      {issue.title}
                    </span>
                    {issue.project && (
                      // Solid badge: the project's own color as the fill, with
                      // a near-black or near-white ink chosen per color so a
                      // light project (amber) and a dark one (indigo) both
                      // stay legible. `border` stays on both variants so a
                      // project-less row and one with a badge keep the same
                      // box height.
                      <span
                        className={clsx(
                          "flex max-w-[9rem] min-w-0 shrink-0 items-center rounded-sm border px-1.5 py-1 text-label font-medium leading-none",
                          !projectColor && "border-border text-muted",
                        )}
                        style={
                          projectColor
                            ? {
                                backgroundColor: projectColor,
                                borderColor: projectColor,
                                color: readableOn(projectColor),
                              }
                            : undefined
                        }
                        title={issue.project}
                      >
                        <span className="truncate">{issue.project}</span>
                      </span>
                    )}
                    {isAttaching ? (
                      <Loader2
                        size={14}
                        className="shrink-0 animate-spin text-muted"
                      />
                    ) : (
                      issue.state && (
                        <span className="flex shrink-0 items-center gap-1.5 text-label text-muted">
                          {stateColor && (
                            <span
                              className="size-1.5 shrink-0 rounded-full"
                              style={{ backgroundColor: stateColor }}
                            />
                          )}
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
