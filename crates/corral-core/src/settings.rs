//! User-configurable app preferences, persisted to disk so they aren't tied
//! to a single webview's localStorage.
//!
//! Stored at `~/.config/corral/settings.json` (XDG config dir via
//! `crate::paths::config_root`) — a sibling convention to `agent_session`'s
//! `~/.local/share/corral/sessions.json` (XDG data dir): settings are
//! user-editable config, sessions are app-generated state. The config dir can
//! be redirected with `CORRAL_CONFIG_DIR` (see `crate::paths`) so a dev
//! instance can test a new setting without touching the live app's file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::json_store;
use crate::paths;

fn settings_path() -> Result<PathBuf> {
    Ok(settings_path_in(&paths::config_root()?))
}

/// The settings file path under an explicit corral config root, so the join
/// is testable without touching `$XDG_CONFIG_HOME`/`$CORRAL_CONFIG_DIR`.
fn settings_path_in(config_root: &std::path::Path) -> PathBuf {
    config_root.join("settings.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub editor_command: String,
    pub notifications_enabled: bool,
    pub terminal_font_size: u16,
    /// Diff viewer font size in pixels. Independently adjustable from
    /// `terminal_font_size` via Ctrl/Cmd +/-/0 while the diff tab is active
    /// (see `useGlobalShortcuts.ts`).
    pub diff_font_size: u16,
    pub collapsed_groups: Vec<String>,
    /// User-customized sidebar project order (drag-to-reorder). Projects not
    /// listed here — new ones, or from a settings.json written before this
    /// field existed — fall back to alphabetical, appended after the ones
    /// listed here. Self-healing: the frontend always persists the full
    /// current project set on reorder, so stale entries for removed
    /// projects harmlessly drop out next time they're written.
    pub project_order: Vec<String>,
    pub sidebar_collapsed: bool,
    pub changes_collapsed: bool,
    /// Sidebar width in pixels, adjustable via a drag handle. Clamped to
    /// 200-500 in the frontend before it ever reaches here.
    pub sidebar_width: u32,
    /// Changes pane width in pixels, adjustable via a drag handle. Clamped
    /// to 240-600 in the frontend before it ever reaches here.
    pub changes_width: u32,
    /// Free-text instructions appended to every fresh session's system
    /// prompt (see `onboarding::system_prompt`), alongside the app's own
    /// briefing about running inside Corral.
    pub extra_instructions: String,
    /// Launch every new session with permission prompts bypassed — Claude via
    /// `--dangerously-skip-permissions`, OpenCode v2 via `--auto` (OpenCode
    /// v1 legacy has no equivalent flag, so it's unaffected).
    pub skip_permissions: bool,
    /// Instruct fresh sessions to add a `Co-Authored-By: Corral` trailer to
    /// commits they create.
    pub co_author_trailer: bool,
    /// Play a synthesized sound when a backgrounded agent starts waiting,
    /// independent of the OS notification popup.
    pub sound_enabled: bool,
    /// Which synthesized sound to play — see `SOUND_OPTIONS` in
    /// `lib/notifications.ts`. Free-form string (not a Rust enum) since this
    /// field is purely a frontend concern; the backend just stores it.
    pub sound_choice: String,
    /// Terminal font family; empty string means "use the built-in monospace
    /// fallback" (see `session.ts`'s `FONT` constant).
    pub terminal_font_family: String,
    /// Per-session scrollback buffer cap, in KB.
    pub scrollback_kb: u32,
    /// Default state of the DeleteDialog's "Also delete branch" checkbox.
    pub delete_branch_default: bool,
    /// Inhibit system sleep/idle (via `systemd-inhibit`) while at least one
    /// agent session is actively working (running, not waiting on the user).
    pub caffeinate_enabled: bool,
    /// Root directory under which new worktrees are created. Empty string
    /// means "use the default" (`~/corral`) — see `config::worktrees_root`.
    /// May contain a leading `~` for the home directory.
    pub worktrees_root: String,
    /// Command used by the "Shell" button to open an external terminal at a
    /// worktree's path (e.g. `ghostty`, `alacritty`, `kitty`).
    pub terminal_command: String,
    /// Auto-stop a session once it's been continuously "waiting" for
    /// `idle_timeout_minutes` — a cost/resource guard for unattended
    /// parallel agents. Off by default: this ends a real process, so it
    /// shouldn't surprise anyone who hasn't opted in.
    pub idle_timeout_enabled: bool,
    /// How long a session may sit "waiting" before it's auto-stopped, once
    /// `idle_timeout_enabled` is on.
    pub idle_timeout_minutes: u32,
    /// Path or command name used to spawn agent sessions. Empty string means
    /// "use the default" (`claude`, resolved via `PATH`) — see
    /// `commands/session.rs::start_session`. Lets a custom build, version
    /// pin, or non-PATH install be used instead.
    pub claude_executable_path: String,
    /// Which AI coding agent CLI to spawn for new sessions. App-wide rather
    /// than per-worktree, since the harness drives session spawn/status
    /// wiring that is simpler to keep global for now.
    pub harness: crate::harness::Harness,
    /// Path or command name used to spawn OpenCode v1 (legacy) sessions when
    /// `harness == Opencode`. Empty string means "use the default"
    /// (`opencode`, resolved via `PATH`) — mirrors `claude_executable_path`.
    /// Note the current `opencode` binary is v2, so legacy users generally
    /// need to set this explicitly to a v1 install.
    pub opencode_executable_path: String,
    /// Path or command name used to spawn OpenCode v2 sessions when
    /// `harness == Opencode2`. Empty string means "use the default"
    /// (`opencode2`, resolved via `PATH`; the released `@opencode/cli` ships
    /// both the `opencode` and `opencode2` binaries).
    pub opencode2_executable_path: String,
    /// Path or command name used to spawn Pi sessions when `harness == Pi`.
    /// Empty string means "use the default" (`pi`, resolved via `PATH`).
    pub pi_executable_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            editor_command: "zeditor".into(),
            notifications_enabled: true,
            terminal_font_size: 13,
            diff_font_size: 16,
            collapsed_groups: Vec::new(),
            project_order: Vec::new(),
            sidebar_collapsed: false,
            changes_collapsed: false,
            sidebar_width: 320,
            changes_width: 384,
            extra_instructions: String::new(),
            skip_permissions: false,
            co_author_trailer: false,
            sound_enabled: false,
            sound_choice: "chime".into(),
            terminal_font_family: String::new(),
            scrollback_kb: 256,
            delete_branch_default: true,
            caffeinate_enabled: true,
            worktrees_root: String::new(),
            terminal_command: "ghostty".into(),
            idle_timeout_enabled: false,
            idle_timeout_minutes: 30,
            claude_executable_path: String::new(),
            harness: crate::harness::Harness::default(),
            opencode_executable_path: String::new(),
            opencode2_executable_path: String::new(),
            pi_executable_path: String::new(),
        }
    }
}

pub fn load() -> Result<Settings> {
    load_under(&settings_path()?)
}

pub fn save(settings: &Settings) -> Result<()> {
    save_under(&settings_path()?, settings)
}

fn load_under(path: &std::path::Path) -> Result<Settings> {
    json_store::load_or_default(path, "settings")
}

fn save_under(path: &std::path::Path, settings: &Settings) -> Result<()> {
    // Route through the same store lock every other store uses, so a save
    // can't interleave with a concurrent save (the GUI and CLI are separate
    // processes, and multiple corral windows can run at once). It's a
    // whole-struct replace rather than a read-modify-write, so it's lower
    // risk than the keyed stores, but the symmetry keeps the discipline
    // uniform.
    //
    // Last-write-wins across windows is accepted as intended UX: each window
    // holds a full `Settings` loaded once at startup and every save writes
    // the whole blob, so a save from one window overwrites any change another
    // window made since it loaded. This lock only serializes the write (and
    // the `backup_existing` snapshot below); it does not merge field-level
    // edits. Fixing that would mean patch-based saves (send only changed
    // fields), which was deliberately declined — settings is a single blob,
    // every change is already 500ms-debounced in the frontend, and two
    // windows editing settings at the same moment is rare enough that a
    // last-writer-wins resolution is fine.
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    json_store::with_locked_path(dir, &file_name, |path| {
        // Preserve what's on disk before overwriting it: `settings.json` is
        // shared across every corral build on the machine, so a file written
        // by a different (newer) build, or hand-edited, shouldn't be
        // silently destroyed by a save from a build that couldn't fully read
        // it. `load_or_default` also backs up the file when it can't parse
        // it, so this is belt-and-suspenders for the "about to overwrite"
        // moment.
        json_store::backup_existing(path, "settings");
        json_store::save_pretty(path, settings)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fields_default_to_conservative_values() {
        let s = Settings::default();
        assert!(!s.skip_permissions);
        assert!(!s.co_author_trailer);
        assert!(!s.sound_enabled);
        assert_eq!(s.terminal_font_family, "");
        assert_eq!(s.scrollback_kb, 256);
        assert!(s.delete_branch_default);
        assert_eq!(s.sidebar_width, 320);
        assert_eq!(s.changes_width, 384);
        assert!(s.caffeinate_enabled);
        assert!(s.project_order.is_empty());
        assert_eq!(s.terminal_command, "ghostty");
        assert!(!s.idle_timeout_enabled);
        assert_eq!(s.idle_timeout_minutes, 30);
    }

    #[test]
    fn deserializes_missing_new_fields_to_defaults() {
        // Simulates an old settings.json written before these fields existed.
        // "waiting_threshold_ms" is a legacy key from a since-removed field;
        // serde silently ignores unknown fields, so old files with it still
        // deserialize fine.
        let json = r#"{
            "editor_command": "zeditor",
            "waiting_threshold_ms": 1500,
            "notifications_enabled": true,
            "terminal_font_size": 13,
            "collapsed_groups": [],
            "sidebar_collapsed": false,
            "changes_collapsed": false,
            "extra_instructions": ""
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(!s.skip_permissions);
        assert!(!s.co_author_trailer);
        assert!(!s.sound_enabled);
        assert_eq!(s.terminal_font_family, "");
        assert_eq!(s.scrollback_kb, 256);
        assert!(s.delete_branch_default);
        assert_eq!(s.sidebar_width, 320);
        assert_eq!(s.changes_width, 384);
        assert!(s.caffeinate_enabled);
        assert!(s.project_order.is_empty());
    }

    #[test]
    fn worktrees_root_defaults_to_empty_string() {
        let s = Settings::default();
        assert_eq!(s.worktrees_root, "");
    }

    #[test]
    fn deserializes_missing_worktrees_root_to_empty_string() {
        // Simulates a settings.json written before this field existed.
        let json = r#"{
            "editor_command": "zeditor",
            "notifications_enabled": true,
            "terminal_font_size": 13,
            "collapsed_groups": [],
            "sidebar_collapsed": false,
            "changes_collapsed": false,
            "extra_instructions": ""
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.worktrees_root, "");
    }

    #[test]
    fn terminal_command_defaults_to_ghostty() {
        let s = Settings::default();
        assert_eq!(s.terminal_command, "ghostty");
    }

    #[test]
    fn deserializes_missing_terminal_command_to_ghostty() {
        // Simulates a settings.json written before this field existed.
        let json = r#"{
            "editor_command": "zeditor",
            "notifications_enabled": true,
            "terminal_font_size": 13,
            "collapsed_groups": [],
            "sidebar_collapsed": false,
            "changes_collapsed": false,
            "extra_instructions": ""
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.terminal_command, "ghostty");
    }

    #[test]
    fn deserializes_missing_idle_timeout_fields_to_defaults() {
        let json = r#"{
            "editor_command": "zeditor",
            "notifications_enabled": true,
            "terminal_font_size": 13,
            "collapsed_groups": [],
            "sidebar_collapsed": false,
            "changes_collapsed": false,
            "extra_instructions": ""
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(!s.idle_timeout_enabled);
        assert_eq!(s.idle_timeout_minutes, 30);
    }

    #[test]
    fn settings_path_in_joins_settings_json() {
        let path = settings_path_in(std::path::Path::new("/home/x/.config/corral"));
        assert_eq!(
            path,
            std::path::PathBuf::from("/home/x/.config/corral/settings.json")
        );
    }

    #[test]
    fn load_under_a_missing_path_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist").join("settings.json");
        let s = load_under(&path).unwrap();
        assert_eq!(s.scrollback_kb, Settings::default().scrollback_kb);
    }

    #[test]
    fn save_then_load_round_trips_a_non_default_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings {
            editor_command: "code".into(),
            notifications_enabled: false,
            terminal_font_size: 16,
            collapsed_groups: vec!["corral".into(), "other-project".into()],
            project_order: vec!["other-project".into(), "corral".into()],
            sidebar_collapsed: true,
            changes_collapsed: true,
            sidebar_width: 280,
            changes_width: 500,
            extra_instructions: "always use pnpm".into(),
            skip_permissions: true,
            co_author_trailer: true,
            sound_enabled: true,
            sound_choice: "whistle".into(),
            terminal_font_family: "Fira Code".into(),
            scrollback_kb: 512,
            delete_branch_default: false,
            caffeinate_enabled: false,
            worktrees_root: "~/elsewhere".into(),
            terminal_command: "alacritty".into(),
            idle_timeout_enabled: true,
            idle_timeout_minutes: 45,
            claude_executable_path: "/opt/claude-nightly/bin/claude".into(),
            diff_font_size: 20,
            harness: crate::harness::Harness::Claude,
            opencode_executable_path: String::new(),
            opencode2_executable_path: String::new(),
            pi_executable_path: "/opt/pi/bin/pi".into(),
        };

        save_under(&path, &s).unwrap();
        let loaded = load_under(&path).unwrap();

        assert_eq!(loaded.editor_command, s.editor_command);
        assert_eq!(loaded.notifications_enabled, s.notifications_enabled);
        assert_eq!(loaded.terminal_font_size, s.terminal_font_size);
        assert_eq!(loaded.collapsed_groups, s.collapsed_groups);
        assert_eq!(loaded.project_order, s.project_order);
        assert_eq!(loaded.sidebar_collapsed, s.sidebar_collapsed);
        assert_eq!(loaded.changes_collapsed, s.changes_collapsed);
        assert_eq!(loaded.sidebar_width, s.sidebar_width);
        assert_eq!(loaded.changes_width, s.changes_width);
        assert_eq!(loaded.extra_instructions, s.extra_instructions);
        assert_eq!(loaded.skip_permissions, s.skip_permissions);
        assert_eq!(loaded.co_author_trailer, s.co_author_trailer);
        assert_eq!(loaded.sound_enabled, s.sound_enabled);
        assert_eq!(loaded.sound_choice, s.sound_choice);
        assert_eq!(loaded.terminal_font_family, s.terminal_font_family);
        assert_eq!(loaded.scrollback_kb, s.scrollback_kb);
        assert_eq!(loaded.delete_branch_default, s.delete_branch_default);
        assert_eq!(loaded.caffeinate_enabled, s.caffeinate_enabled);
        assert_eq!(loaded.worktrees_root, s.worktrees_root);
        assert_eq!(loaded.terminal_command, s.terminal_command);
        assert_eq!(loaded.idle_timeout_enabled, s.idle_timeout_enabled);
        assert_eq!(loaded.idle_timeout_minutes, s.idle_timeout_minutes);
        assert_eq!(loaded.claude_executable_path, s.claude_executable_path);
        assert_eq!(loaded.diff_font_size, s.diff_font_size);
        assert_eq!(loaded.harness, s.harness);
        assert_eq!(loaded.opencode_executable_path, s.opencode_executable_path);
        assert_eq!(
            loaded.opencode2_executable_path,
            s.opencode2_executable_path
        );
        assert_eq!(loaded.pi_executable_path, s.pi_executable_path);
    }

    #[test]
    fn deserializes_missing_claude_executable_path_to_empty_string() {
        // Simulates a settings.json written before this field existed.
        let json = r#"{
            "editor_command": "zeditor",
            "notifications_enabled": true,
            "terminal_font_size": 13,
            "collapsed_groups": [],
            "sidebar_collapsed": false,
            "changes_collapsed": false,
            "extra_instructions": ""
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.claude_executable_path, "");
    }

    #[test]
    fn diff_font_size_defaults_to_16() {
        let s = Settings::default();
        assert_eq!(s.diff_font_size, 16);
    }

    #[test]
    fn deserializes_missing_diff_font_size_to_default() {
        // Simulates a settings.json written before this field existed.
        let json = r#"{
            "editor_command": "zeditor",
            "notifications_enabled": true,
            "terminal_font_size": 13,
            "collapsed_groups": [],
            "sidebar_collapsed": false,
            "changes_collapsed": false,
            "extra_instructions": ""
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.diff_font_size, 16);
    }

    #[test]
    fn loading_settings_json_without_harness_field_defaults_to_claude() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"editor_command": "zeditor"}"#).unwrap();
        let settings = load_under(&path).unwrap();
        assert_eq!(settings.harness, crate::harness::Harness::Claude);
        assert_eq!(settings.opencode_executable_path, "");
        assert_eq!(settings.pi_executable_path, "");
    }

    #[test]
    fn unknown_harness_from_a_newer_build_survives_save_round_trip() {
        // The regression this whole forward-compat design guards against: an
        // older corral build reading a settings.json written by a newer one
        // (here, harness `opencode9`) must not fail to parse (which would
        // reset the file to defaults on the next save) and must not
        // downgrade the harness on save — the value round-trips verbatim.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"harness": "opencode9"}"#).unwrap();

        let loaded = load_under(&path).unwrap();
        assert_eq!(
            loaded.harness,
            crate::harness::Harness::Unknown("opencode9".to_string())
        );

        // The app's other settings all default (as load-or-default does);
        // saving must keep the unknown harness intact.
        save_under(&path, &loaded).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("\"opencode9\""),
            "unknown harness was clobbered on save: {raw}"
        );
    }

    #[test]
    fn save_under_backs_up_the_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"editor_command": "zeditor"}"#).unwrap();

        save_under(&path, &Settings::default()).unwrap();

        let backup = dir.path().join("settings.json.bak");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            r#"{"editor_command": "zeditor"}"#
        );
    }
}
