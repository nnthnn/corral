/// Short git sha for diagnostics (`copy_diagnostics`, mirroring `corral
/// --version`). Falls back to `"unknown"` outside a git checkout or when
/// `git` isn't on `PATH`, so a source-tarball build still succeeds.
fn git_sha() -> String {
    use std::process::Command;
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn main() {
    println!("cargo:rustc-env=CORRAL_GIT_SHA={}", git_sha());
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "notify_icon",
            "send_notification",
            "home_dir",
            "platform_defaults",
            "memory_usage",
            "list_monospace_fonts",
            "copy_diagnostics",
            "list_projects",
            "project_icon",
            "add_project",
            "create_project",
            "remove_project",
            "list_worktrees",
            "set_worktree_note",
            "branch_changes",
            "file_diff",
            "revert_file",
            "list_worktree_dir",
            "read_worktree_file",
            "create_worktree",
            "remove_worktree",
            "rename_worktree",
            "merge_worktree",
            "rebase_worktree",
            "pr_status",
            "pr_checks",
            "pr_details",
            "check_log",
            "merge_pr",
            "mark_pr_ready",
            "watch_worktree",
            "refresh_worktree_status",
            "save_pasted_image",
            "save_text_attachment",
            "open_in_zed",
            "open_terminal",
            "get_settings",
            "save_settings",
            "get_project_settings",
            "save_project_settings",
            "list_sessions",
            "start_session",
            "attach_session",
            "write_session",
            "resize_session",
            "kill_session",
            "close_session",
            "get_provision_output",
            "rerun_provision",
            "stop_provision",
            "resize_provision",
            "start_run",
            "stop_run",
            "get_run_output",
            "resize_run",
            "resolve_run_command",
            "list_run_targets",
            "browser_ui_ready",
            "browser_ui_bounds",
            "browser_tab_new",
            "browser_tab_focus",
            "browser_tab_close",
            "browser_navigate",
            "browser_go_back",
            "browser_go_forward",
            "browser_reload",
            "browser_refresh_titles",
        ]),
    ))
    .expect("failed to run tauri-build");
}
