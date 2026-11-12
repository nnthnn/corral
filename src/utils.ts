import type { ChecksSummary, GitStatus, PrInfo, Worktree } from "./types";

export function clampWidth(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/** Formats a byte count as a compact human-readable size ("512.0 MB",
 * "1.01 GB") — for the sidebar header's total-RSS readout. */
export function formatBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(2)} GB`;
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

/** Shortens an absolute path under the user's home directory to `~/...` for
 * display — never use the result to invoke a command that expects a real
 * path (e.g. `open_in_zed`), only for showing paths to the user. */
export function shortenPath(path: string, homeDir: string | undefined): string {
  // Require a path boundary after the home dir: a bare `startsWith` would
  // wrongly shorten `/home/aliceson/...` under `/home/alice` to `~son/...`.
  if (homeDir && (path === homeDir || path.startsWith(`${homeDir}/`))) {
    return `~${path.slice(homeDir.length)}`;
  }
  return path;
}

export const sessionId = (project: string, feature: string) =>
  `${project}/${feature}`;

export const wtId = (wt: Worktree) => sessionId(wt.project, wt.feature);

export const worktreeTitle = (wt: Worktree): string => wt.title || wt.slug;

/** Whether a worktree row survives the sidebar's text filter — shared by the
 * sidebar itself and the keyboard shortcuts, so `Ctrl+1–9` numbers the rows
 * you can actually see. */
export function matchesWorktreeFilter(
  wt: Worktree,
  filterText: string,
): boolean {
  if (!filterText) return true;
  return (
    wt.project.toLowerCase().includes(filterText) ||
    worktreeTitle(wt).toLowerCase().includes(filterText) ||
    wt.feature.toLowerCase().includes(filterText) ||
    wt.branch.toLowerCase().includes(filterText)
  );
}

export type FeatureStatus =
  | "new"
  | "in-progress"
  | "committed"
  | "pr_draft"
  | "pr_open"
  | "pr_closed"
  | "merged"
  | "broken";

/** Derives an at-a-glance lifecycle status for a worktree from local git
 * state plus (if known) its GitHub PR state — the single source of truth
 * behind the sidebar dot (`FeatureStatusIcon`), `isFullyMerged`, and
 * `MergeButton`'s no-remote merge path. Replaces the old `featureStatus`,
 * which only ever looked at local git state. Order matters and is
 * deliberate:
 *
 * 1. `status` is `null` only when the worktree's directory isn't a readable
 *    git repo (see `Worktree.status` in `worktree.rs`) — e.g. a leftover
 *    directory whose git registration is gone. That's a distinct "broken"
 *    state, not "unknown yet", so it gets its own icon rather than
 *    rendering as a blank row indistinguishable from a real worktree.
 * 2. Otherwise, dirty (uncommitted edits) always wins, even over "merged"
 *    — a worktree merged on GitHub that's since had new edits dropped into
 *    it is still in-progress, not done. This generalizes the fix already
 *    made for the "stale merged banner" bug: `merged_into_base`/
 *    `justMerged` alone can't tell that new dirty work landed after the
 *    merge.
 * 3. GitHub's own PR state is authoritative once known: `merged`/`closed`
 *    on GitHub is correct even when local git ancestry can't confirm it
 *    (e.g. a squash merge creates a brand-new commit hash, so
 *    `merged_into_base`'s ancestor check would never catch it — see
 *    `PrState`'s doc comment in `pr.rs`). Local `merged_into_base`/
 *    `justMerged` are only consulted when no PR info is available at all
 *    (no remote, or no PR ever opened).
 * 4. An open PR splits into `pr_draft`/`pr_open` before falling back to
 *    local-only `committed`/`new` when no PR exists yet. */
export function deriveLifecycleState(
  status: GitStatus | null,
  prInfo: PrInfo | null,
  justMerged: boolean,
): FeatureStatus {
  if (!status) return "broken";
  const dirty = status.staged + status.modified + status.untracked;
  if (dirty > 0) return "in-progress";
  if (prInfo) {
    if (prInfo.state === "merged") return "merged";
    if (prInfo.state === "closed") return "pr_closed";
    return prInfo.is_draft ? "pr_draft" : "pr_open";
  }
  if (status.merged_into_base) return "merged";
  if (justMerged && status.commits_ahead_of_base === 0) return "merged";
  if (status.commits_ahead_of_base > 0) return "committed";
  return "new";
}

/** True once there's nothing left to do on this branch but remove it — see
 * `deriveLifecycleState` for the full precedence rules (dirty vetoes,
 * GitHub PR state takes priority over local git ancestry when known). */
export function isFullyMerged(
  status: GitStatus | null,
  prInfo: PrInfo | null,
  justMerged: boolean,
): boolean {
  return deriveLifecycleState(status, prInfo, justMerged) === "merged";
}

/** Entity-encodes `& < > " '` — a small self-contained escape so this module
 * stays DOM-free (unlike `ui/templates.ts`'s `html` tag, it can run under
 * plain Node in tests, no jsdom required). */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** Escape `text`, wrapping the first case-insensitive match of `needle` (the
 * active filter) in a <mark> so the sidebar can show why a row matched. */
export function highlightMatch(text: string, needle: string): string {
  if (!needle) return escapeHtml(text);
  const idx = text.toLowerCase().indexOf(needle);
  if (idx === -1) return escapeHtml(text);
  const before = text.slice(0, idx);
  const match = text.slice(idx, idx + needle.length);
  const after = text.slice(idx + needle.length);
  return `${escapeHtml(before)}<mark class="match">${escapeHtml(match)}</mark>${escapeHtml(after)}`;
}

/** Splits a rename into a shared directory (if old and new paths live in
 * the same one) plus the two filenames — lets the Changes pane show
 * `dir/ old.tsx → new.tsx` instead of repeating the whole path on both
 * sides. Returns `null` when the directory itself changed too, since
 * there's nothing to share in that case. */
export function splitRenamedPath(
  oldPath: string,
  newPath: string,
): { dir: string; oldName: string; newName: string } | null {
  const oldSlash = oldPath.lastIndexOf("/");
  const newSlash = newPath.lastIndexOf("/");
  const oldDir = oldSlash === -1 ? "" : oldPath.slice(0, oldSlash + 1);
  const newDir = newSlash === -1 ? "" : newPath.slice(0, newSlash + 1);
  if (oldDir !== newDir) return null;
  return {
    dir: oldDir,
    oldName: oldPath.slice(oldSlash + 1),
    newName: newPath.slice(newSlash + 1),
  };
}

const SUMMARY_MAX_LEN = 140;

/** Reduces a raw assistant message (a session's `last_summary`) to a
 * flattened one-line sidebar summary: strips the common markdown syntax
 * Claude's own final messages tend to use (fenced/inline code, headers,
 * emphasis, links), collapses newlines/whitespace to single spaces, then
 * hard-truncates with an ellipsis. Not a full markdown parser — just enough
 * to keep raw `#`/`**`/backticks from leaking into a one-liner. */
export function summarizeText(raw: string, maxLen = SUMMARY_MAX_LEN): string {
  const stripped = raw
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/__([^_]+)__/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/_([^_]+)_/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
  if (stripped.length <= maxLen) return stripped;
  return `${stripped.slice(0, maxLen - 1).trimEnd()}…`;
}

/** Merges the backend's alphabetical project list with a user-customized
 * drag-to-reorder order: listed projects keep that order, anything new (or
 * from before `project_order` existed) falls back to alphabetical, appended
 * after. Drops entries for projects that no longer exist. */
export function orderProjects(projects: string[], order: string[]): string[] {
  const known = new Set(projects);
  const ordered = order.filter((p) => known.has(p));
  const placed = new Set(ordered);
  const remaining = projects.filter((p) => !placed.has(p));
  return [...ordered, ...remaining];
}

/** Worktree ids in the exact order `WorktreeTree` renders them: projects via
 * `orderProjects`, worktrees within a project in their existing array order.
 * `isVisible` lets callers mirror the sidebar's filter — the keyboard
 * shortcuts use it so `Ctrl+1–9` numbers the rows on screen, not the raw
 * `worktrees` array. */
export function orderedWorktreeIds(
  worktrees: Worktree[],
  projects: string[],
  projectOrder: string[],
  isVisible?: (wt: Worktree) => boolean,
): string[] {
  const ordered = orderProjects(projects, projectOrder);
  const ids: string[] = [];
  for (const project of ordered) {
    for (const wt of worktrees) {
      if (wt.project !== project) continue;
      if (isVisible && !isVisible(wt)) continue;
      ids.push(wtId(wt));
    }
  }
  return ids;
}

/** Picks what should become selected after `removedId` is removed — the
 * next worktree in the same sidebar order `WorktreeTree` renders (projects
 * via `orderProjects`, worktrees within a project in their existing array
 * order), falling back to the previous one if `removedId` was last, or
 * `null` if it was the only worktree (or isn't found at all). */
export function adjacentWorktreeId(
  worktrees: Worktree[],
  projects: string[],
  projectOrder: string[],
  removedId: string,
): string | null {
  const flattened = orderedWorktreeIds(worktrees, projects, projectOrder);
  const index = flattened.indexOf(removedId);
  if (index === -1) return null;
  if (flattened[index + 1] !== undefined) return flattened[index + 1];
  if (flattened[index - 1] !== undefined) return flattened[index - 1];
  return null;
}

/** Moves `project` one spot up (-1) or down (+1) within `projects`, clamped
 * at the ends — the full list to persist as the new `project_order`. */
export function moveProject(
  projects: string[],
  project: string,
  direction: -1 | 1,
): string[] {
  const from = projects.indexOf(project);
  if (from === -1) return projects;
  const to = from + direction;
  if (to < 0 || to >= projects.length) return projects;
  const next = [...projects];
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}

/** Reorders `projects` so `dragged` lands immediately before/after `target`
 * — the full list to persist as the new `project_order`. No-op if either
 * name is missing or they're the same project. */
export function reorderProjects(
  projects: string[],
  dragged: string,
  target: string,
  side: "before" | "after",
): string[] {
  if (dragged === target) return projects;
  const without = projects.filter((p) => p !== dragged);
  const targetIndex = without.indexOf(target);
  if (targetIndex === -1) return projects;
  const insertAt = side === "before" ? targetIndex : targetIndex + 1;
  const next = [...without];
  next.splice(insertAt, 0, dragged);
  return next;
}

const RELATIVE_TIME_FORMAT = new Intl.RelativeTimeFormat("en", {
  numeric: "auto",
});
const RELATIVE_TIME_UNITS: [
  unit: Intl.RelativeTimeFormatUnit,
  seconds: number,
][] = [
  ["year", 60 * 60 * 24 * 365],
  ["month", 60 * 60 * 24 * 30],
  ["week", 60 * 60 * 24 * 7],
  ["day", 60 * 60 * 24],
  ["hour", 60 * 60],
  ["minute", 60],
];

/** Formats a Unix timestamp (seconds) as a short relative string ("2d ago",
 * "in 3h") for the sidebar hover card's last-commit time — `nowMs` defaults
 * to the real clock but is overridable for deterministic tests. */
export function relativeTime(unixSeconds: number, nowMs = Date.now()): string {
  const deltaSeconds = unixSeconds - Math.floor(nowMs / 1000);
  for (const [unit, unitSeconds] of RELATIVE_TIME_UNITS) {
    if (Math.abs(deltaSeconds) >= unitSeconds) {
      return RELATIVE_TIME_FORMAT.format(
        Math.round(deltaSeconds / unitSeconds),
        unit,
      );
    }
  }
  return RELATIVE_TIME_FORMAT.format(Math.round(deltaSeconds / 60), "minute");
}

export type RightSlotIndicator =
  | { kind: "running" }
  | { kind: "attention" }
  | { kind: "checks-failed" }
  | { kind: "checks-running" }
  | { kind: "conflict" }
  | { kind: "diff"; added: number; removed: number }
  | { kind: "sync"; ahead: number; behind: number }
  | { kind: "none" };

/** Picks the single highest-priority signal for a row's right slot — a
 * running Run command beats attention, which beats failed CI, which beats a
 * rebase conflict, which beats running CI, which beats diff stats, which
 * beats ahead/behind, which beats nothing. Running wins over attention
 * because it's actively in-progress feedback (the Run command is doing
 * something right now), whereas attention is a passive "come look at this
 * when you get a chance" cue. Failed CI sits just below attention: both are
 * "something needs you", but attention means an agent is blocked on input
 * right now. Running CI outranks diff stats because a diff is static
 * bookkeeping, while CI in flight is a live change worth watching for.
 * Diff stats prefer the dirty worktree's own uncommitted added/removed line
 * counts when there's anything uncommitted, falling back to the branch's
 * total lines changed vs its base otherwise — so the row always shows the
 * numbers most relevant to what's actually changed. The full breakdown
 * (conflicts, individual staged/modified/untracked counts, etc.) still
 * lives in the hover card — this only decides what's worth surfacing at a
 * glance. */
export function rightSlotIndicator(
  status: GitStatus | null,
  attention: boolean,
  running = false,
  checks?: ChecksSummary | null,
): RightSlotIndicator {
  if (running) return { kind: "running" };
  if (attention) return { kind: "attention" };
  if (checks && checks.failed > 0) return { kind: "checks-failed" };
  if (!status) return { kind: "none" };
  if (status.conflicts_with_base) return { kind: "conflict" };
  if (checks && checks.pending > 0) return { kind: "checks-running" };
  const dirty = status.staged + status.modified + status.untracked;
  if (dirty > 0 && (status.added > 0 || status.removed > 0)) {
    return { kind: "diff", added: status.added, removed: status.removed };
  }
  if (dirty === 0 && (status.base_added > 0 || status.base_removed > 0)) {
    return {
      kind: "diff",
      added: status.base_added,
      removed: status.base_removed,
    };
  }
  if (status.ahead > 0 || status.behind > 0) {
    return { kind: "sync", ahead: status.ahead, behind: status.behind };
  }
  return { kind: "none" };
}
