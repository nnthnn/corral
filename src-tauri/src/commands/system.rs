//! Misc host-integration commands: notification icon, clipboard-paste temp
//! files, and launching an external editor.

use std::path::Path;
use std::time::{Duration, SystemTime};

use corral_core::log_err::LogErr;

use super::StrErr;

/// How long a pasted-image or text-attachment temp file is kept before it's
/// considered stale and swept up. Generous, since these only need to outlive
/// the terminal drag/paste (or "Add to chat") action that references the
/// path.
const TEMP_FILE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Absolute path to the app icon for use as a notification icon. The icon is
/// embedded at compile time and written once to the cache dir, so the path is
/// valid in both `tauri dev` and bundled builds.
#[tauri::command]
pub fn notify_icon(app: tauri::AppHandle) -> Option<String> {
    use tauri::Manager;
    const ICON: &[u8] = include_bytes!("../../icons/128x128.png");
    let dir = app.path().app_cache_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("corral-notify.png");
    if !path.exists() {
        std::fs::write(&path, ICON).ok()?;
    }
    Some(path.to_string_lossy().into_owned())
}

/// Sends an OS notification whose default (body-click) action focuses the
/// main window. Replaces the frontend's non-actionable `sendNotification`
/// path — see `src/lib/notifications.ts::notify`.
#[tauri::command]
pub fn send_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
    icon: Option<String>,
) -> Result<(), String> {
    let mut notification = notify_rust::Notification::new();
    notification.summary(&title);
    notification.body(&body);
    if let Some(icon) = icon.as_deref() {
        notification.icon(icon);
    }
    // "default" is the action the notification daemon fires when the body is
    // clicked (e.g. mako's `on-button-left=invoke-default-action`).
    notification.action("default", "Focus Corral");

    let handle = notification
        .show()
        .map_err(|e| format!("failed to show notification: {e}"))?;

    let app = app.clone();
    std::thread::spawn(move || {
        handle.wait_for_action(|action| {
            if action == "default" {
                focus_main_window(&app);
            }
        });
    });

    Ok(())
}

/// Raise and focus the main window (best-effort; a Wayland compositor may
/// ignore a focus request that isn't backed by an xdg-activation token).
fn focus_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Writes a pasted clipboard image to a uniquely-named temp file and returns
/// its path, so it can be inserted into a terminal as a real file path (the
/// same way an OS-level file drop is handled) instead of trying to push raw
/// image bytes through a terminal keystroke.
#[tauri::command]
pub fn save_pasted_image(bytes: Vec<u8>) -> Result<String, String> {
    cleanup_stale_temp_files_in(
        &std::env::temp_dir(),
        "corral-paste-",
        ".png",
        SystemTime::now(),
        TEMP_FILE_MAX_AGE,
    );

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .str_err()?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("corral-paste-{nanos}.png"));
    std::fs::write(&path, &bytes).str_err()?;
    Ok(path.to_string_lossy().into_owned())
}

/// Writes text (a failed check's job log, PR comments) to a uniquely-named
/// temp file and returns its path, so "Add to chat" can hand it to the agent
/// as a referenced file — same reasoning as `save_pasted_image` — instead of
/// pasting the whole thing inline and flooding the terminal with text.
#[tauri::command]
pub fn save_text_attachment(text: String) -> Result<String, String> {
    cleanup_stale_temp_files_in(
        &std::env::temp_dir(),
        "corral-attach-",
        ".txt",
        SystemTime::now(),
        TEMP_FILE_MAX_AGE,
    );

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .str_err()?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("corral-attach-{nanos}.txt"));
    std::fs::write(&path, text.as_bytes()).str_err()?;
    Ok(path.to_string_lossy().into_owned())
}

/// Remove temp files in `dir` matching `prefix`/`suffix` last modified more
/// than `max_age` before `now`. Best-effort: an unreadable directory or a
/// file that fails to remove (e.g. still open elsewhere) just logs and moves
/// on, since this piggybacks on every paste/attach rather than being the
/// source of truth for temp-file hygiene.
fn cleanup_stale_temp_files_in(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    now: SystemTime,
    max_age: Duration,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let matches = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.starts_with(prefix) && name.ends_with(suffix));
        if !matches {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age > max_age {
            std::fs::remove_file(&path)
                .log_warn(&format!("failed to remove stale temp file {path:?}"));
        }
    }
}

/// The current user's home directory, so the frontend can shorten absolute
/// paths to `~/...` for display without ever sending a real path through a
/// command that expects one (e.g. `open_in_zed`).
#[tauri::command]
pub fn home_dir() -> Option<String> {
    corral_core::config::home_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Total resident memory (RSS) of this process and all its descendants, in
/// bytes — `None` on platforms without `/proc` (non-Linux). Polled by the
/// sidebar header to answer "is this eating my RAM" across parallel agents.
#[tauri::command]
pub fn memory_usage() -> Option<u64> {
    corral_core::process::total_rss_bytes()
}

#[tauri::command]
pub fn open_in_zed(path: String, editor_command: String) -> Result<(), String> {
    std::process::Command::new(&editor_command)
        .arg("-n")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("failed to launch {editor_command}: {e}"))?;
    Ok(())
}

/// Kitty's CLI flag for the startup directory is `--directory`/`-d`, not
/// `--working-directory` (Ghostty/Alacritty). Matched on the command's file
/// name so a full path (e.g. `/usr/bin/kitty`) still resolves correctly.
fn terminal_args(terminal_command: &str, path: &str) -> Vec<String> {
    let program_name = std::path::Path::new(terminal_command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(terminal_command);
    match program_name {
        "kitty" => vec!["--directory".to_string(), path.to_string()],
        _ => vec![format!("--working-directory={path}")],
    }
}

#[tauri::command]
pub fn open_terminal(path: String, terminal_command: String) -> Result<(), String> {
    std::process::Command::new(&terminal_command)
        .args(terminal_args(&terminal_command, &path))
        .spawn()
        .map_err(|e| format!("failed to launch {terminal_command}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cleanup_stale_temp_files_in, save_pasted_image, save_text_attachment, terminal_args,
    };
    use std::time::{Duration, SystemTime};

    #[test]
    fn terminal_args_uses_combined_working_directory_flag_for_ghostty_and_alacritty() {
        assert_eq!(
            terminal_args("ghostty", "/some/path"),
            vec!["--working-directory=/some/path".to_string()]
        );
        assert_eq!(
            terminal_args("alacritty", "/some/path"),
            vec!["--working-directory=/some/path".to_string()]
        );
    }

    #[test]
    fn terminal_args_uses_directory_flag_for_kitty() {
        assert_eq!(
            terminal_args("kitty", "/some/path"),
            vec!["--directory".to_string(), "/some/path".to_string()]
        );
    }

    #[test]
    fn terminal_args_matches_on_binary_name_not_full_path() {
        assert_eq!(
            terminal_args("/usr/bin/kitty", "/some/path"),
            vec!["--directory".to_string(), "/some/path".to_string()]
        );
    }

    #[test]
    fn cleanup_removes_matching_files_older_than_max_age() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("corral-paste-123.png");
        std::fs::write(&path, b"fake png").unwrap();

        cleanup_stale_temp_files_in(
            dir.path(),
            "corral-paste-",
            ".png",
            SystemTime::now(),
            Duration::ZERO,
        );

        assert!(!path.exists(), "stale file should be removed");
    }

    #[test]
    fn cleanup_keeps_matching_files_within_max_age() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("corral-paste-123.png");
        std::fs::write(&path, b"fake png").unwrap();

        cleanup_stale_temp_files_in(
            dir.path(),
            "corral-paste-",
            ".png",
            SystemTime::now(),
            Duration::from_secs(3600),
        );

        assert!(path.exists(), "fresh file should be kept");
    }

    #[test]
    fn cleanup_ignores_files_that_dont_match_prefix_or_suffix() {
        let dir = tempfile::TempDir::new().unwrap();
        let unrelated = dir.path().join("notes.txt");
        std::fs::write(&unrelated, b"hello").unwrap();

        cleanup_stale_temp_files_in(
            dir.path(),
            "corral-paste-",
            ".png",
            SystemTime::now(),
            Duration::ZERO,
        );

        assert!(unrelated.exists(), "non-matching files must be left alone");
    }

    #[test]
    fn save_pasted_image_writes_bytes_to_a_unique_temp_path() {
        let bytes = vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4];
        let path1 = save_pasted_image(bytes.clone()).unwrap();
        let path2 = save_pasted_image(bytes.clone()).unwrap();

        assert_ne!(path1, path2, "each paste should get a unique filename");
        assert_eq!(std::fs::read(&path1).unwrap(), bytes);
        assert_eq!(std::fs::read(&path2).unwrap(), bytes);

        std::fs::remove_file(&path1).unwrap();
        std::fs::remove_file(&path2).unwrap();
    }

    #[test]
    fn save_text_attachment_writes_text_to_a_unique_temp_path() {
        let path1 = save_text_attachment("log line 1".to_string()).unwrap();
        let path2 = save_text_attachment("log line 2".to_string()).unwrap();

        assert_ne!(path1, path2, "each attach should get a unique filename");
        assert_eq!(std::fs::read_to_string(&path1).unwrap(), "log line 1");
        assert_eq!(std::fs::read_to_string(&path2).unwrap(), "log line 2");

        std::fs::remove_file(&path1).unwrap();
        std::fs::remove_file(&path2).unwrap();
    }
}
