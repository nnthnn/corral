export interface GitStatus {
  branch: string;
  ahead: number;
  behind: number;
  staged: number;
  modified: number;
  untracked: number;
  added: number;
  removed: number;
  conflicts: number;
  commits_ahead_of_base: number;
  merged_into_base: boolean;
  conflicts_with_base: boolean;
  has_remote: boolean;
  last_commit_message: string;
  last_commit_time: number;
  base_added: number;
  base_removed: number;
}

export interface Worktree {
  project: string;
  feature: string;
  /** Current rename slug — starts as `feature` and becomes the kebab slug
   * on rename. Drives the branch name and a slug symlink alias; distinct
   * from the human-readable `title`. */
  slug: string;
  branch: string;
  path: string;
  status: GitStatus | null;
  has_resumable_session: boolean;
  note: string;
  /** Human-readable name set via rename, e.g. "Fix Login Timeout". Empty
   * or absent means unset — display code should fall back to
   * `slug`. Optional (rather than required like the other fields)
   * so existing test fixtures that don't care about it don't all need
   * updating; real worktrees from the backend always include it. */
  title?: string;
  /** This worktree's GitHub PR state, if the backend's 5-minute poll has
   * fetched it (`pr_poll.rs`) — `undefined`/`null` means "not known yet,"
   * not "no PR exists." `deriveLifecycleState` in `utils.ts` falls back to
   * local git state in either case. */
  pr_state?: PrInfo | null;
  /** Coarse CI status for the open PR above, from the same background poll —
   * `undefined`/`null` means "no PR or not fetched yet". A summary with
   * `total === 0` means "PR open, no checks configured". Drives the
   * sidebar's CI indicator for rows other than the selected one; the
   * selected worktree's live `checksByWtId` takes precedence when present. */
  pr_checks?: ChecksSummary | null;
  /** Whether this worktree's project has a `setup_command` configured.
   * Optional so test fixtures needn't set it; the Setup tab treats a
   * missing/`false` value as "nothing configured to re-run". */
  has_setup_command?: boolean;
}

/** Aggregate CI status for a PR's check runs — see `ChecksSummary` in
 * `corral-core/src/checks.rs`. */
export interface ChecksSummary {
  total: number;
  pending: number;
  /** Terminal non-pass runs — failed plus cancelled. */
  failed: number;
}

export type SyncState = "uncommitted" | "committed" | "pushed";

export interface ChangedFile {
  path: string;
  status: string;
  /** The file's path before the rename/copy, if `status` is "renamed" or
   * "copied". `null` otherwise. */
  old_path: string | null;
  staged: boolean;
  sync_state: SyncState;
  /** True if the file has uncommitted working-tree changes (staged,
   * unstaged, or untracked). Orthogonal to `sync_state`: a file that's
   * already committed/pushed but edited again keeps its `sync_state` and
   * reports `dirty`, shown as an extra dot next to the sync icon. */
  dirty: boolean;
  added: number;
  removed: number;
}

export interface BranchChanges {
  base: string | null;
  files: ChangedFile[];
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export type FileContent =
  | { kind: "text"; text: string }
  /** A raster image the preview renders directly — `data` is base64 file
   * bytes, wrapped in a `data:<mime>;base64,` URL by `imageDataUrl`. SVG
   * arrives as `text` instead (see `isSvgPath`). */
  | { kind: "image"; mime: string; data: string }
  | { kind: "binary" }
  | { kind: "too_large"; size: number };

export type MergeResult =
  | { outcome: "merged"; pushed: boolean }
  | { outcome: "conflict" }
  | { outcome: "dirty" }
  | { outcome: "no_base_branch" }
  | { outcome: "base_diverged"; base: string }
  | { outcome: "merged_push_failed"; error: string }
  | { outcome: "not_on_base_branch"; current: string; base: string };

export type RebaseResult =
  | { outcome: "rebased" }
  | { outcome: "conflict" }
  | { outcome: "dirty" }
  | { outcome: "no_base_branch" };

export type PrState = "open" | "merged" | "closed";

export interface PrInfo {
  number: number;
  url: string;
  state: PrState;
  is_draft: boolean;
}

export type CheckBucket = "pass" | "fail" | "pending" | "skipping" | "cancel";

export interface CheckRun {
  name: string;
  workflow: string;
  bucket: CheckBucket;
  url: string;
}

export interface PrComment {
  author: string;
  body: string;
  created_at: string;
  url: string;
}

export interface PrDetails {
  title: string;
  body: string;
  /** GitHub login of the PR's author. */
  author: string;
  /** `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or empty when no
   * review is required. */
  review_decision: string;
  comments: PrComment[];
}

export type MergePrResult =
  | { outcome: "merged" }
  | { outcome: "rejected"; reason: string };

export type SoundChoice = "chime" | "whistle" | "holler" | "moo";

export interface Settings {
  editor_command: string;
  notifications_enabled: boolean;
  terminal_font_size: number;
  /** Diff viewer font size in pixels. Independently zoomable from the
   * terminal via Ctrl/Cmd +/-/0 while the diff tab is active. */
  diff_font_size: number;
  collapsed_groups: string[];
  /** User-customized sidebar project order (drag-to-reorder). Projects not
   * listed here fall back to alphabetical, appended after the ones listed. */
  project_order: string[];
  sidebar_collapsed: boolean;
  changes_collapsed: boolean;
  sidebar_width: number;
  changes_width: number;
  extra_instructions: string;
  skip_permissions: boolean;
  co_author_trailer: boolean;
  sound_enabled: boolean;
  /** Which synthesized sound to play — see `SOUND_CHOICES` in
   * `lib/notifications.ts`. */
  sound_choice: SoundChoice;
  terminal_font_family: string;
  scrollback_kb: number;
  delete_branch_default: boolean;
  caffeinate_enabled: boolean;
  worktrees_root: string;
  terminal_command: string;
  idle_timeout_enabled: boolean;
  idle_timeout_minutes: number;
  /** Path or command name used to spawn agent sessions. Empty = default
   * (`claude`, resolved via PATH). */
  claude_executable_path: string;
  /** Which agent harness new sessions launch. */
  harness: "claude" | "opencode" | "opencode2" | "pi";
  /** Path or command name used to spawn OpenCode v1 (legacy) sessions when
   * `harness === "opencode"`. Empty = default (`opencode`, resolved via
   * PATH) — but the current `opencode` is v2, so legacy users usually set
   * this explicitly. */
  opencode_executable_path: string;
  /** Path or command name used to spawn the OpenCode v2 agent server when
   * `harness === "opencode2"`. Empty = default (`opencode`, resolved via
   * PATH); the old `opencode2` alias is no longer shipped. */
  opencode2_executable_path: string;
  /** Path or command name used to spawn Pi sessions when `harness === "pi"`.
   * Empty = default (`pi`, resolved via PATH). */
  pi_executable_path: string;
}

export interface ProjectSettings {
  copy_globs: string[];
  setup_command: string;
  run_command: string;
  project_prompt: string;
  /** Per-project environment variables injected into every agent session,
   * one `KEY=VALUE` per entry. */
  env: string[];
}

export interface ProvisionState {
  output: number[];
  /** Bytes trimmed from the front of `output` once it hit its cap — see
   * `lib/output-buffer.ts`. Absent on backend snapshots (nothing was trimmed
   * there), which reads as 0. */
  dropped?: number;
  running: boolean;
  exit_code: number | null;
}

export interface RunState {
  output: number[];
  /** See `ProvisionState.dropped`. */
  dropped?: number;
  running: boolean;
  exit_code: number | null;
}

/** One clickable Run-tab target: a public `justfile` recipe or a
 * `package.json` script, listed by `list_run_targets`. */
export interface RunTarget {
  /** Chip text (`build`). */
  label: string;
  /** What ▶ actually runs (`just build`, `pnpm run build`). */
  command: string;
  /** Recipe doc comment, or the package script's body. Button tooltip. */
  doc: string | null;
  /** Parameters with no default — clicking this target prefills the command
   * instead of running it. Empty for package scripts. */
  required_params: string[];
}

/** The Run tab's target row: which source it came from (the row's prefix
 * label: `just`, `pnpm`, `yarn`, `npm`, `bun`) plus the targets themselves. */
export interface RunTargets {
  source: string | null;
  targets: RunTarget[];
}
