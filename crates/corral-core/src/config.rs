//! Filesystem layout conventions for worktrees. A project's own repo can
//! live anywhere on disk
//! — see `crate::project_settings`'s registry, which is the source of
//! truth for a project's path — but every worktree of it is created under
//! one shared root:
//!
//! ```text
//! <worktrees_root>/              ← ~/corral by default, configurable via
//!                                   Settings.worktrees_root
//!   <project>/
//!     <feature>/                ← git worktree on feat/<feature>
//! ```

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The current user's home directory.
pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or(Error::NoHomeDir)
}

/// Root under which worktrees are organized. Defaults to `~/corral`;
/// overridable via `Settings.worktrees_root` (see `crate::settings`).
pub fn worktrees_root() -> Result<PathBuf> {
    let configured = crate::settings::load()
        .map(|s| s.worktrees_root)
        .unwrap_or_default();
    Ok(resolve_worktrees_root(&configured, &home_dir()?))
}

/// Pure resolution logic behind [`worktrees_root`], testable without
/// touching `$HOME` or the real settings file.
fn resolve_worktrees_root(configured: &str, home: &Path) -> PathBuf {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        return home.join("corral");
    }
    expand_tilde(trimmed, home)
}

/// Expands a leading `~` or `~/...` to `home`. Any other input (including
/// relative paths) is returned as-is — validating that the input is usable
/// is the caller's job (the CLI `set` command, the GUI settings form), not
/// this function's.
fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => PathBuf::from(path),
    }
}

/// Main repository path for a project — looked up in the registered-
/// projects store (`crate::project_settings`), since a project's repo can
/// live anywhere on disk, not just under a fixed convention. Errors with
/// [`Error::ProjectNotRegistered`] if `project` hasn't been added (via the
/// GUI's Add Project or `corral project add <path>`).
pub fn repo_path(project: &str) -> Result<PathBuf> {
    crate::project_settings::get_path(project)?
        .ok_or_else(|| Error::ProjectNotRegistered(project.to_string()))
}

/// Worktree path for a project + feature (`<worktrees_root>/<project>/<feature>`).
pub fn worktree_path(project: &str, feature: &str) -> Result<PathBuf> {
    Ok(worktrees_root()?.join(project).join(feature))
}

/// Branch name convention: `feat/<feature>`.
pub fn branch_name(feature: &str) -> String {
    format!("feat/{feature}")
}

/// Infer `(project, feature)` from a path inside
/// `<worktrees_root>/<project>/<feature>[/...]` — lets an agent running in
/// its own worktree identify itself from `cwd` alone.
pub fn resolve_worktree_from_path(path: &Path) -> Result<(String, String)> {
    resolve_worktree_from_path_under(&worktrees_root()?, path)
}

/// [`resolve_worktree_from_path`] with an explicit worktrees root, so the
/// path-parsing logic is testable without touching `$HOME`.
fn resolve_worktree_from_path_under(root: &Path, path: &Path) -> Result<(String, String)> {
    let not_found = || Error::WorktreeNotFound(path.display().to_string());

    let root = std::fs::canonicalize(root).map_err(|_| not_found())?;
    let canon_path = std::fs::canonicalize(path).map_err(|_| not_found())?;
    let rel = canon_path.strip_prefix(&root).map_err(|_| not_found())?;

    let mut components = rel.components();
    let project = components
        .next()
        .ok_or_else(not_found)?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    let feature = components
        .next()
        .ok_or_else(not_found)?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    Ok((project, feature))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_name_uses_feat_prefix() {
        assert_eq!(branch_name("churning"), "feat/churning");
    }

    #[test]
    fn resolve_worktree_from_path_under_extracts_project_and_feature() {
        let root = tempfile::tempdir().unwrap();
        let worktree = root.path().join("corral").join("weeding");
        std::fs::create_dir_all(&worktree).unwrap();

        let (project, feature) = resolve_worktree_from_path_under(root.path(), &worktree).unwrap();
        assert_eq!(project, "corral");
        assert_eq!(feature, "weeding");
    }

    #[test]
    fn resolve_worktree_from_path_under_handles_nested_subpath() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("corral").join("weeding").join("src");
        std::fs::create_dir_all(&nested).unwrap();

        let (project, feature) = resolve_worktree_from_path_under(root.path(), &nested).unwrap();
        assert_eq!(project, "corral");
        assert_eq!(feature, "weeding");
    }

    #[test]
    fn resolve_worktree_from_path_under_rejects_path_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        let result = resolve_worktree_from_path_under(root.path(), outside.path());
        assert!(matches!(result, Err(Error::WorktreeNotFound(_))));
    }

    #[test]
    fn resolve_worktree_from_path_under_rejects_root_itself() {
        let root = tempfile::tempdir().unwrap();

        // The root has no project/feature components below it.
        let result = resolve_worktree_from_path_under(root.path(), root.path());
        assert!(matches!(result, Err(Error::WorktreeNotFound(_))));
    }

    #[test]
    fn resolve_worktree_from_path_under_rejects_nonexistent_path() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("corral").join("does-not-exist");

        let result = resolve_worktree_from_path_under(root.path(), &missing);
        assert!(matches!(result, Err(Error::WorktreeNotFound(_))));
    }

    #[test]
    fn resolve_worktrees_root_defaults_to_home_corral_when_empty() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_worktrees_root("", home),
            PathBuf::from("/home/alice/corral")
        );
    }

    #[test]
    fn resolve_worktrees_root_defaults_when_whitespace_only() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_worktrees_root("   ", home),
            PathBuf::from("/home/alice/corral")
        );
    }

    #[test]
    fn resolve_worktrees_root_expands_leading_tilde() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_worktrees_root("~/source/worktrees", home),
            PathBuf::from("/home/alice/source/worktrees")
        );
    }

    #[test]
    fn resolve_worktrees_root_expands_bare_tilde() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_worktrees_root("~", home),
            PathBuf::from("/home/alice")
        );
    }

    #[test]
    fn resolve_worktrees_root_passes_through_absolute_path() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_worktrees_root("/mnt/data/worktrees", home),
            PathBuf::from("/mnt/data/worktrees")
        );
    }

    #[test]
    fn resolve_worktrees_root_passes_through_relative_path() {
        // Resolution itself doesn't reject relative input — validating
        // that the input is usable is the caller's job (CLI `set`, GUI form).
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_worktrees_root("relative/path", home),
            PathBuf::from("relative/path")
        );
    }
}
