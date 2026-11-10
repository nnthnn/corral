//! Corral's own on-disk roots.
//!
//! Two directories, by convention the XDG ones corral has always used:
//!
//! - **config** (`~/.config/corral`) — user-editable files: `settings.json`
//!   and `projects.json`.
//! - **data** (`~/.local/share/corral`) — app-generated working state:
//!   `sessions.json`, `worktree_notes.json`, `worktree_settings.json`, and the
//!   bundled `pi/` status extension.
//!
//! Both can be redirected wholesale with the [`CONFIG_DIR_ENV`] /
//! [`DATA_DIR_ENV`] environment variables, which lets a second instance (e.g.
//! a dev build testing a new setting) run against a throwaway profile without
//! touching the real app's files. The override is deliberately corral-scoped:
//! unlike redirecting `XDG_CONFIG_HOME`/`XDG_DATA_HOME`, it never changes where
//! the spawned agent CLIs (Claude / opencode / pi) read *their* own config —
//! `opencode.rs`, for instance, still reads the user's real opencode config.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Environment variable overriding corral's config dir (`settings.json`,
/// `projects.json`). Unset or empty falls back to `~/.config/corral`.
pub const CONFIG_DIR_ENV: &str = "CORRAL_CONFIG_DIR";

/// Environment variable overriding corral's data dir (`sessions.json`,
/// `worktree_notes.json`, `worktree_settings.json`, `pi/`). Unset or empty
/// falls back to `~/.local/share/corral`.
pub const DATA_DIR_ENV: &str = "CORRAL_DATA_DIR";

/// The directory holding corral's user-editable config files.
pub fn config_root() -> Result<PathBuf> {
    resolve_root(
        std::env::var_os(CONFIG_DIR_ENV),
        dirs::config_dir().map(|d| d.join("corral")),
    )
    .ok_or(Error::NoConfigDir)
}

/// The directory holding corral's app-generated data files.
pub fn data_root() -> Result<PathBuf> {
    resolve_root(
        std::env::var_os(DATA_DIR_ENV),
        dirs::data_dir().map(|d| d.join("corral")),
    )
    .ok_or(Error::NoDataDir)
}

/// Pure resolution behind [`config_root`] / [`data_root`]: an explicit,
/// non-empty environment override wins; otherwise the XDG-derived fallback is
/// used. Split out so the precedence is testable without mutating the
/// process environment (which is global and racy across parallel tests).
fn resolve_root(override_value: Option<OsString>, fallback: Option<PathBuf>) -> Option<PathBuf> {
    match override_value {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_root_prefers_a_non_empty_override() {
        assert_eq!(
            resolve_root(
                Some(OsString::from("/tmp/corral-dev")),
                Some(PathBuf::from("/home/alice/.config/corral")),
            ),
            Some(PathBuf::from("/tmp/corral-dev"))
        );
    }

    #[test]
    fn resolve_root_falls_back_when_unset() {
        assert_eq!(
            resolve_root(None, Some(PathBuf::from("/home/alice/.config/corral"))),
            Some(PathBuf::from("/home/alice/.config/corral"))
        );
    }

    #[test]
    fn resolve_root_falls_back_when_override_is_empty() {
        // An empty env var (e.g. `CORRAL_CONFIG_DIR=`) must mean "unset", not
        // "use the current directory".
        assert_eq!(
            resolve_root(
                Some(OsString::new()),
                Some(PathBuf::from("/home/alice/.config/corral")),
            ),
            Some(PathBuf::from("/home/alice/.config/corral"))
        );
    }

    #[test]
    fn resolve_root_can_resolve_to_nothing_when_there_is_no_fallback() {
        assert_eq!(resolve_root(None, None), None);
    }

    #[test]
    fn resolve_root_passes_a_relative_override_through_unvalidated() {
        // Resolution doesn't reject relative paths — the caller decides
        // whether the result is usable.
        assert_eq!(
            resolve_root(Some(OsString::from("relative/profile")), None),
            Some(PathBuf::from("relative/profile"))
        );
    }
}
