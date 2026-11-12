import { create } from "zustand";
import type { SessionInfo, SessionManager } from "./session";
import type {
  BranchChanges,
  CheckRun,
  FileContent,
  FileEntry,
  PrDetails,
  PrInfo,
  ProvisionState,
  RunState,
  Settings,
  Worktree,
} from "./types";
import { matchesWorktreeFilter, orderedWorktreeIds, wtId } from "./utils";

export const DEFAULT_SETTINGS: Settings = {
  editor_command: "zeditor",
  notifications_enabled: true,
  terminal_font_size: 13,
  diff_font_size: 16,
  collapsed_groups: [],
  project_order: [],
  sidebar_collapsed: false,
  changes_collapsed: false,
  sidebar_width: 320,
  changes_width: 384,
  extra_instructions: "",
  skip_permissions: false,
  co_author_trailer: false,
  sound_enabled: false,
  sound_choice: "chime",
  terminal_font_family: "",
  scrollback_kb: 256,
  delete_branch_default: true,
  caffeinate_enabled: true,
  worktrees_root: "",
  terminal_command: "ghostty",
  idle_timeout_enabled: false,
  idle_timeout_minutes: 30,
  claude_executable_path: "",
  harness: "claude",
  opencode_executable_path: "",
  opencode2_executable_path: "",
  pi_executable_path: "",
};

export type TabId = "agent" | "run" | "setup" | "pr" | "diff" | "file";

export interface Toast {
  id: string;
  message: string;
  variant?: "error" | "success";
}

export interface PrPreparation {
  token: number;
  kind: "create" | "update";
  initialAhead?: number;
  waits?: number;
}

export interface AppState {
  worktrees: Worktree[];
  /** All known projects (git repos under `~/source`), alphabetically —
   * independent of `worktrees` so a project with no active worktrees still
   * gets a sidebar row, and only disappears once its repo checkout is gone.
   * Populated by `loadAll()`. */
  projects: string[];
  sessions: Map<string, SessionInfo>;
  /** Sessions already pinged for the current waiting period; cleared when
   * you type into them (a new turn) so we notify at most once per turn. */
  notifiedWaiting: Set<string>;
  selectedId: string | null;
  notifyIcon: string | undefined;
  /** Fetched once at startup; used to shorten displayed paths to `~/...`. */
  homeDir: string | undefined;
  settings: Settings;
  collapsedGroups: Set<string>;
  /** Auto-detected project icon (a `data:` URI) or `null` if none was found —
   * absent from the map entirely means "not fetched yet". Populated by
   * `loadAll()`'s `ensureProjectIcons`. */
  projectIcons: Map<string, string | null>;
  activeTab: TabId;
  diffTab: { path: string } | null;
  /** Loaded diff text (or a formatted error string) for the active diffTab. */
  diffContent: string | null;
  /** Set instead of `diffContent` when `file_diff` fails. */
  diffError: string | null;
  /** Which mode the right-hand panel (`ChangesPane`) is showing — the git
   * Changes list, or the Files tree. Independent of `activeTab`, which
   * governs the center-pane tab strip. */
  changesPanelTab: "changes" | "files";
  /** Cached directory listings for the Files tree, keyed
   * `<wtId>:<relativePath>` (empty path = worktree root) — populated by
   * `loadDir` in `lib/files.ts`. Absent means "not fetched yet"; present
   * (even if empty) means "fetched, don't refetch on re-expand". */
  fileTree: Map<string, FileEntry[]>;
  /** Directories currently expanded in the Files tree, keyed the same way
   * as `fileTree`. Session-only, not persisted. */
  expandedDirs: Set<string>;
  /** Mirrors `diffTab` exactly, for the Files tree's click-to-preview tab. */
  fileTab: { path: string } | null;
  /** Loaded file text (or a formatted error string) for the active
   * fileTab — mirrors `diffContent`. */
  fileContent: FileContent | null;
  /** Set instead of `fileContent` when `read_worktree_file` fails — mirrors
   * `diffError`. */
  fileError: string | null;
  /** Settings is an app-level full-window overlay, independent of the
   * worktree-scoped Agent/Diff tab strip — not itself a tab. */
  settingsOpen: boolean;
  /** Whether the command palette (Ctrl/Cmd+K) is open — a full-window
   * overlay for type-ahead worktree switching and a few global actions. */
  paletteOpen: boolean;
  /** `true` when the compositor/WM did not grant real window decorations
   * (e.g. Hyprland) — set once at startup by `bootstrap()`. Drives whether
   * `WindowControls` renders its own minimize/maximize/close buttons. */
  needsCustomChrome: boolean;
  filterText: string;
  attentionOnly: boolean;
  /** Worktree ids that produced output while backgrounded, not yet viewed —
   * distinct from the fading 🔔 attention cue, which only lights up once a
   * backgrounded session settles into "waiting". */
  unreadOutput: Set<string>;
  /** Worktree ids that were successfully merged into base this session —
   * an ephemeral client-side bridge state so `MergeButton` can show
   * "Remove" right after a merge, before the backend's `merged_into_base`
   * status signal can distinguish "just merged" from "never diverged".
   * Cleaned up whenever the worktree is actually removed. */
  justMergedIds: Set<string>;
  /** Worktree ids with a merge currently in flight — guards against
   * double-clicking "Merge to main" while the first request is pending. */
  mergingIds: Set<string>;
  /** Worktree ids currently inside `start_session`'s invoke — mirrors
   * `mergingIds`. Claude sessions resolve near-instantly so this rarely
   * renders, but an opencode session spawns a second headless process and
   * blocks on its HTTP readiness (can take several seconds), so `Detail`
   * shows a loading state instead of a dead-looking button click. */
  startingIds: Set<string>;
  /** Worktree ids with a rebase currently in flight — mirrors `mergingIds`,
   * guarding against double-clicking "Rebase" while the first request is
   * pending. */
  rebasingIds: Set<string>;
  /** Worktree ids currently playing their sidebar-row removal animation —
   * see `removeWorktree` in `lib/actions.ts`. The row stays in `worktrees`
   * (and hence stays mounted) until the animation window elapses. */
  removingIds: Set<string>;
  /** Set briefly by `removeWorktree` when it had to auto-reselect after
   * removing the currently-selected worktree — `MainPane` reads this to
   * play a short opacity cue instead of jump-cutting to the new selection,
   * then clears it back to `false` once the cue has played. */
  reselectFlash: boolean;
  /** Set by `focusFirstProjectCreate` (the homepage's "New worktree" card)
   * to the first project in sidebar order — `WorktreeTree` reads this to
   * scroll to and briefly flash its own per-project `+` button, then
   * clears it back to `null` once the flash has played. `null` means no
   * nudge is pending. */
  worktreeNudgeTarget: string | null;
  /** On-demand PR state for a worktree, keyed like `wtId`. Only populated
   * for worktrees whose `status.has_remote` is true, and only when that
   * worktree is selected (see `selectWorktree` in `lib/actions.ts`) — this
   * is a network call, unlike the rest of `GitStatus`, so it isn't
   * background-polled for every worktree in the sidebar. Absent from the
   * map means "not fetched yet"; `null` means "fetched, no PR exists yet". */
  prInfoByWtId: Map<string, PrInfo | null>;
  /** Active agent-driven PR tasks, keyed by worktree id. The token protects
   * newer tasks from stale async cleanup; kind and progress fields determine
   * whether a wait means clarification or actual completion. */
  preparingPr: Map<string, PrPreparation>;
  /** Live CI check-run status for a worktree's open PR, keyed like `wtId`.
   * Populated by `fetchPrChecks` in `lib/checks.ts`; polled while any check
   * is pending by `usePrChecksPolling` (only for the active/focused
   * workspace — see that hook's doc comment). Absent means "not fetched
   * yet"; an empty array means "no checks configured". */
  checksByWtId: Map<string, CheckRun[]>;
  /** PR title/body/comments for the Checks tab, keyed like `wtId`. Only
   * fetched when the PR tab is actually opened (see `PrPane`). */
  prDetailsByWtId: Map<string, PrDetails | null>;
  /** Worktree ids with a `merge_pr` call currently in flight — mirrors
   * `mergingIds`, guarding against double-clicks. */
  mergingPrIds: Set<string>;
  /** Worktree ids with a `mark_pr_ready` call currently in flight — same
   * double-click guard as `mergingPrIds`. */
  markingPrReadyIds: Set<string>;
  /** Set once by TerminalHost's mount effect; read imperatively (via
   * `useStore.getState().sessionMgr`) by lib/agent-actions.ts and
   * lib/settings.ts, which run outside any component's render. */
  sessionMgr: SessionManager | null;
  /** Bumped by `SessionManager` every time it creates or disposes a
   * terminal entry. `sessionMgr.has(id)` itself isn't reactive — it reads a
   * plain `Map` owned by a class instance, not store state — so a component
   * calling it directly (`AgentPane`'s `hasTerminal`) would only ever see a
   * stale answer until something *else* happened to trigger a re-render.
   * For Claude that "something else" is usually near-immediate (the
   * injected onboarding prompt kicks off hook events quickly); a freshly
   * created opencode session sends no message and can sit with no SSE event
   * at all, so nothing ever forced a re-render and the loading `Detail`
   * overlay stuck around forever, reading as "no xterm shows up." Depending
   * on this counter (its value is never read) forces the recompute right
   * after the entry actually exists, regardless of what else fires. */
  terminalsVersion: number;
  changes: BranchChanges | null;
  loadError: string | null;
  deleteTarget: Worktree | null;
  renameTarget: Worktree | null;
  /** Project name currently shown in `ProjectSettingsDialog`, or `null` if
   * closed. */
  projectSettingsTarget: string | null;
  /** Project name currently shown in `RemoveProjectDialog`, or `null` if
   * closed. */
  removeProjectTarget: string | null;
  /** Post-create setup-command output, keyed like `wtId` (`<project>/<feature>`)
   * — populated by `provisioning-events.ts`'s listeners and by
   * `ProvisionPanel`'s reattach fetch. */
  provisioning: Map<string, ProvisionState>;
  /** Run-tab dev-server output, keyed like `wtId` (`<project>/<feature>`) —
   * populated by `run-events.ts`'s listeners and by `RunPanel`'s reattach
   * fetch. Mirrors `provisioning` exactly; kept separate because Setup and
   * the Run tab are independent processes that can both be live for the
   * same worktree at once. */
  run: Map<string, RunState>;
  /** Non-blocking error toasts, shown by `ToastHost` and pushed via
   * `lib/toasts.ts`'s `showError`. */
  toasts: Toast[];
  /** Worktree ids whose current waiting period has already been viewed —
   * keeps the fading 🔔 attention cue cleared after you've selected a
   * waiting session, even once you navigate away again. Cleared whenever
   * the session starts working again, so the next waiting period re-alerts. */
  acknowledgedWaiting: Set<string>;
}

export const useStore = create<AppState>(() => ({
  worktrees: [],
  projects: [],
  sessions: new Map(),
  notifiedWaiting: new Set(),
  selectedId: null,
  notifyIcon: undefined,
  homeDir: undefined,
  settings: { ...DEFAULT_SETTINGS },
  collapsedGroups: new Set(),
  projectIcons: new Map(),
  activeTab: "agent",
  diffTab: null,
  diffContent: null,
  diffError: null,
  changesPanelTab: "changes",
  fileTree: new Map(),
  expandedDirs: new Set(),
  fileTab: null,
  fileContent: null,
  fileError: null,
  settingsOpen: false,
  paletteOpen: false,
  needsCustomChrome: false,
  filterText: "",
  attentionOnly: false,
  unreadOutput: new Set(),
  justMergedIds: new Set(),
  mergingIds: new Set(),
  startingIds: new Set(),
  rebasingIds: new Set(),
  removingIds: new Set(),
  reselectFlash: false,
  worktreeNudgeTarget: null,
  prInfoByWtId: new Map(),
  preparingPr: new Map(),
  checksByWtId: new Map(),
  prDetailsByWtId: new Map(),
  mergingPrIds: new Set(),
  markingPrReadyIds: new Set(),
  sessionMgr: null,
  terminalsVersion: 0,
  changes: null,
  loadError: null,
  deleteTarget: null,
  renameTarget: null,
  projectSettingsTarget: null,
  removeProjectTarget: null,
  provisioning: new Map(),
  run: new Map(),
  toasts: [],
  acknowledgedWaiting: new Set(),
}));

export function currentWt(s: AppState): Worktree | undefined {
  return s.worktrees.find((w) => wtId(w) === s.selectedId);
}

/** The center-pane tab strip for the selected worktree, in left-to-right
 * order: Agent, Run, and Setup are always present once a worktree is
 * selected; PR, Diff, and File appear only when there's something to show.
 * Single source of truth for tab membership + order — `TabBar` renders it
 * (deriving labels/closable per id) and the tab-cycling shortcut walks it,
 * so the two surfaces can't drift apart.
 *
 * Setup is deliberately unconditional (rather than gated on a `provisioning`
 * entry, as the old docked panel was): provisioning output is retained only
 * in memory, so after an app restart a worktree whose setup already ran had
 * no entry and lost its panel entirely. An always-present tab keeps the
 * re-run affordance reachable regardless. */
export function worktreeTabIds(s: AppState): TabId[] {
  const wt = currentWt(s);
  if (!wt) return [];
  const ids: TabId[] = ["agent", "run", "setup"];
  if (s.prInfoByWtId.get(wtId(wt))) ids.push("pr");
  if (s.diffTab) ids.push("diff");
  if (s.fileTab) ids.push("file");
  return ids;
}

/** Derive a session's display status, or null if there's no session. */
export function sessionStatus(
  s: AppState,
  id: string,
): { cls: string; label: string } | null {
  const sess = s.sessions.get(id);
  if (!sess) return null;
  if (!sess.running) {
    if (sess.killed) return { cls: "stopped", label: "stopped" };
    if (sess.exit_code === 0) return { cls: "done", label: "done" };
    return {
      cls: "stopped",
      label: sess.exit_code != null ? `exited (${sess.exit_code})` : "exited",
    };
  }
  if (!sess.waiting) return { cls: "working", label: "working" };
  return { cls: "waiting", label: "waiting at the gate" };
}

/** Ids of the worktree rows the sidebar is currently showing, in the exact
 * order it renders them (custom project order, then the text / attention
 * filter). Single source of truth for `Ctrl+1–9`, `Ctrl+[`/`]`, and the
 * hold-Ctrl shortcut hint. */
export function visibleWorktreeIds(s: AppState): string[] {
  return orderedWorktreeIds(
    s.worktrees,
    s.projects,
    s.settings.project_order,
    (wt) =>
      matchesWorktreeFilter(wt, s.filterText) &&
      (!s.attentionOnly || wantsAttention(s, wtId(wt))),
  );
}

/** A backgrounded, waiting agent wants your attention — shared by the fading
 * 🔔 cue and the "show only waiting worktrees" triage toggle. */
export function wantsAttention(s: AppState, id: string): boolean {
  return (
    sessionStatus(s, id)?.cls === "waiting" &&
    id !== s.selectedId &&
    !s.acknowledgedWaiting.has(id)
  );
}

/** Count of sessions currently running, for the sidebar header. */
export function runningSessionCount(s: AppState): number {
  let count = 0;
  for (const sess of s.sessions.values()) {
    if (sess.running) count++;
  }
  return count;
}
