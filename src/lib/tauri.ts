import { invoke as rawInvoke } from "@tauri-apps/api/core";
import type { Attach, SessionInfo } from "../session";
import type {
  BranchChanges,
  CheckRun,
  FileContent,
  FileEntry,
  GitStatus,
  MergePrResult,
  MergeResult,
  PrDetails,
  PrInfo,
  ProjectSettings,
  ProvisionState,
  RebaseResult,
  RunState,
  Settings,
  Worktree,
} from "../types";

/**
 * One entry per `#[tauri::command]` in src-tauri/src/commands/ — keep in
 * sync with the Rust signatures there. Argument keys are camelCase to match
 * Tauri's default snake_case-to-camelCase conversion of Rust parameter names.
 */
interface CommandMap {
  list_worktrees: { args: undefined; result: Worktree[] };
  branch_changes: {
    args: { project: string; feature: string };
    result: BranchChanges;
  };
  file_diff: {
    args: { project: string; feature: string; path: string };
    result: string;
  };
  revert_file: {
    args: {
      project: string;
      feature: string;
      path: string;
      oldPath: string | null;
    };
    result: undefined;
  };
  list_worktree_dir: {
    args: { project: string; feature: string; path: string };
    result: FileEntry[];
  };
  read_worktree_file: {
    args: { project: string; feature: string; path: string };
    result: FileContent;
  };
  watch_worktree: {
    args: { project: string; feature: string };
    result: undefined;
  };
  refresh_worktree_status: {
    args: { project: string; feature: string };
    result: GitStatus | null;
  };
  create_worktree: {
    args: { project: string; feature: string | null };
    result: Worktree;
  };
  remove_worktree: {
    args: { project: string; feature: string; deleteBranch: boolean };
    result: undefined;
  };
  rename_worktree: {
    args: { project: string; feature: string; newName: string };
    result: Worktree;
  };
  set_worktree_note: {
    args: { project: string; feature: string; note: string };
    result: undefined;
  };
  merge_worktree: {
    args: { project: string; feature: string };
    result: MergeResult;
  };
  rebase_worktree: {
    args: { project: string; feature: string };
    result: RebaseResult;
  };
  pr_status: {
    args: { project: string; feature: string };
    result: PrInfo | null;
  };
  pr_checks: {
    args: { project: string; feature: string };
    result: CheckRun[];
  };
  pr_details: {
    args: { project: string; feature: string };
    result: PrDetails | null;
  };
  check_log: {
    args: { project: string; feature: string; checkUrl: string };
    result: string;
  };
  merge_pr: {
    args: { project: string; feature: string };
    result: MergePrResult;
  };
  mark_pr_ready: {
    args: { project: string; feature: string };
    result: undefined;
  };
  get_settings: { args: undefined; result: Settings };
  save_settings: { args: { settings: Settings }; result: undefined };
  get_project_settings: {
    args: { project: string };
    result: ProjectSettings;
  };
  save_project_settings: {
    args: { project: string; settings: ProjectSettings };
    result: undefined;
  };
  get_provision_output: {
    args: { key: string };
    result: ProvisionState | null;
  };
  resize_provision: {
    args: { key: string; rows: number; cols: number };
    result: undefined;
  };
  rerun_provision: {
    args: { project: string; feature: string };
    result: undefined;
  };
  stop_provision: {
    args: { key: string };
    result: undefined;
  };
  start_run: {
    args: { project: string; feature: string; command: string };
    result: undefined;
  };
  stop_run: {
    args: { key: string };
    result: undefined;
  };
  get_run_output: {
    args: { key: string };
    result: RunState | null;
  };
  resize_run: {
    args: { key: string; rows: number; cols: number };
    result: undefined;
  };
  resolve_run_command: {
    args: { project: string; feature: string };
    result: string;
  };
  list_sessions: { args: undefined; result: SessionInfo[] };
  start_session: {
    args: { project: string; feature: string; fresh?: boolean };
    result: SessionInfo;
  };
  attach_session: { args: { id: string }; result: Attach };
  write_session: { args: { id: string; data: string }; result: undefined };
  resize_session: {
    args: { id: string; rows: number; cols: number };
    result: undefined;
  };
  kill_session: { args: { id: string }; result: undefined };
  close_session: { args: { id: string }; result: undefined };
  notify_icon: { args: undefined; result: string | null };
  send_notification: {
    args: { title: string; body: string; icon: string | null };
    result: undefined;
  };
  home_dir: { args: undefined; result: string | null };
  memory_usage: { args: undefined; result: number | null };
  save_pasted_image: { args: { bytes: number[] }; result: string };
  save_text_attachment: { args: { text: string }; result: string };
  open_in_zed: {
    args: { path: string; editorCommand: string };
    result: undefined;
  };
  open_terminal: {
    args: { path: string; terminalCommand: string };
    result: undefined;
  };
  list_projects: { args: undefined; result: string[] };
  project_icon: { args: { project: string }; result: string | null };
  add_project: { args: { path: string }; result: string };
  remove_project: { args: { project: string }; result: undefined };
}

type Args<K extends keyof CommandMap> = CommandMap[K]["args"];

/** Typed wrapper over Tauri's `invoke` — the command name and argument shape
 * are checked against `CommandMap`, so a typo'd command or a mismatched arg
 * only shows up as a compiler error instead of a runtime "not found". */
export function invoke<K extends keyof CommandMap>(
  cmd: K,
  ...args: Args<K> extends undefined ? [] : [Args<K>]
): Promise<CommandMap[K]["result"]> {
  return rawInvoke<CommandMap[K]["result"]>(
    cmd,
    args[0] as Record<string, unknown> | undefined,
  );
}
