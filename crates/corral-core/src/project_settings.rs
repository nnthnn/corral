//! Per-project registry: which git repos on disk corral tracks as
//! "projects", plus each one's provisioning rules (which files/globs to
//! copy into a fresh worktree, and what setup command, if any, to run there
//! afterward).
//!
//! Stored at `~/.config/corral/projects.json` (XDG config dir via
//! `crate::paths::config_root`, so it moves with `CORRAL_CONFIG_DIR`), keyed
//! by project name — a sibling file to `settings.rs`'s single global
//! `settings.json`. A project *exists* in corral if and only if it has an
//! entry here (added via [`register`], e.g. from the GUI's Add Project
//! folder picker or `corral project add <path>`); its provisioning fields
//! default to "do nothing" ([`ProjectSettings::default`]) until a user
//! opts in via the settings dialog.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::json_store;

const STORE_FILE: &str = "projects.json";

fn projects_dir() -> Result<PathBuf> {
    crate::paths::config_root()
}

fn projects_path() -> Result<PathBuf> {
    Ok(projects_dir()?.join(STORE_FILE))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProjectSettings {
    /// Absolute path to this project's git repo on disk. Set once when the
    /// project is registered ([`register`]) and preserved by [`set`] even
    /// though `set`'s callers (e.g. the `save_project_settings` Tauri
    /// command) never populate it themselves — only [`register`]/
    /// [`unregister`] are meant to change it.
    pub path: PathBuf,
    /// Glob patterns (relative to the repo root) copied into every new
    /// worktree of this project, matched with `globset`.
    pub copy_globs: Vec<String>,
    /// Shell command run via `sh -c` in the new worktree's directory, after
    /// files are copied and `.claude` is linked. Empty = no-op. Also a fine
    /// place for a user's own `ln -s` commands if they'd rather symlink a
    /// path than copy it.
    pub setup_command: String,
    /// Shell command run via `sh -c` for the Run tab (e.g. a dev server).
    /// Empty means "no saved override — auto-detect a best-effort default
    /// instead" (see `commands::run_detect` in the `corral-app` crate).
    /// Updated whenever the user runs a different command from the Run tab,
    /// so the next time it's opened it prefills with what was actually run.
    pub run_command: String,
    /// Project-specific context appended to every fresh session's system
    /// prompt, after the global `extra_instructions` setting. Empty = no-op.
    pub project_prompt: String,
    /// Per-project environment variables injected into every agent session
    /// spawned for this project (see `commands::session::start_session`).
    /// One `KEY=VALUE` string per entry, e.g. `"API_KEY=..."` — blank lines,
    /// `#`-comments, and lines without a `=` are ignored at injection time
    /// (see [`ProjectSettings::env_vars`]). Empty = no extra env.
    pub env: Vec<String>,
}

impl ProjectSettings {
    /// Parse `self.env` (one `KEY=VALUE` per entry) into the `(key, value)`
    /// pairs to inject into a spawned agent, skipping blank lines, `#`
    /// comments, and lines without a `=`. Keys and values are trimmed; a
    /// value may itself contain `=` (only the first `=` splits the pair).
    pub fn env_vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.env.iter().filter_map(|line| parse_env_line(line))
    }
}

/// Parse one `KEY=VALUE` line into `(key, value)`, or `None` for a blank or
/// comment line or one without a `=`.
fn parse_env_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim()))
}

pub(crate) type Store = HashMap<String, ProjectSettings>;

fn load_from(path: &Path) -> Result<Store> {
    json_store::load_or_default(path, "project_settings")
}

#[cfg(test)]
fn save_to(path: &Path, store: &Store) -> Result<()> {
    json_store::save_pretty(path, store)
}

/// This project's provisioning rules, or `ProjectSettings::default()` if
/// none have been saved yet.
pub fn get(project: &str) -> Result<ProjectSettings> {
    Ok(load_from(&projects_path()?)?
        .remove(project)
        .unwrap_or_default())
}

/// Persist `settings` as `project`'s provisioning rules. Preserves the
/// existing entry's `path` if `settings.path` is empty (the normal case —
/// see `ProjectSettings::path`'s doc comment) so this can never
/// accidentally un-register a project.
///
/// Locks a sibling `projects.json.lock` across the read-modify-write so two
/// processes (or corral windows) saving different projects' settings at
/// nearly the same time can't race and silently drop one write.
pub fn set(project: &str, settings: &ProjectSettings) -> Result<()> {
    set_in(&projects_dir()?, project, settings)
}

fn set_in(dir: &Path, project: &str, settings: &ProjectSettings) -> Result<()> {
    json_store::with_locked_store(dir, STORE_FILE, "project_settings", |store: &mut Store| {
        let mut entry = settings.clone();
        if entry.path.as_os_str().is_empty()
            && let Some(existing) = store.get(project)
        {
            entry.path = existing.path.clone();
        }
        store.insert(project.to_string(), entry);
    })
}

/// Register a new project at `path`, with default (empty) provisioning
/// settings. Errors with [`Error::ProjectAlreadyRegistered`] if `name` is
/// already registered — re-registering an existing project would silently
/// reset its provisioning settings, which is never what a caller wants.
pub fn register(name: &str, path: PathBuf) -> Result<()> {
    register_in(&projects_dir()?, name, path)
}

fn register_in(dir: &Path, name: &str, path: PathBuf) -> Result<()> {
    json_store::with_locked_store(dir, STORE_FILE, "project_settings", |store: &mut Store| {
        if store.contains_key(name) {
            Err(Error::ProjectAlreadyRegistered(name.to_string()))
        } else {
            store.insert(
                name.to_string(),
                ProjectSettings {
                    path,
                    ..Default::default()
                },
            );
            Ok(())
        }
    })?
}

/// Remove a project's registry entry entirely (path and provisioning
/// settings both gone). A no-op if `name` wasn't registered.
pub fn unregister(name: &str) -> Result<()> {
    unregister_in(&projects_dir()?, name)
}

fn unregister_in(dir: &Path, name: &str) -> Result<()> {
    json_store::with_locked_store(dir, STORE_FILE, "project_settings", |store: &mut Store| {
        store.remove(name);
    })
}

/// The registered path for `name`, or `None` if it isn't registered.
pub fn get_path(name: &str) -> Result<Option<PathBuf>> {
    Ok(load_from(&projects_path()?)?
        .get(name)
        .map(|s| s.path.clone()))
}

/// Every registered project's settings, keyed by name — the definitive
/// project list (see `project::list()`).
pub(crate) fn all() -> Result<Store> {
    load_from(&projects_path()?)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn default_has_no_copy_globs_setup_command_or_run_command() {
        let s = ProjectSettings::default();
        assert_eq!(s.copy_globs, Vec::<String>::new());
        assert_eq!(s.setup_command, "");
        assert_eq!(s.run_command, "");
        assert!(s.env.is_empty());
    }

    #[test]
    fn load_from_missing_file_returns_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = load_from(&dir.path().join("projects.json")).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn load_from_corrupt_file_resets_to_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        fs::write(&path, b"not json").unwrap();
        assert!(load_from(&path).unwrap().is_empty());
    }

    #[test]
    fn save_then_load_round_trips_a_projects_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        let mut store = Store::default();
        store.insert(
            "corral".into(),
            ProjectSettings {
                path: "/home/x/source/corral".into(),
                copy_globs: vec!["*.md".into()],
                setup_command: "pnpm install".into(),
                run_command: "pnpm dev".into(),
                project_prompt: "use pnpm, not npm".into(),
                env: vec!["API_KEY=abc".into(), "DEBUG=true".into()],
            },
        );
        save_to(&path, &store).unwrap();

        let loaded = load_from(&path).unwrap();
        let corral = loaded.get("corral").unwrap();
        assert_eq!(
            corral.path,
            std::path::PathBuf::from("/home/x/source/corral")
        );
        assert_eq!(corral.copy_globs, vec!["*.md".to_string()]);
        assert_eq!(corral.setup_command, "pnpm install");
        assert_eq!(corral.run_command, "pnpm dev");
        assert_eq!(corral.project_prompt, "use pnpm, not npm");
        assert_eq!(
            corral.env,
            vec!["API_KEY=abc".to_string(), "DEBUG=true".to_string()]
        );
    }

    #[test]
    fn env_vars_parses_key_value_lines_and_skips_blank_comment_and_malformed() {
        let s = ProjectSettings {
            env: vec![
                "API_KEY=abc".into(),
                "# a comment".into(),
                "   ".into(),
                "no equals sign".into(),
                " DEBUG = true ".into(),
                "URL=a=b=c".into(),
            ],
            ..Default::default()
        };
        let vars: Vec<(&str, &str)> = s.env_vars().collect();
        assert_eq!(
            vars,
            vec![("API_KEY", "abc"), ("DEBUG", "true"), ("URL", "a=b=c"),]
        );
    }

    /// The whole point of routing `set` through `json_store::with_locked_store`:
    /// without a lock, concurrent writers for *different* projects would each
    /// load the same starting map and one's insert would clobber the other's
    /// on save.
    #[test]
    fn concurrent_set_calls_for_different_projects_do_not_drop_each_others_updates() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        let handles: Vec<_> = (0..20)
            .map(|i| {
                let dir_path = dir_path.clone();
                std::thread::spawn(move || {
                    set_in(
                        &dir_path,
                        &format!("project-{i}"),
                        &ProjectSettings {
                            path: PathBuf::new(),
                            copy_globs: vec![],
                            setup_command: format!("cmd-{i}"),
                            run_command: String::new(),
                            project_prompt: String::new(),
                            env: vec![],
                        },
                    )
                    .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        let store = load_from(&dir_path.join(STORE_FILE)).unwrap();
        assert_eq!(
            store.len(),
            20,
            "expected all 20 concurrent writes to survive, got {:?}",
            store.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn register_in_adds_a_new_entry_with_the_given_path_and_default_settings() {
        let dir = tempfile::tempdir().unwrap();
        register_in(dir.path(), "corral", PathBuf::from("/home/x/source/corral")).unwrap();

        let store = load_from(&dir.path().join(STORE_FILE)).unwrap();
        let entry = store.get("corral").unwrap();
        assert_eq!(entry.path, PathBuf::from("/home/x/source/corral"));
        assert_eq!(entry.setup_command, "");
    }

    #[test]
    fn register_in_errors_if_the_name_is_already_registered() {
        let dir = tempfile::tempdir().unwrap();
        register_in(dir.path(), "corral", PathBuf::from("/a")).unwrap();

        let result = register_in(dir.path(), "corral", PathBuf::from("/b"));
        assert!(matches!(result, Err(Error::ProjectAlreadyRegistered(_))));
        // The original entry must be untouched by the failed re-registration.
        let store = load_from(&dir.path().join(STORE_FILE)).unwrap();
        assert_eq!(store.get("corral").unwrap().path, PathBuf::from("/a"));
    }

    #[test]
    fn unregister_in_removes_the_entry() {
        let dir = tempfile::tempdir().unwrap();
        register_in(dir.path(), "corral", PathBuf::from("/a")).unwrap();

        unregister_in(dir.path(), "corral").unwrap();

        let store = load_from(&dir.path().join(STORE_FILE)).unwrap();
        assert!(!store.contains_key("corral"));
    }

    #[test]
    fn unregister_in_is_a_noop_for_an_unknown_name() {
        let dir = tempfile::tempdir().unwrap();
        // Must not error just because there was nothing to remove.
        unregister_in(dir.path(), "never-registered").unwrap();
    }

    #[test]
    fn set_in_preserves_the_existing_path_when_incoming_settings_have_no_path() {
        let dir = tempfile::tempdir().unwrap();
        register_in(dir.path(), "corral", PathBuf::from("/home/x/source/corral")).unwrap();

        set_in(
            dir.path(),
            "corral",
            &ProjectSettings {
                path: PathBuf::new(),
                setup_command: "pnpm install".into(),
                ..Default::default()
            },
        )
        .unwrap();

        let store = load_from(&dir.path().join(STORE_FILE)).unwrap();
        let entry = store.get("corral").unwrap();
        assert_eq!(entry.path, PathBuf::from("/home/x/source/corral"));
        assert_eq!(entry.setup_command, "pnpm install");
    }

    #[test]
    fn set_in_uses_the_incoming_path_when_one_is_provided() {
        let dir = tempfile::tempdir().unwrap();
        register_in(dir.path(), "corral", PathBuf::from("/old/path")).unwrap();

        set_in(
            dir.path(),
            "corral",
            &ProjectSettings {
                path: PathBuf::from("/new/path"),
                ..Default::default()
            },
        )
        .unwrap();

        let store = load_from(&dir.path().join(STORE_FILE)).unwrap();
        assert_eq!(
            store.get("corral").unwrap().path,
            PathBuf::from("/new/path")
        );
    }

    #[test]
    fn get_path_via_load_from_reflects_a_registered_project() {
        let dir = tempfile::tempdir().unwrap();
        register_in(dir.path(), "corral", PathBuf::from("/home/x/source/corral")).unwrap();

        let store = load_from(&dir.path().join(STORE_FILE)).unwrap();
        assert_eq!(
            store.get("corral").map(|s| s.path.clone()),
            Some(PathBuf::from("/home/x/source/corral"))
        );
        assert_eq!(store.get("never-registered").map(|s| s.path.clone()), None);
    }
}
