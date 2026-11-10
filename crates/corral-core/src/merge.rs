//! Orchestrates "merge this worktree's branch into its base": rebase onto
//! base, fast-forward `base` in the main repo, and push it to `origin` if a
//! remote exists. Each step is a primitive from [`crate::git`]; this module
//! just sequences them and turns their outcomes into one [`MergeResult`].

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config;
use crate::error::Result;
use crate::git;

/// Outcome of [`merge_into_base`], serialized to the frontend as a
/// discriminated union on `outcome`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MergeResult {
    /// Fast-forwarded `base` in the main repo to this branch's (rebased) tip.
    Merged { pushed: bool },
    /// Rebase hit a conflict; already aborted, worktree left untouched.
    Conflict,
    /// Worktree has uncommitted changes — commit or stash first.
    Dirty,
    /// No base branch could be detected for this worktree.
    NoBaseBranch,
    /// The main repo wasn't checked out on `base`; needs manual intervention
    /// there rather than corral switching branches out from under whatever
    /// the user has open.
    NotOnBaseBranch { current: String, base: String },
    /// The main repo's `base` has drifted from the remote the branch was
    /// rebased onto, so a fast-forward merge isn't possible. Reconcile `base`
    /// in the main repo first (e.g. `git pull --rebase`), then merge again.
    BaseDiverged { base: String },
    /// Merged into `base` locally, but pushing `base` to `origin` failed —
    /// the merge itself succeeded, only the push is still pending.
    MergedPushFailed { error: String },
}

/// Outcome of [`rebase_worktree`], serialized to the frontend as a
/// discriminated union on `outcome`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RebaseResult {
    /// Rebased cleanly onto the detected base branch.
    Rebased,
    /// Rebase hit a conflict; already aborted, worktree left untouched.
    Conflict,
    /// Worktree has uncommitted changes — commit or stash first.
    Dirty,
    /// No base branch could be detected for this worktree.
    NoBaseBranch,
}

/// Rebase `project`/`feature`'s branch onto its base and fast-forward-merge
/// it into the main repo, pushing `base` to `origin` afterward if a remote
/// exists.
pub fn merge_into_base(project: &str, feature: &str) -> Result<MergeResult> {
    merge_into_base_under(
        &config::repo_path(project)?,
        &config::worktree_path(project, feature)?,
    )
}

/// [`merge_into_base`] with explicit repo/worktree paths, so it's testable
/// without touching `$HOME`.
fn merge_into_base_under(repo_dir: &Path, worktree_dir: &Path) -> Result<MergeResult> {
    let status = git::status(worktree_dir)?;
    if status.is_dirty() {
        return Ok(MergeResult::Dirty);
    }
    let Some(base) = git::detect_base_branch(worktree_dir) else {
        return Ok(MergeResult::NoBaseBranch);
    };
    // A rebase never renames the branch, so the branch we'll merge is the one
    // `status` just reported — no need for a second full `git::status` later.
    let branch = status.branch;

    if git::rebase_onto_base(worktree_dir, &base)? == git::RebaseOutcome::Conflict {
        return Ok(MergeResult::Conflict);
    }

    match git::fast_forward_merge(repo_dir, &base, &branch)? {
        git::MergeOutcome::NotOnBaseBranch { current } => {
            Ok(MergeResult::NotOnBaseBranch { current, base })
        }
        git::MergeOutcome::BaseDiverged => Ok(MergeResult::BaseDiverged { base }),
        git::MergeOutcome::Merged => match git::push_base_if_remote(repo_dir, &base) {
            Ok(pushed) => Ok(MergeResult::Merged { pushed }),
            // The merge already succeeded locally; a push failure is a distinct
            // outcome, not a total failure that would mislead the user into
            // thinking their branch never got merged.
            Err(e) => Ok(MergeResult::MergedPushFailed {
                error: e.to_string(),
            }),
        },
    }
}

/// Rebase `project`/`feature`'s branch onto its detected base branch. Unlike
/// [`merge_into_base`], this doesn't touch the main repo or push anywhere —
/// the user decides when to push afterward (existing ahead/behind indicators
/// and the merge/PR button already surface that a push is needed).
pub fn rebase_worktree(project: &str, feature: &str) -> Result<RebaseResult> {
    rebase_worktree_under(&config::worktree_path(project, feature)?)
}

/// [`rebase_worktree`] with an explicit worktree path, so it's testable
/// without touching `$HOME`.
fn rebase_worktree_under(worktree_dir: &Path) -> Result<RebaseResult> {
    let status = git::status(worktree_dir)?;
    if status.is_dirty() {
        return Ok(RebaseResult::Dirty);
    }
    let Some(base) = git::detect_base_branch(worktree_dir) else {
        return Ok(RebaseResult::NoBaseBranch);
    };

    match git::rebase_onto_base(worktree_dir, &base)? {
        git::RebaseOutcome::Success => Ok(RebaseResult::Rebased),
        git::RebaseOutcome::Conflict => Ok(RebaseResult::Conflict),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// A repo with one commit on `main`.
    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("committed.txt"), "one\ntwo\nthree\n").unwrap();
        run_git(dir.path(), &["add", "."]);
        run_git(dir.path(), &["commit", "--quiet", "-m", "initial"]);
        dir
    }

    /// Initializes a bare `origin` remote under a fresh tempdir, wires it up
    /// as `repo`'s `origin`, and pushes `main` so `origin/main` exists.
    /// Returns the tempdir (kept alive so the bare repo persists) and the bare
    /// repo's path — a test can delete the latter to make the remote
    /// unreachable and force a push failure.
    fn add_bare_origin(repo: &Path) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("origin.git");
        run_git(repo, &["init", "--quiet", "--bare", bare.to_str().unwrap()]);
        run_git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        run_git(repo, &["push", "--quiet", "origin", "main"]);
        (dir, bare)
    }

    #[test]
    fn merges_a_clean_ahead_branch_and_reports_no_push() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        std::fs::write(worktree.join("feature.txt"), "hello\n").unwrap();
        run_git(&worktree, &["add", "."]);
        run_git(&worktree, &["commit", "--quiet", "-m", "feature work"]);

        let result = merge_into_base_under(repo.path(), &worktree).unwrap();

        assert!(matches!(result, MergeResult::Merged { pushed: false }));
        assert_eq!(
            git::git(repo.path(), &["rev-parse", "HEAD"]).unwrap(),
            git::git(&worktree, &["rev-parse", "HEAD"]).unwrap()
        );
    }

    #[test]
    fn reports_dirty_without_touching_git_state() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        std::fs::write(
            worktree.join("committed.txt"),
            "one\ntwo\nthree\nuncommitted\n",
        )
        .unwrap();
        let head_before = git::git(&worktree, &["rev-parse", "HEAD"]).unwrap();

        let result = merge_into_base_under(repo.path(), &worktree).unwrap();

        assert!(matches!(result, MergeResult::Dirty));
        assert_eq!(
            git::git(&worktree, &["rev-parse", "HEAD"]).unwrap(),
            head_before
        );
    }

    #[test]
    fn reports_no_base_branch_when_none_is_detected() {
        let repo = init_repo();
        // No second branch, no origin/HEAD: detect_base_branch finds nothing.
        let result = merge_into_base_under(repo.path(), repo.path()).unwrap();
        assert!(matches!(result, MergeResult::NoBaseBranch));
    }

    #[test]
    fn reports_conflict_and_leaves_the_worktree_untouched() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(
            repo.path().join("committed.txt"),
            "one\ntwo\nthree\nfeature\n",
        )
        .unwrap();
        run_git(repo.path(), &["commit", "-am", "feature edit"]);
        run_git(repo.path(), &["checkout", "main"]);
        std::fs::write(
            repo.path().join("committed.txt"),
            "one\ntwo\nthree\nmain-edit\n",
        )
        .unwrap();
        run_git(repo.path(), &["commit", "-am", "main edit"]);
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &["worktree", "add", worktree.to_str().unwrap(), "feat/thing"],
        );
        let head_before = git::git(&worktree, &["rev-parse", "HEAD"]).unwrap();

        let result = merge_into_base_under(repo.path(), &worktree).unwrap();

        assert!(matches!(result, MergeResult::Conflict));
        assert_eq!(
            git::git(&worktree, &["rev-parse", "HEAD"]).unwrap(),
            head_before
        );
    }

    #[test]
    fn reports_not_on_base_branch_when_repo_is_checked_out_elsewhere() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        std::fs::write(worktree.join("feature.txt"), "hello\n").unwrap();
        run_git(&worktree, &["add", "."]);
        run_git(&worktree, &["commit", "--quiet", "-m", "feature work"]);
        run_git(repo.path(), &["checkout", "-b", "wip"]);

        let result = merge_into_base_under(repo.path(), &worktree).unwrap();

        assert!(matches!(
            result,
            MergeResult::NotOnBaseBranch { current, base }
                if current == "wip" && base == "main"
        ));
    }

    #[test]
    fn merges_and_pushes_when_a_remote_exists() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        let (_origin_dir, _bare) = add_bare_origin(repo.path());

        std::fs::write(worktree.join("feature.txt"), "hello\n").unwrap();
        run_git(&worktree, &["add", "."]);
        run_git(&worktree, &["commit", "--quiet", "-m", "feature work"]);

        let result = merge_into_base_under(repo.path(), &worktree).unwrap();

        assert!(matches!(result, MergeResult::Merged { pushed: true }));
        // The push actually landed: origin/main now matches merged main.
        assert_eq!(
            git::git(repo.path(), &["rev-parse", "origin/main"]).unwrap(),
            git::git(repo.path(), &["rev-parse", "HEAD"]).unwrap(),
        );
    }

    #[test]
    fn reports_push_failure_after_a_successful_local_merge() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        let (_origin_dir, bare) = add_bare_origin(repo.path());
        // Make the remote unreachable so the post-merge push fails — the merge
        // itself must still succeed locally.
        std::fs::remove_dir_all(&bare).unwrap();

        std::fs::write(worktree.join("feature.txt"), "hello\n").unwrap();
        run_git(&worktree, &["add", "."]);
        run_git(&worktree, &["commit", "--quiet", "-m", "feature work"]);

        let result = merge_into_base_under(repo.path(), &worktree).unwrap();

        assert!(matches!(result, MergeResult::MergedPushFailed { .. }));
        // The local merge did happen: main == the worktree's branch tip.
        assert_eq!(
            git::git(repo.path(), &["rev-parse", "HEAD"]).unwrap(),
            git::git(&worktree, &["rev-parse", "HEAD"]).unwrap(),
        );
    }

    #[test]
    fn reports_base_diverged_when_main_has_drifted_from_origin() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        let (_origin_dir, _bare) = add_bare_origin(repo.path());
        // Local main gets an unpushed commit, drifting it ahead of origin/main.
        std::fs::write(repo.path().join("local-only.txt"), "local\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "local main work"]);

        std::fs::write(worktree.join("feature.txt"), "hello\n").unwrap();
        run_git(&worktree, &["add", "."]);
        run_git(&worktree, &["commit", "--quiet", "-m", "feature work"]);

        let result = merge_into_base_under(repo.path(), &worktree).unwrap();

        assert!(matches!(
            result,
            MergeResult::BaseDiverged { base } if base == "main"
        ));
    }

    #[test]
    fn rebase_worktree_rebases_a_cleanly_diverged_branch() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        std::fs::write(worktree.join("feature.txt"), "hello\n").unwrap();
        run_git(&worktree, &["add", "."]);
        run_git(&worktree, &["commit", "--quiet", "-m", "feature work"]);
        std::fs::write(repo.path().join("later.txt"), "later\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "later main work"]);

        let result = rebase_worktree_under(&worktree).unwrap();

        assert!(matches!(result, RebaseResult::Rebased));
        // Rebased onto main's new tip: later.txt should now exist in the worktree.
        assert!(worktree.join("later.txt").exists());
    }

    #[test]
    fn rebase_worktree_reports_dirty_without_touching_git_state() {
        let repo = init_repo();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
            ],
        );
        std::fs::write(
            worktree.join("committed.txt"),
            "one\ntwo\nthree\nuncommitted\n",
        )
        .unwrap();
        let head_before = git::git(&worktree, &["rev-parse", "HEAD"]).unwrap();

        let result = rebase_worktree_under(&worktree).unwrap();

        assert!(matches!(result, RebaseResult::Dirty));
        assert_eq!(
            git::git(&worktree, &["rev-parse", "HEAD"]).unwrap(),
            head_before
        );
    }

    #[test]
    fn rebase_worktree_reports_no_base_branch_when_none_is_detected() {
        let repo = init_repo();
        // No second branch, no origin/HEAD: detect_base_branch finds nothing.
        let result = rebase_worktree_under(repo.path()).unwrap();
        assert!(matches!(result, RebaseResult::NoBaseBranch));
    }

    #[test]
    fn rebase_worktree_reports_conflict_and_leaves_the_worktree_untouched() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(
            repo.path().join("committed.txt"),
            "one\ntwo\nthree\nfeature\n",
        )
        .unwrap();
        run_git(repo.path(), &["commit", "-am", "feature edit"]);
        run_git(repo.path(), &["checkout", "main"]);
        std::fs::write(
            repo.path().join("committed.txt"),
            "one\ntwo\nthree\nmain-edit\n",
        )
        .unwrap();
        run_git(repo.path(), &["commit", "-am", "main edit"]);
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &["worktree", "add", worktree.to_str().unwrap(), "feat/thing"],
        );
        let head_before = git::git(&worktree, &["rev-parse", "HEAD"]).unwrap();

        let result = rebase_worktree_under(&worktree).unwrap();

        assert!(matches!(result, RebaseResult::Conflict));
        assert_eq!(
            git::git(&worktree, &["rev-parse", "HEAD"]).unwrap(),
            head_before
        );
    }
}
