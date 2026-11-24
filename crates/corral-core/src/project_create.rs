//! Creating brand-new projects: an empty directory that corral initializes
//! as a git repo, ready for worktree creation.
//!
//! Corral's whole model assumes a project's main repo already has history:
//! `git worktree add` needs a commit to branch from ([`crate::worktree::create`]),
//! and the Changes/Diff/Merge/PR panes all diff against a base branch. So
//! "start a new project" means: create the directory, `git init`, and make one
//! empty commit. The agent then does all the real scaffolding in a normal
//! worktree, and every one of corral's git features works from the first
//! session — no special "not a repo yet" state to carry around.
//!
//! The functions here only touch the filesystem/git; registering the result
//! (see [`crate::project_settings::register`]) is the caller's job, so these
//! stay testable without touching the user's real registry.

use std::path::{Path, PathBuf};

use crate::config;
use crate::error::{Error, Result};
use crate::git;
use crate::project_settings;

/// Create and register a brand-new project at `parent/name`, defaulting
/// `parent` to `~/source` when empty. Returns the project's repo path.
pub fn create(name: &str, parent: &str) -> Result<PathBuf> {
    // Check before touching the filesystem: a name that's already registered
    // would otherwise leave a freshly initialized repo behind when
    // `register` (below) rejects the duplicate.
    if project_settings::get_path(name)?.is_some() {
        return Err(Error::ProjectAlreadyRegistered(name.to_string()));
    }
    let parent = resolve_parent(parent)?;
    let path = create_at(&parent, name)?;
    project_settings::register(name, path.clone())?;
    Ok(path)
}

/// Register an existing directory as a project, initializing it as a fresh git
/// repo first when it's empty and not already a repo. Returns the project name
/// (derived from the directory's basename).
///
/// This is what lets the GUI's "Add project" folder picker accept an empty
/// folder: there's nothing to import (an arbitrary folder with files would
/// need a real first commit, which is a separate feature), so an empty folder
/// is exactly a new project that just hasn't been scaffolded yet.
pub fn register_existing(path: &Path) -> Result<String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| Error::Other(format!("{}: {e}", path.display())))?;
    let name = canonical
        .file_name()
        .ok_or_else(|| Error::Other(format!("{} has no directory name", canonical.display())))?
        .to_string_lossy()
        .into_owned();
    if project_settings::get_path(&name)?.is_some() {
        return Err(Error::ProjectAlreadyRegistered(name));
    }
    if !canonical.join(".git").exists() {
        init_at(&canonical)?;
    }
    project_settings::register(&name, canonical)?;
    Ok(name)
}

/// Create a brand-new, empty git repo at `parent/name`: an empty directory
/// (created if absent) with `main` and a single empty commit. Errors if
/// `name` isn't usable as a single directory name, or if `parent/name`
/// already exists and isn't empty. Does not register the project.
pub fn create_at(parent: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    let target = parent.join(name);
    if target.exists() {
        if !target.is_dir() {
            return Err(Error::Other(format!(
                "{} already exists and isn't a directory",
                target.display()
            )));
        }
        if !is_empty_dir(&target)? {
            return Err(Error::ProjectDirNotEmpty(target));
        }
    } else {
        // `create_dir_all` rather than `create_dir` so the CLI's `--parent`
        // and the GUI's default `~/source` work on a machine that has never
        // had a source directory.
        std::fs::create_dir_all(&target)?;
    }
    init_repo(&target)?;
    Ok(target)
}

/// Initialize the *existing* empty directory `path` as a fresh git repo, with
/// `main` and an empty initial commit. Errors if `path` isn't a directory or
/// isn't empty.
pub fn init_at(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Err(Error::Other(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    if !is_empty_dir(path)? {
        return Err(Error::ProjectDirNotEmpty(path.to_path_buf()));
    }
    init_repo(path)
}

/// Resolve the `parent` argument for [`create`]: empty means the default
/// source root (`~/source`); a leading `~` is expanded to the home directory.
fn resolve_parent(parent: &str) -> Result<PathBuf> {
    let trimmed = parent.trim();
    if trimmed.is_empty() {
        return Ok(config::home_dir()?.join("source"));
    }
    config::expand_tilde_input(trimmed)
}

/// Reject anything that isn't a single, sane directory name — path
/// separators would escape `parent`, and `.`/`..` would resolve somewhere
/// unexpected. Surrounding whitespace is rejected too (rather than silently
/// trimmed) so the project name the user sees is exactly the directory name.
fn validate_name(name: &str) -> Result<()> {
    let invalid = name.is_empty()
        || name.trim() != name
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0');
    if invalid {
        return Err(Error::InvalidName(name.to_string()));
    }
    Ok(())
}

fn is_empty_dir(path: &Path) -> Result<bool> {
    Ok(std::fs::read_dir(path)?.next().is_none())
}

/// `git init` plus the one empty commit every corral worktree needs to branch
/// from. The commit is deliberately contentless: what the project actually
/// contains is the first agent's call, not corral's.
fn init_repo(path: &Path) -> Result<()> {
    git::git(path, &["init", "--quiet", "--initial-branch=main"])?;

    const COMMIT: [&str; 5] = [
        "commit",
        "--allow-empty",
        "--no-verify",
        "-m",
        "Initial commit",
    ];
    if git::git(path, &COMMIT).is_ok() {
        return Ok(());
    }

    // No `user.name`/`user.email` configured (common on a fresh machine), or
    // global signing config with no usable key. Corral's bootstrap commit must
    // not depend on any of that — it's a structural placeholder, not authored
    // work — so retry with a throwaway identity and signing off.
    let fallback: Vec<&str> = [
        "-c",
        "user.name=Corral",
        "-c",
        "user.email=corral@localhost",
        "-c",
        "commit.gpgsign=false",
    ]
    .into_iter()
    .chain(COMMIT)
    .collect();
    git::git(path, &fallback).inspect_err(|e| {
        tracing::warn!("project_create: could not create the initial commit in {path:?}: {e}");
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_count(path: &Path) -> String {
        git::git(path, &["rev-list", "--count", "HEAD"]).unwrap()
    }

    #[test]
    fn create_at_makes_a_repo_with_one_empty_commit_on_main() {
        let parent = tempfile::tempdir().unwrap();

        let path = create_at(parent.path(), "myapp").unwrap();

        assert_eq!(path, parent.path().join("myapp"));
        assert!(path.is_dir());
        assert_eq!(
            git::git(&path, &["branch", "--show-current"]).unwrap(),
            "main"
        );
        assert_eq!(commit_count(&path), "1");
        // The initial commit carries no files, so the tree is clean — the
        // agent's scaffolding is the first real content.
        assert!(
            git::git(&path, &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn create_at_creates_missing_parent_directories() {
        let parent = tempfile::tempdir().unwrap();
        let nested = parent.path().join("a").join("b");

        let path = create_at(&nested, "myapp").unwrap();

        assert!(path.join(".git").exists());
    }

    #[test]
    fn create_at_accepts_an_existing_empty_directory() {
        let parent = tempfile::tempdir().unwrap();
        let existing = parent.path().join("myapp");
        std::fs::create_dir(&existing).unwrap();

        let path = create_at(parent.path(), "myapp").unwrap();

        assert_eq!(path, existing);
        assert_eq!(commit_count(&path), "1");
    }

    #[test]
    fn create_at_rejects_a_non_empty_directory_without_touching_it() {
        let parent = tempfile::tempdir().unwrap();
        let existing = parent.path().join("myapp");
        std::fs::create_dir(&existing).unwrap();
        std::fs::write(existing.join("notes.txt"), "mine\n").unwrap();

        let result = create_at(parent.path(), "myapp");

        assert!(matches!(result, Err(Error::ProjectDirNotEmpty(_))));
        assert!(!existing.join(".git").exists());
        assert_eq!(
            std::fs::read_to_string(existing.join("notes.txt")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn create_at_rejects_names_that_are_not_a_single_directory_name() {
        let parent = tempfile::tempdir().unwrap();
        for name in ["", " ", " myapp", "myapp ", "a/b", "a\\b", ".", ".."] {
            assert!(
                matches!(create_at(parent.path(), name), Err(Error::InvalidName(_))),
                "expected {name:?} to be rejected"
            );
        }
    }

    #[test]
    fn init_at_initializes_an_existing_empty_directory() {
        let dir = tempfile::tempdir().unwrap();

        init_at(dir.path()).unwrap();

        assert_eq!(
            git::git(dir.path(), &["branch", "--show-current"]).unwrap(),
            "main"
        );
        assert_eq!(commit_count(dir.path()), "1");
    }

    #[test]
    fn init_at_rejects_a_non_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "mine\n").unwrap();

        assert!(matches!(
            init_at(dir.path()),
            Err(Error::ProjectDirNotEmpty(_))
        ));
        assert!(!dir.path().join(".git").exists());
    }

    #[test]
    fn init_at_rejects_a_path_that_is_not_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();

        assert!(init_at(&file).is_err());
    }
}
