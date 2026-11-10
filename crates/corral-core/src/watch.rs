//! Decides which filesystem events matter for a single watched worktree:
//! respects `.gitignore` for ordinary files, but always treats a
//! worktree's real `HEAD`, `index`, and `refs` paths as relevant so commits
//! and checkouts made outside the app are still detected, even though the
//! rest of `.git/` is otherwise excluded.
//!
//! A corral worktree's `.git` is almost always a *file* containing a
//! `gitdir:` pointer (e.g. `gitdir: /home/x/source/corral/.git/worktrees/foo`),
//! not a directory — so `<root>/.git/HEAD` doesn't exist for corral's
//! primary use case. [`git_metadata_paths`] resolves the real paths by
//! shelling out to `git rev-parse --git-path`, matching this crate's
//! existing convention of treating `git` itself as the source of truth
//! (see `crate::git`'s module doc) rather than reimplementing worktree
//! internals. This also correctly degrades to `<root>/.git/...` for a
//! plain, non-worktree repo.
//!
//! `is_relevant` also drops pure `Access` events regardless of path — a
//! watched path being *read* (rather than created/modified/removed) is
//! never itself a change, and since refreshing the Changes pane runs `git`
//! commands that read `HEAD`/the index, treating reads as relevant would
//! make the watcher retrigger itself indefinitely.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub const GIT_METADATA_NAMES: [&str; 3] = ["HEAD", "index", "refs"];

/// The debounce window every filesystem watcher coalesces on: a burst of
/// events (e.g. a rename's unlink + symlink, or a checkout touching many
/// files) collapses into a single refresh instead of one per event. Shared
/// by the two src-tauri watchers (`src-tauri/src/watch.rs` and
/// `src-tauri/src/worktree_watch.rs`) so their coalescing cadence stays in
/// lockstep — kept here, not duplicated per watcher.
pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

/// Builds a matcher from `root`'s `.gitignore`. Falls back to an empty
/// (never-ignore) matcher if the file is missing or fails to parse, so a
/// bad `.gitignore` makes watching noisier rather than broken.
pub fn build_gitignore(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    let _ = builder.add(root.join(".gitignore"));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// Resolves the real paths to watch for commit/checkout/branch-switch
/// activity. Best-effort: a name that fails to resolve (e.g. `git` missing,
/// or `root` isn't a git repo) is simply omitted rather than failing the
/// whole call.
pub fn git_metadata_paths(root: &Path) -> Vec<PathBuf> {
    GIT_METADATA_NAMES
        .iter()
        .filter_map(|name| {
            let out = crate::git::git(root, &["rev-parse", "--git-path", name]).ok()?;
            let path = PathBuf::from(out);
            // If the path is relative, join it with root to make it absolute
            let absolute_path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            Some(absolute_path)
        })
        .collect()
}

/// Whether a raw notify event should trigger a refresh of `root`'s changes.
pub fn is_relevant(
    event: &notify::Result<notify::Event>,
    root: &Path,
    git_paths: &[PathBuf],
    ignore: &Gitignore,
) -> bool {
    let Ok(event) = event else { return false };
    // Merely *reading* a watched path (e.g. `git status`/`git diff` opening
    // HEAD or the index, an editor's read-only preload) is not a change —
    // and treating it as one creates a feedback loop: refreshing the
    // Changes pane runs `git` commands that read HEAD/index themselves,
    // which would otherwise re-trigger this exact watcher forever.
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return false;
    }
    event
        .paths
        .iter()
        .any(|p| is_path_relevant(p, root, git_paths, ignore))
}

/// True if `path` is one of the worktree's resolved git metadata paths
/// (`HEAD`/`index`/`refs`) — i.e. a change that came from `git` itself
/// (commit, checkout, stage) rather than an ordinary working-tree edit.
fn touches_git_metadata(path: &Path, git_paths: &[PathBuf]) -> bool {
    git_paths.iter().any(|gp| path.starts_with(gp))
}

/// True if the event touches git metadata — a commit, checkout, or stage —
/// as opposed to an ordinary working-tree file edit. Callers use this to
/// decide whether a watcher event warrants a full `git::status` refresh
/// (which recomputes the expensive merge-base/merge-tree/numstat derived
/// fields); ordinary edits only need the cheaper working-tree diff.
pub fn is_git_metadata_event(event: &notify::Result<notify::Event>, git_paths: &[PathBuf]) -> bool {
    let Ok(event) = event else { return false };
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return false;
    }
    event
        .paths
        .iter()
        .any(|p| touches_git_metadata(p, git_paths))
}

fn is_path_relevant(path: &Path, root: &Path, git_paths: &[PathBuf], ignore: &Gitignore) -> bool {
    if touches_git_metadata(path, git_paths) {
        return true;
    }
    if path.starts_with(root.join(".git")) {
        return false;
    }
    let is_dir = path.is_dir();
    !ignore.matched(path, is_dir).is_ignore()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn event(path: &Path) -> notify::Result<notify::Event> {
        Ok(notify::Event::new(notify::EventKind::Any).add_path(path.to_path_buf()))
    }

    /// Real `git` integration fixture — mirrors `crate::git`'s test helper
    /// of the same shape (see `crates/corral-core/src/git.rs`'s `init_repo`).
    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "--quiet", "--initial-branch=main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        fs::write(dir.path().join("committed.txt"), "one\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "initial"]);
        dir
    }

    #[test]
    fn ordinary_file_not_ignored_is_relevant() {
        let dir = tempfile::tempdir().unwrap();
        let ignore = build_gitignore(dir.path());
        let file = dir.path().join("src/main.rs");
        assert!(is_relevant(&event(&file), dir.path(), &[], &ignore));
    }

    #[test]
    fn gitignored_file_is_not_relevant() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
        let ignore = build_gitignore(dir.path());
        let ignored = dir.path().join("target/debug/build.log");
        assert!(!is_relevant(&event(&ignored), dir.path(), &[], &ignore));
    }

    #[test]
    fn non_ignored_file_still_relevant_alongside_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        let ignore = build_gitignore(dir.path());
        let file = dir.path().join("src/lib.rs");
        assert!(is_relevant(&event(&file), dir.path(), &[], &ignore));
    }

    #[test]
    fn other_git_internals_are_not_relevant_without_resolved_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ignore = build_gitignore(dir.path());
        let objects = dir.path().join(".git/objects/ab/cd1234");
        assert!(!is_relevant(&event(&objects), dir.path(), &[], &ignore));
    }

    #[test]
    fn missing_gitignore_falls_back_to_unfiltered() {
        let dir = tempfile::tempdir().unwrap();
        let ignore = build_gitignore(dir.path());
        let file = dir.path().join("anything.txt");
        assert!(is_relevant(&event(&file), dir.path(), &[], &ignore));
    }

    #[test]
    fn access_event_is_never_relevant() {
        // Regression test: reading HEAD (e.g. as a side effect of our own
        // `git status`/`git diff` calls) must not count as a change, or the
        // watcher feeds back into itself forever. Manually verified against
        // a real running instance: git's HEAD reads generated a stream of
        // `Access(Open(Any))` events that, before this fix, were treated as
        // relevant and caused a perpetual refresh loop.
        let dir = tempfile::tempdir().unwrap();
        let ignore = build_gitignore(dir.path());
        let file = dir.path().join("src/main.rs");
        let access = Ok(notify::Event::new(notify::EventKind::Access(
            notify::event::AccessKind::Open(notify::event::AccessMode::Any),
        ))
        .add_path(file.clone()));
        assert!(!is_relevant(&access, dir.path(), &[], &ignore));
        // Sanity check: the same path with a non-Access kind is relevant.
        assert!(is_relevant(&event(&file), dir.path(), &[], &ignore));
    }

    #[test]
    fn err_event_is_never_relevant() {
        let dir = tempfile::tempdir().unwrap();
        let ignore = build_gitignore(dir.path());
        let err: notify::Result<notify::Event> = Err(notify::Error::generic("boom"));
        assert!(!is_relevant(&err, dir.path(), &[], &ignore));
    }

    #[test]
    fn git_metadata_paths_resolves_under_git_dir_for_plain_repo() {
        let repo = init_repo();
        let paths = git_metadata_paths(repo.path());
        assert_eq!(paths.len(), 3);
        let git_dir = repo.path().join(".git");
        assert!(paths.iter().all(|p| p.starts_with(&git_dir)));
        assert!(paths.iter().any(|p| p.ends_with("HEAD")));
        assert!(paths.iter().any(|p| p.ends_with("index")));
        assert!(paths.iter().any(|p| p.ends_with("refs")));
    }

    #[test]
    fn git_metadata_paths_resolves_outside_worktree_root_for_a_real_worktree() {
        let main = init_repo();
        let worktree_dir = tempfile::tempdir().unwrap();
        // `git worktree add` requires the target not to already exist.
        fs::remove_dir(worktree_dir.path()).unwrap();
        let status = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                "feature",
                worktree_dir.path().to_str().unwrap(),
            ])
            .current_dir(main.path())
            .status()
            .unwrap();
        assert!(status.success());

        // Sanity check on the bug this test guards against: the worktree's
        // own `.git` is a file, not a directory.
        assert!(worktree_dir.path().join(".git").is_file());

        let paths = git_metadata_paths(worktree_dir.path());
        assert_eq!(paths.len(), 3);
        let head = paths.iter().find(|p| p.ends_with("HEAD")).unwrap();
        let index = paths.iter().find(|p| p.ends_with("index")).unwrap();
        assert!(!head.starts_with(worktree_dir.path()));
        assert!(!index.starts_with(worktree_dir.path()));
        assert!(head.to_string_lossy().contains(".git/worktrees/"));
    }

    #[test]
    fn is_relevant_true_for_resolved_worktree_head_outside_root() {
        let main = init_repo();
        let worktree_dir = tempfile::tempdir().unwrap();
        fs::remove_dir(worktree_dir.path()).unwrap();
        std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                "feature2",
                worktree_dir.path().to_str().unwrap(),
            ])
            .current_dir(main.path())
            .status()
            .unwrap();

        let git_paths = git_metadata_paths(worktree_dir.path());
        let ignore = build_gitignore(worktree_dir.path());
        let head = git_paths.iter().find(|p| p.ends_with("HEAD")).unwrap();
        assert!(is_relevant(
            &event(head),
            worktree_dir.path(),
            &git_paths,
            &ignore
        ));
    }

    #[test]
    fn is_git_metadata_event_distinguishes_commits_from_edits() {
        let dir = tempfile::tempdir().unwrap();
        let git_paths = vec![dir.path().join(".git").join("HEAD")];

        // An ordinary file edit (non-git path) is not a git-metadata event.
        let edit = event(&dir.path().join("src/main.rs"));
        assert!(!is_git_metadata_event(&edit, &git_paths));

        // A change to a resolved git path (commit/checkout/stage) is.
        let commit = event(&dir.path().join(".git/HEAD"));
        assert!(is_git_metadata_event(&commit, &git_paths));

        // Access events never count, even on a git path (they'd otherwise
        // retrigger on our own `git status` reads of HEAD).
        let access = Ok(notify::Event::new(notify::EventKind::Access(
            notify::event::AccessKind::Open(notify::event::AccessMode::Any),
        ))
        .add_path(dir.path().join(".git/HEAD")));
        assert!(!is_git_metadata_event(&access, &git_paths));
    }
}
