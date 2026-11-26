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
use serde_json::Value;

use crate::error::Result;
use crate::json_store;
use crate::paths;
use crate::secret;

/// Default external-editor command for the "Zed" button. Zed's CLI is named
/// differently per platform: distro packages on Linux install `zeditor` (the
/// `zed` name is often a stale wrapper — see AGENTS.md), while the macOS app
/// installs `zed`.
#[cfg(target_os = "macos")]
pub const DEFAULT_EDITOR_COMMAND: &str = "zed";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_EDITOR_COMMAND: &str = "zeditor";

/// Default external-terminal command for the "Shell" button. macOS has no
/// `--working-directory` convention: `open -a Terminal <dir>` is the
/// equivalent (see `commands/system.rs::terminal_args`).
#[cfg(target_os = "macos")]
pub const DEFAULT_TERMINAL_COMMAND: &str = "open";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_TERMINAL_COMMAND: &str = "ghostty";

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
    /// Inhibit system sleep/idle (via the platform's inhibitor — see
    /// `crate::caffeinate`) while at least one agent session is actively
    /// working (running, not waiting on the user).
    pub caffeinate_enabled: bool,
    /// Root directory under which new worktrees are created. Empty string
    /// means "use the default" (`~/corral`) — see `config::worktrees_root`.
    /// May contain a leading `~` for the home directory.
    pub worktrees_root: String,
    /// Command used by the "Shell" button to open an external terminal at a
    /// worktree's path (e.g. `ghostty`, `alacritty`, `kitty` on Linux, `open`
    /// on macOS — see `DEFAULT_TERMINAL_COMMAND`).
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
    /// (`opencode`, resolved via `PATH`; current `@opencode/cli` releases no
    /// longer ship the old `opencode2` alias).
    pub opencode2_executable_path: String,
    /// Path or command name used to spawn Pi sessions when `harness == Pi`.
    /// Empty string means "use the default" (`pi`, resolved via `PATH`).
    pub pi_executable_path: String,
    /// Attach corral's embedded browser host to OpenCode v2 sessions, which is
    /// what enables OpenCode's built-in `browser.*` tools. On by default: the
    /// attachment is what makes those tools exist at all. Turning it off
    /// spawns nothing extra (no SSE/subscribe threads, no browser window) and
    /// leaves the tools unavailable — useful for users who never want them, or
    /// as a switch if a future opencode release changes the protocol.
    pub browser_enabled: bool,
    /// Personal Linear API key, used by the Linear ticket commands to read
    /// issue titles/bodies. Empty means "Linear integration not set up".
    ///
    /// Unlike every other field this is a credential, so it is *never*
    /// serialized: `#[serde(skip_serializing)]` keeps it out of `settings.json`
    /// (and its `.bak` snapshots) entirely, and out of any `Settings` handed
    /// across the IPC boundary *to* the webview. It lives in its own owner-only
    /// file next to `settings.json` — see [`load_linear_api_key`] /
    /// [`set_linear_api_key`] — which is where [`load`] reads it from.
    ///
    /// It must stay deserializable, though: `save_settings` is the one way a
    /// freshly typed key reaches the backend, and the webview sends the whole
    /// `Settings` (with the typed key in this field, empty when unchanged).
    /// `skip_serializing_if` drops the always-empty webview copy from the blob
    /// as well. A plain `#[serde(skip)]` silently discarded the typed key on
    /// deserialization, so the form cleared without saving anything.
    #[serde(skip_serializing, skip_serializing_if = "String::is_empty")]
    pub linear_api_key: String,
    /// Whether a Linear API key is stored, for the UI to gate on (the key
    /// itself never leaves the backend). Derived from the key file on every
    /// [`load`], so the value persisted in `settings.json` is only a hint for
    /// hand-edited files — never the source of truth.
    pub linear_api_key_set: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            editor_command: DEFAULT_EDITOR_COMMAND.into(),
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
            terminal_command: DEFAULT_TERMINAL_COMMAND.into(),
            idle_timeout_enabled: false,
            idle_timeout_minutes: 30,
            claude_executable_path: String::new(),
            harness: crate::harness::Harness::default(),
            opencode_executable_path: String::new(),
            opencode2_executable_path: String::new(),
            pi_executable_path: String::new(),
            browser_enabled: true,
            linear_api_key: String::new(),
            linear_api_key_set: false,
        }
    }
}

/// Name of the Linear API key secret — an OS-keychain entry name, and (for the
/// file fallback) the file name in the config dir.
pub const LINEAR_API_KEY: &str = "linear_api_key";

// File helpers used by `crate::secret`'s fallback backend. `0600`, and a
// missing file reads as `None` rather than an error — but an unreadable one is
// an error, so callers never mistake "can't read" for "not set".

pub(crate) fn read_secret_file(path: &std::path::Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn write_secret_file(path: &std::path::Path, value: &str) -> Result<()> {
    // `\n` terminates the value so `cat`/`wc -l` behave in a terminal; the
    // reader trims it back off.
    json_store::write_private(path, format!("{}\n", value.trim()).as_bytes())
}

pub(crate) fn remove_secret_file(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// The stored Linear API key, or empty when none is set.
pub fn load_linear_api_key() -> Result<String> {
    Ok(secret::get(LINEAR_API_KEY, &paths::config_root()?)?.unwrap_or_default())
}

/// Store the Linear API key (trimmed); blank clears it.
pub fn set_linear_api_key(key: &str) -> Result<()> {
    secret::set(LINEAR_API_KEY, &paths::config_root()?, key).map(|_| ())
}

/// Whether a Linear key is stored, without loading its value where avoidable.
pub fn linear_api_key_is_set() -> Result<bool> {
    Ok(!load_linear_api_key()?.is_empty())
}

/// Where the key is stored right now ("system keychain" vs "owner-only file"),
/// so Settings can say so.
pub fn linear_api_key_storage_description() -> Result<&'static str> {
    Ok(secret::backend_description(&paths::config_root()?))
}

pub fn load() -> Result<Settings> {
    load_under(&settings_path()?)
}

pub fn save(settings: &Settings) -> Result<()> {
    save_under(&settings_path()?, settings)
}

/// Forget the stored Linear API key (the explicit "remove it" half of
/// [`set_linear_api_key`] — saving a blank key through [`save`] is a no-op,
/// since the webview's copy of `Settings` is always blank).
pub fn clear_linear_api_key() -> Result<()> {
    secret::clear(LINEAR_API_KEY, &paths::config_root()?)
}

fn load_under(path: &std::path::Path) -> Result<Settings> {
    let mut settings: Settings = json_store::load_or_default(path, "settings")?;
    // Older builds stored the key *inside* `settings.json`; the secret store
    // is now the only home for it, so migrate a legacy copy there and evict
    // it from the JSON blob (`.bak` copies go on the next save). Without this,
    // the first release that ships this would silently lose keys set by the
    // previous one.
    let config_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    migrate_legacy_linear_key(config_dir);
    // The key lives in the secret store rather than in `settings.json`, so
    // load it the same way `save_under` persists it. A *missing* key is "not
    // set", not an error — `load` must stay usable for every other field. Any
    // other failure (a key we can't read) must not be turned into "not set":
    // the Settings form would then offer to add a key, and saving would
    // overwrite one that's still there.
    settings.linear_api_key = secret::get(LINEAR_API_KEY, config_dir)?.unwrap_or_default();
    settings.linear_api_key_set = !settings.linear_api_key.is_empty();
    Ok(settings)
}

/// Move a `linear_api_key` value found in an old `settings.json` into the
/// secret store, then rewrite the blob without it. Best-effort: a failed write
/// leaves the value where it was (still readable) and the next launch retries.
///
/// Two generations of legacy layout are handled: the original one kept the key
/// in `settings.json` (this function), and the immediately preceding one kept
/// it in a plain `linear_api_key` file beside it (the file branch below).
fn migrate_legacy_linear_key(config_dir: &std::path::Path) {
    let path = settings_path_in(config_dir);
    // The file layout wins if both exist — it's newer — and only a stale
    // `settings.json` copy gets adopted.
    let stored = secret::get(LINEAR_API_KEY, config_dir).ok().flatten();
    if let Ok(Some(file_value)) = read_secret_file(&config_dir.join(LINEAR_API_KEY))
        && stored.is_none()
        && let Err(e) = secret::set(LINEAR_API_KEY, config_dir, &file_value)
    {
        tracing::warn!("settings: couldn't move the Linear key file into the secret store: {e}");
    }
    let raw = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let Ok(Value::Object(mut map)) = serde_json::from_slice::<Value>(&raw) else {
        return;
    };
    let Some(Value::String(legacy)) = map.remove("linear_api_key") else {
        return;
    };
    let key = legacy.trim();
    let already_stored = secret::get(LINEAR_API_KEY, config_dir)
        .ok()
        .flatten()
        .is_some_and(|v| !v.is_empty());
    if !key.is_empty()
        && !already_stored
        && let Err(e) = secret::set(LINEAR_API_KEY, config_dir, key)
    {
        // Keep the legacy value in place rather than dropping it on a failed
        // write — losing the user's key to storage trouble is worse than
        // leaving it in the old spot for the next launch.
        tracing::warn!("settings: couldn't move legacy Linear key into the secret store: {e}");
        return;
    }
    match serde_json::to_vec_pretty(&Value::Object(map)) {
        Ok(bytes) => {
            if let Err(e) = json_store::write_private(&path, &bytes) {
                tracing::warn!("settings: couldn't evict legacy Linear key from {path:?}: {e}");
                return;
            }
            tracing::info!("settings: moved Linear API key out of settings.json");
        }
        Err(e) => tracing::warn!("settings: couldn't rewrite {path:?} after migration: {e}"),
    }
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
    // A blank key means "unchanged" rather than "clear the stored key": the
    // webview never receives the real key (it's `skip_serializing`), so every
    // save it sends has it blank, and treating that as a clear would log the
    // user out of Linear on the next unrelated toggle. Clearing is explicit,
    // via `clear_linear_api_key`.
    let new_key = settings.linear_api_key.trim();
    // Belt-and-suspenders: the field is `skip_serializing`, but blanking it
    // here means a future change to that attribute can't silently start
    // writing the credential to `settings.json`.
    let mut scrubbed = settings.clone();
    scrubbed.linear_api_key.clear();
    scrubbed.linear_api_key_set = settings.linear_api_key_set || !new_key.is_empty();
    // The key goes to the secret store (keychain, or its file fallback) and
    // never through `#[serde]`, so neither `settings.json` nor its `.bak`
    // snapshot can contain it. Outside the settings lock: keychain access can
    // prompt for unlock, and holding the store lock across that would stall
    // every other corral process waiting on settings.
    if !new_key.is_empty() {
        secret::set(LINEAR_API_KEY, dir, new_key)?;
    }
    json_store::with_locked_path(dir, &file_name, |path| {
        // Preserve what's on disk before overwriting it: `settings.json` is
        // shared across every corral build on the machine, so a file written
        // by a different (newer) build, or hand-edited, shouldn't be
        // silently destroyed by a save from a build that couldn't fully read
        // it. `load_or_default` also backs up the file when it can't parse
        // it, so this is belt-and-suspenders for the "about to overwrite"
        // moment.
        json_store::backup_existing(path, "settings");
        json_store::save_pretty(path, &scrubbed)
    })?;
    // Files written by older builds (and any hand-edited or restored from a
    // backup) are still at the umask default. Tightening is best-effort: a
    // failure here must not fail the save itself, since the JSON layer already
    // writes owner-only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [path.to_path_buf(), path.with_file_name("settings.json.bak")] {
            if let Err(e) = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)) {
                tracing::debug!("settings: couldn't tighten permissions on {p:?}: {e}");
            }
        }
    }
    Ok(())
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
        assert_eq!(s.terminal_command, DEFAULT_TERMINAL_COMMAND);
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
    fn terminal_command_defaults_to_the_platform_default() {
        let s = Settings::default();
        assert_eq!(s.terminal_command, DEFAULT_TERMINAL_COMMAND);
    }

    #[test]
    fn editor_command_defaults_to_the_platform_default() {
        let s = Settings::default();
        assert_eq!(s.editor_command, DEFAULT_EDITOR_COMMAND);
    }

    #[test]
    fn deserializes_missing_terminal_command_to_the_platform_default() {
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
        assert_eq!(s.terminal_command, DEFAULT_TERMINAL_COMMAND);
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
            browser_enabled: false,
            linear_api_key: "lin_api_test".into(),
            linear_api_key_set: true,
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
        assert_eq!(loaded.browser_enabled, s.browser_enabled);
        assert_eq!(loaded.linear_api_key, s.linear_api_key);
        assert!(loaded.linear_api_key_set);
    }

    #[test]
    fn linear_api_key_defaults_to_empty_string() {
        let s = Settings::default();
        assert_eq!(s.linear_api_key, "");
        assert!(!s.linear_api_key_set);

        // Simulates a settings.json written before this field existed.
        let json = r#"{"editor_command":"zeditor"}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.linear_api_key, "");
    }

    #[test]
    fn linear_api_key_never_reaches_settings_json_or_its_backup() {
        let _guard = crate::secret::tests::FILE_ONLY_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // The whole reason the key lives in its own file: `settings.json` is
        // rewritten on every unrelated toggle, and every save snapshots the
        // previous contents to `.bak`. Neither may contain the credential.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        let mut s = Settings {
            linear_api_key: "lin_api_secret".into(),
            ..Settings::default()
        };
        save_under(&path, &s).unwrap();

        // A later save from the webview (which never receives the key) must
        // not clear the stored one.
        s.linear_api_key.clear();
        s.sidebar_width = 400;
        save_under(&path, &s).unwrap();

        for file in ["settings.json", "settings.json.bak"] {
            let text = std::fs::read_to_string(dir.path().join(file)).unwrap();
            assert!(!text.contains("lin_api_secret"), "{file} leaked the key");
        }
        assert_eq!(load_under(&path).unwrap().linear_api_key, "lin_api_secret");

        // Explicit clear removes it.
        secret::clear(LINEAR_API_KEY, dir.path()).unwrap();
        assert!(load_under(&path).unwrap().linear_api_key.is_empty());
        assert!(!load_under(&path).unwrap().linear_api_key_set);
    }

    #[test]
    fn a_typed_key_survives_the_ipc_round_trip() {
        // `save_settings` receives a whole `Settings` as JSON from the webview,
        // with the freshly typed key in `linear_api_key` (that's the only way
        // a new key gets in). The field is `skip_serializing` so the key never
        // goes *out*, but it must still deserialize — a plain `#[serde(skip)]`
        // dropped the typed value, so Settings appeared to save and immediately
        // cleared, with nothing stored.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        let ipc = serde_json::json!({
            "editor_command": "zeditor",
            "linear_api_key": "lin_api_typed",
            "linear_api_key_set": true,
        });
        let settings: Settings = serde_json::from_value(ipc).unwrap();
        assert_eq!(settings.linear_api_key, "lin_api_typed");

        save_under(&path, &settings).unwrap();
        let loaded = load_under(&path).unwrap();
        assert_eq!(loaded.linear_api_key, "lin_api_typed");
        assert!(loaded.linear_api_key_set);
        // And the webview's own copy, on the way back out, still omits it.
        let outbound = serde_json::to_value(&loaded).unwrap();
        assert!(outbound.get("linear_api_key").is_none());
    }

    #[test]
    fn legacy_key_inside_settings_json_is_migrated_out() {
        let _guard = crate::secret::tests::FILE_ONLY_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Builds before this one stored the key in `settings.json`; the first
        // launch after the upgrade must move it to the key file and evict it
        // from the blob, or the key would be silently forgotten (and keep
        // sitting at world-readable permissions).
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        std::fs::write(
            &path,
            r#"{"editor_command":"zeditor","linear_api_key":"lin_api_legacy"}"#,
        )
        .unwrap();

        let loaded = load_under(&path).unwrap();
        assert_eq!(loaded.linear_api_key, "lin_api_legacy");
        assert!(loaded.linear_api_key_set);
        let blob = std::fs::read_to_string(&path).unwrap();
        assert!(!blob.contains("lin_api_legacy"), "legacy key not evicted");
        assert!(blob.contains("zeditor"), "other fields must survive");
    }

    #[test]
    fn unreadable_key_file_is_an_error_not_a_silent_clear() {
        let _guard = crate::secret::tests::FILE_ONLY_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // A key file that exists but can't be read must surface as an error:
        // treating it as "not set" would make the Settings form offer to add
        // a key, and the next save would overwrite the one still on disk.
        //
        // The lock above is load-bearing for the *failure*: without it, a
        // concurrent test setting the process-global file-only env var lets
        // the OS store (which has no entry) answer first, so the broken file
        // is never read and the load just looks "not set".
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        // A directory in the key file's place: readable as a path, unreadable
        // as a file, on every platform and without relying on permissions.
        std::fs::create_dir(dir.path().join(LINEAR_API_KEY)).unwrap();

        assert!(load_under(&path).is_err());
    }

    #[test]
    fn legacy_key_file_moves_into_the_secret_store() {
        let _guard = crate::secret::tests::FILE_ONLY_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // The layout shipped before the keyring work wrote a plain
        // `linear_api_key` file. Upgrading must adopt it (rather than treating
        // the store as empty) and put it behind `secret`.
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path_in(dir.path());
        write_secret_file(&dir.path().join(LINEAR_API_KEY), "lin_api_file").unwrap();

        let loaded = load_under(&path).unwrap();
        assert_eq!(loaded.linear_api_key, "lin_api_file");
        assert!(loaded.linear_api_key_set);
    }

    #[test]
    fn save_pretty_writes_owner_only_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save_under(&path, &Settings::default()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
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
