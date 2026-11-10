//! Thin wrappers over the `git` CLI. We shell out (rather than link libgit2)
//! to keep the dependency surface small.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Run `git <args>` in `dir`, returning trimmed stdout on success.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    Ok(git_untrimmed(dir, args)?.trim().to_string())
}

/// Like [`git`], but preserves leading/trailing whitespace in stdout. Needed
/// for line-oriented, column-based output like `git status --porcelain=v1`,
/// where the first column can be a literal space that's semantically
/// significant (unstaged vs staged) — `.trim()` would eat it and corrupt the
/// first line's parsing.
fn git_untrimmed(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(dir).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::Git {
            command: format!("git {}", args.join(" ")),
            stderr,
        });
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Whether `dir`'s repo has an `origin` remote configured.
pub fn has_remote(dir: &Path) -> bool {
    git(dir, &["remote", "get-url", "origin"]).is_ok()
}

/// Best-effort `git fetch origin` — updates remote-tracking refs (e.g.
/// `origin/main`) without touching any local branch, so it's safe to call
/// from a worktree even though the base branch may be checked out elsewhere
/// (the main repo). Logs and swallows failure (e.g. offline) instead of
/// propagating, matching this file's other best-effort helpers.
pub fn fetch_remote(dir: &Path) -> bool {
    if !has_remote(dir) {
        return false;
    }
    match git_untrimmed(dir, &["fetch", "origin"]) {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("fetch_remote: git fetch origin failed in {dir:?}: {e}");
            false
        }
    }
}

/// Outcome of [`rebase_onto_base`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebaseOutcome {
    /// Rebase completed cleanly.
    Success,
    /// Rebase hit a conflict; already aborted (`git rebase --abort`), so
    /// `dir` is back at its pre-rebase state.
    Conflict,
}

/// Resolve `base`'s effective merge target: prefer `origin/<base>` if that
/// ref exists locally, else fall back to the local `<base>` branch. Shared
/// by [`rebase_onto_base`] (which then actually rebases onto it),
/// `conflicts_with_base` (which only simulates the merge), and
/// `worktree::create_under` (which branches a new worktree off it).
pub(crate) fn resolve_onto_ref(dir: &Path, base: &str) -> String {
    let remote_ref = format!("origin/{base}");
    if git(dir, &["rev-parse", "--verify", "--quiet", &remote_ref]).is_ok() {
        remote_ref
    } else {
        base.to_string()
    }
}

/// Best-effort `git fetch origin` in `dir`, then resolve `dir`'s currently
/// checked-out branch to `origin/<branch>` if that ref exists, else the
/// local branch itself (i.e. [`resolve_onto_ref`] applied to `HEAD`'s own
/// branch). Used as the start point for a new worktree's branch, so a fresh
/// worktree is based on the latest fetched code instead of whatever `dir`
/// happened to have checked out locally. Falls back to `HEAD` itself (not
/// just the local branch name) if the branch name can't be determined, e.g.
/// a detached `HEAD`.
pub fn fetch_and_resolve_head(dir: &Path) -> String {
    fetch_remote(dir);
    match git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(branch) if branch != "HEAD" => resolve_onto_ref(dir, &branch),
        _ => "HEAD".to_string(),
    }
}

/// Rebase `dir`'s current branch onto `origin/<base>` if a remote exists and
/// that ref is present after a best-effort fetch, else onto local `<base>`.
/// On conflict, aborts the rebase automatically so `dir` is never left
/// mid-rebase — see [`RebaseOutcome::Conflict`].
pub fn rebase_onto_base(dir: &Path, base: &str) -> Result<RebaseOutcome> {
    fetch_remote(dir);
    let onto = resolve_onto_ref(dir, base);

    let output = Command::new("git")
        .args(["rebase", &onto])
        .current_dir(dir)
        .output()?;
    if output.status.success() {
        return Ok(RebaseOutcome::Success);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    // A failed rebase is only a conflict if git actually started one (and
    // left `dir` mid-rebase); otherwise it failed *before* starting — e.g. a
    // dirty working tree or an unparsable ref — and that real error should
    // surface instead of being reported as a conflict. `git rebase --abort`
    // succeeds iff there was a rebase in progress, so it doubles as the probe.
    match git(dir, &["rebase", "--abort"]) {
        Ok(_) => Ok(RebaseOutcome::Conflict),
        Err(abort_err) => {
            tracing::warn!("rebase_onto_base: git rebase {onto} failed in {dir:?}: {stderr}");
            tracing::warn!("rebase_onto_base: git rebase --abort failed in {dir:?}: {abort_err}");
            Err(Error::Git {
                command: format!("git rebase {onto}"),
                stderr,
            })
        }
    }
}

/// Outcome of [`fast_forward_merge`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MergeOutcome {
    /// Fast-forwarded `base` to `branch`'s tip.
    Merged,
    /// `repo_dir` wasn't checked out on `base` — merging would require
    /// switching branches there, which risks disrupting whatever the user
    /// has open in the main repo, so this is surfaced instead of done
    /// automatically.
    NotOnBaseBranch { current: String },
    /// `base` is checked out in `repo_dir`, but `branch` is not a fast-forward
    /// descendant of it — `base` has drifted from the ref `branch` was rebased
    /// onto (typically `origin/<base>`). A fast-forward merge is impossible
    /// without first reconciling `base` in the main repo, which corral
    /// deliberately leaves to the user rather than doing automatically.
    BaseDiverged,
}

/// Fast-forward `base` to `branch`'s tip in `repo_dir` (the *main* repo, not
/// the worktree — `base` must already be checked out there). Returns
/// [`MergeOutcome::BaseDiverged`] instead of a raw git error when `branch`
/// isn't a fast-forward descendant of `base`, which can happen when `base`
/// has local commits that the rebased `branch` doesn't share.
pub fn fast_forward_merge(repo_dir: &Path, base: &str, branch: &str) -> Result<MergeOutcome> {
    let current = git(repo_dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if current != base {
        return Ok(MergeOutcome::NotOnBaseBranch { current });
    }
    // `git merge --ff-only` can only fast-forward when `base` is an ancestor
    // of `branch`. Only short-circuit on a *confirmed* non-ancestor; an
    // unresolvable ref falls through to the real merge, whose error is more
    // informative than a guessed "diverged".
    if matches!(is_ancestor(repo_dir, base, branch), Ok(false)) {
        return Ok(MergeOutcome::BaseDiverged);
    }
    git(repo_dir, &["merge", "--ff-only", branch])?;
    Ok(MergeOutcome::Merged)
}

/// Whether `ancestor` is an ancestor of (or equal to) `descendant`, via
/// `git merge-base --is-ancestor`. Errors when either ref doesn't resolve, so
/// callers can distinguish "no" from "can't tell".
fn is_ancestor(dir: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(dir)
        .output()?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(Error::Git {
            command: format!("git merge-base --is-ancestor {ancestor} {descendant}"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
    }
}

/// Push `base` to `origin` if `repo_dir` has a remote configured. Returns
/// whether it pushed.
pub fn push_base_if_remote(repo_dir: &Path, base: &str) -> Result<bool> {
    if !has_remote(repo_dir) {
        return Ok(false);
    }
    git(repo_dir, &["push", "origin", base])?;
    Ok(true)
}

/// Force-with-lease push `branch` to `origin`, setting it as the upstream if
/// it isn't tracking one yet. Callers are expected to have already confirmed
/// a remote exists (e.g. via `GitStatus.has_remote`) — unlike
/// `push_base_if_remote`, this doesn't check first, since every caller of
/// this function has just rebased onto `origin/<base>` and needs the force
/// (the rebase rewrites commits the remote already has).
pub fn force_push_branch(dir: &Path, branch: &str) -> Result<()> {
    git(dir, &["push", "--force-with-lease", "-u", "origin", branch])?;
    Ok(())
}

/// Summary of a worktree's git state for the dashboard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitStatus {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
    pub added: u32,
    pub removed: u32,
    pub conflicts: u32,
    /// Commits on `HEAD` not reachable from the detected base branch's ref —
    /// `origin/<base>` when that exists, else local `<base>` (see
    /// [`resolve_onto_ref`]) — i.e. this branch's local, not-yet-merged
    /// history. `0` if no base branch could be detected (see
    /// [`detect_base_branch`]).
    pub commits_ahead_of_base: u32,
    /// True once this branch's commits have been folded into the base
    /// branch, whether that happened just now or previously. See
    /// [`is_merged_into_base`] for how it tells that apart from a branch
    /// that simply never diverged from base.
    pub merged_into_base: bool,
    /// True if rebasing `HEAD` onto its detected base branch would hit a
    /// conflict, per a non-destructive `git merge-tree` simulation — no
    /// working-tree mutation, unlike actually attempting
    /// [`rebase_onto_base`]. Only as fresh as the last fetch of the base's
    /// remote-tracking ref — this function performs no fetch of its own, so
    /// it stays cheap enough to run on every [`status`] call; see
    /// `conflict_poll` (src-tauri) for what keeps that ref current. `false`
    /// if no base branch could be detected.
    pub conflicts_with_base: bool,
    /// Whether this repo has an `origin` remote configured (see
    /// [`has_remote`]) — lets callers (e.g. the frontend's merge-button
    /// state machine) branch into the push+PR flow without a second,
    /// separate check.
    pub has_remote: bool,
    /// Subject line of `HEAD`'s most recent commit, empty if there is no
    /// commit yet (a brand-new repo).
    pub last_commit_message: String,
    /// Committer-date Unix timestamp (seconds) of `HEAD`'s most recent
    /// commit, `0` if there is no commit yet.
    pub last_commit_time: i64,
    /// Total lines added/removed on this branch relative to its detected
    /// base (merge-base to `HEAD`, matching [`branch_changes`]'s diff),
    /// including uncommitted edits — `0`/`0` if no base branch could be
    /// detected. Distinct from `added`/`removed` above, which only count
    /// uncommitted changes.
    pub base_added: u32,
    pub base_removed: u32,
}

impl GitStatus {
    pub fn is_dirty(&self) -> bool {
        self.staged + self.modified + self.untracked + self.conflicts > 0
    }
}

/// How far a changed file's *committed* copy has traveled toward the upstream
/// branch.
///
/// This deliberately describes only what's committed. Whether the file also
/// has uncommitted working-tree edits is tracked separately by
/// [`ChangedFile::dirty`], so the two stay independent — an already-pushed file
/// that gets edited again still reads as [`SyncState::Pushed`] (with
/// `dirty: true`) instead of collapsing to "uncommitted" and losing the fact
/// that it's part of the branch's shipped work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// No committed copy at `HEAD` — a brand-new/untracked file, one staged
    /// but never committed, or a conflicted path mid-merge/rebase.
    Uncommitted,
    /// Committed locally, but the branch has no upstream, or the commit
    /// hasn't reached it yet.
    Committed,
    /// Committed and identical to the upstream branch's copy of the file.
    Pushed,
}

/// A single changed file in the working tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    /// One of: modified, added, deleted, renamed, copied, untracked.
    pub status: String,
    /// The file's path before the rename/copy, if `status` is "renamed" or
    /// "copied". `None` otherwise.
    pub old_path: Option<String>,
    /// True if the change is staged.
    pub staged: bool,
    /// The state of the file's committed copy relative to the branch's
    /// upstream. Always `Uncommitted` for `changed_files()`; computed by
    /// cross-referencing the committed branch diff and the upstream branch for
    /// `branch_changes()`, since that also includes already-committed (and
    /// possibly already-pushed) changes since the branch's fork point.
    pub sync_state: SyncState,
    /// True if the file has uncommitted working-tree changes (staged,
    /// unstaged, or untracked). Orthogonal to `sync_state`, so a file that's
    /// already committed/pushed but edited again keeps its `sync_state` *and*
    /// reports `dirty` — the Changes pane marks it with an extra dot rather
    /// than downgrading it to plain "uncommitted".
    pub dirty: bool,
    /// Lines added/removed, from `git diff --numstat`. Both 0 for binary
    /// files (numstat reports "-" there) or if counts couldn't be computed.
    pub added: u32,
    pub removed: u32,
}

/// List working-tree changes for `dir` via `git status --porcelain=v1`.
///
/// Uses `--untracked-files=all` so new untracked directories are expanded to
/// their individual files instead of collapsed into one `dir/` entry (git's
/// default "normal" mode) — otherwise the Changes pane undercounts and hides
/// files inside newly-created directories.
pub fn changed_files(dir: &Path) -> Result<Vec<ChangedFile>> {
    let out = git_untrimmed(dir, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    let mut files = Vec::new();

    for line in out.lines() {
        if line.len() < 4 {
            continue;
        }
        let bytes = line.as_bytes();
        let (x, y) = (bytes[0] as char, bytes[1] as char);

        // Path starts at column 3; renames/copies are "old -> new". Only
        // renames/copies use that arrow separator — a modified/added/etc.
        // file whose *name* contains " -> " (e.g. `a -> b.txt`, which git
        // quotes because of the spaces) must not be mis-split into a bogus
        // rename with a stray quote left on each side.
        let mut path = line[3..].to_string();
        let mut old_path = None;
        if (x == 'R' || x == 'C')
            && let Some((old, new)) = split_rename_arrow(&path)
        {
            old_path = Some(unquote_path(old));
            path = new.to_string();
        }
        path = unquote_path(&path);

        let (status, staged) = classify(x, y);
        files.push(ChangedFile {
            path,
            status: status.to_string(),
            old_path,
            staged,
            sync_state: SyncState::Uncommitted,
            dirty: true,
            added: 0,
            removed: 0,
        });
    }

    // git lists status paths lexicographically; sort naturally so numbered
    // files (`icon-2.png`) precede `icon-10.png`.
    files.sort_by(|a, b| crate::natural::compare(&a.path, &b.path));

    Ok(files)
}

/// Strip the quotes git adds around paths with unusual characters, and
/// C-unescape what's inside them. Git quotes a path whenever it contains a
/// control char, `"`, `\`, or a byte with the high bit set, escaping `"` as
/// `\"`, `\` as `\\`, tab/newline/CR as `\t`/`\n`/`\r`, and everything else
/// as three-digit octal (`\346\227\245` for a UTF-8 byte). Without the
/// unescape, such names come through with their raw backslash sequences
/// instead of the real characters.
fn unquote_path(path: &str) -> String {
    let inner = if path.len() >= 2 && path.starts_with('"') && path.ends_with('"') {
        &path[1..path.len() - 1]
    } else {
        path
    };
    c_unescape(inner)
}

/// Reverse git's C-style escaping (see [`unquote_path`]). Byte-oriented so
/// octal escapes reconstruct arbitrary bytes; the result is decoded lossily,
/// matching `git_diff`'s `from_utf8_lossy` convention for non-UTF-8 names.
fn c_unescape(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut it = s.bytes().peekable();
    while let Some(b) = it.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        match it.next() {
            Some(b'"') => out.push(b'"'),
            Some(b'\\') => out.push(b'\\'),
            Some(b'a') => out.push(0x07),
            Some(b'b') => out.push(0x08),
            Some(b'f') => out.push(0x0C),
            Some(b'v') => out.push(0x0B),
            Some(b't') => out.push(b'\t'),
            Some(b'n') => out.push(b'\n'),
            Some(b'r') => out.push(b'\r'),
            // Octal escape: the first digit is already consumed; take up to
            // two more so `\377` is the full byte range.
            Some(first @ b'0'..=b'7') => {
                let mut val = (first - b'0') as u32;
                for _ in 0..2 {
                    match it.peek() {
                        Some(&d @ b'0'..=b'7') => {
                            it.next();
                            val = val * 8 + (d - b'0') as u32;
                        }
                        _ => break,
                    }
                }
                out.push(val as u8);
            }
            // Unknown/truncated escape: keep it verbatim rather than guessing.
            Some(other) => {
                out.push(b'\\');
                out.push(other);
            }
            None => out.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Split a porcelain-v1 rename/copy path (`old -> new`) at the separator
/// arrow. `old` and `new` are each independently C-quoted by git when they
/// contain special characters, but the ` -> ` separator itself is never
/// quoted. So the correct split point is the first ` -> ` outside a quoted
/// region — a filename that *literally* contains ` -> ` (e.g. `"a -> b.txt"`)
/// is always quoted (spaces trigger quoting), which lets us skip its internal
/// arrow and find the real separator.
fn split_rename_arrow(raw: &str) -> Option<(&str, &str)> {
    let bytes = raw.as_bytes();
    let mut i = 0;
    let mut in_quotes = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_quotes {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_quotes = false;
            }
        } else if b == b'"' {
            in_quotes = true;
        } else if raw[i..].starts_with(" -> ") {
            return Some((&raw[..i], &raw[i + 4..]));
        }
        i += 1;
    }
    None
}

fn classify(x: char, y: char) -> (&'static str, bool) {
    if x == '?' {
        return ("untracked", false);
    }
    // Unmerged (conflicted) paths are exactly these seven XY codes — "UU",
    // "AA", "DD", "AU", "UD", "UA", "DU". Only "AA"/"DD" carry no 'U'; the
    // rest do. "AD" ("added to index, then deleted from the working tree")
    // is a *normal*, non-conflicted state and must fall through to the
    // regular classification below rather than being swept up as a conflict.
    let unmerged = matches!(
        (x, y),
        ('A', 'A') | ('D', 'D') | ('A', 'U') | ('U', 'A') | ('D', 'U') | ('U', 'D') | ('U', 'U')
    );
    if unmerged {
        return ("conflict", true);
    }
    let staged = x != ' ';
    let code = if x != ' ' { x } else { y };
    let status = match code {
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        _ => "modified",
    };
    (status, staged)
}

/// Run a `git diff` invocation, tolerating exit code 1 ("differences found").
/// Output is not trimmed (whitespace is significant) and decoded lossily so
/// binary/non-UTF-8 hunks don't error.
fn git_diff(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(dir).output()?;
    match output.status.code() {
        Some(0) | Some(1) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        _ => Err(Error::Git {
            command: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
    }
}

/// Best-effort guess of the branch this worktree forked from: `origin/HEAD`'s
/// target if set, else the first of main / master / develop that exists and
/// isn't the current branch.
pub fn detect_base_branch(dir: &Path) -> Option<String> {
    let head = match git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(head) => head,
        Err(e) => {
            tracing::warn!("detect_base_branch: failed to read HEAD in {dir:?}: {e}");
            return None;
        }
    };

    if let Ok(sym) = git(
        dir,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    ) && let Some(name) = sym.rsplit('/').next()
        && name != head
    {
        return Some(name.to_string());
    }
    for cand in ["main", "master", "develop"] {
        if cand != head && git(dir, &["rev-parse", "--verify", "--quiet", cand]).is_ok() {
            return Some(cand.to_string());
        }
    }
    None
}

/// Commits `HEAD` has beyond `base` that `base` doesn't have — this branch's
/// unmerged history relative to the base ref (callers resolve to
/// `origin/<base>` when it exists via [`resolve_onto_ref`], matching the diff
/// base). Distinct from `GitStatus.ahead`, which counts against the current
/// branch's own upstream tracking ref.
fn commits_ahead_of_base(dir: &Path, base: &str) -> u32 {
    git(dir, &["rev-list", "--count", &format!("{base}..HEAD")])
        .ok()
        .and_then(|out| out.parse().ok())
        .unwrap_or(0)
}

/// Whether rebasing `dir`'s `HEAD` onto `base_ref` would hit a conflict, via
/// a non-destructive `git merge-tree` simulation — no working-tree or ref
/// mutation, unlike [`rebase_onto_base`]. Treats a non-conflict, non-clean
/// exit (e.g. an unparsable ref) as "no conflict" rather than erroring,
/// matching this module's best-effort philosophy for derived status fields.
fn conflicts_with_base(dir: &Path, base_ref: &str) -> bool {
    let output = match Command::new("git")
        .args(["merge-tree", "--write-tree", base_ref, "HEAD"])
        .current_dir(dir)
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            tracing::warn!("conflicts_with_base: failed to run git merge-tree in {dir:?}: {e}");
            return false;
        }
    };
    match output.status.code() {
        Some(0) => false,
        Some(1) => true,
        _ => {
            tracing::warn!(
                "conflicts_with_base: unexpected git merge-tree exit in {dir:?}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            false
        }
    }
}

/// True once this branch has been folded into `base`.
///
/// `git merge-base --is-ancestor HEAD base` alone can't distinguish "this
/// branch was merged" from "this branch never diverged from base" — a branch
/// that never diverged is an ancestor of `base` too, and stays one forever as
/// `base` moves on without it, so ancestry alone (even "HEAD is a *strict*
/// ancestor of base") is never proof of a merge on its own. Real evidence
/// takes one of two forms, checked regardless of whether `HEAD` and `base`
/// currently sit at the same commit:
/// - a merge commit reachable from `base` that names `HEAD` as a parent
///   (`base_has_merge_of`) — covers `git merge --no-ff`;
/// - a "Fast-forward" entry for `branch` in `base`'s reflog
///   (`base_has_ff_merge_reflog_entry`) — covers `git merge --ff-only`
///   (whether run through corral's merge button or a terminal), which
///   collapses `HEAD` and `base` onto the identical commit and so leaves no
///   merge commit to find. Reflog entries persist even after `base` advances
///   further, so this still fires once other work has landed on `base`
///   since. Only a branch that never diverged (and was never merged) has
///   neither signal.
pub fn is_merged_into_base(dir: &Path, base: &str, branch: &str) -> bool {
    let is_ancestor = Command::new("git")
        .args(["merge-base", "--is-ancestor", "HEAD", base])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !is_ancestor {
        return false;
    }
    let Ok(head) = git(dir, &["rev-parse", "HEAD"]) else {
        return false;
    };
    base_has_merge_of(dir, base, &head) || base_has_ff_merge_reflog_entry(dir, base, branch)
}

/// True if some commit reachable from `base` but not from `head` (i.e. added
/// after `head` last diverged) is a merge commit naming `head` as a parent —
/// direct proof `head`'s history was folded into `base` via a real merge
/// commit, as opposed to `head` merely being an old ancestor `base` has
/// since moved past without ever merging it.
fn base_has_merge_of(dir: &Path, base: &str, head: &str) -> bool {
    let Ok(log) = git_untrimmed(
        dir,
        &["log", &format!("{head}..{base}"), "--merges", "--format=%P"],
    ) else {
        return false;
    };
    log.lines()
        .any(|parents| parents.split_whitespace().any(|p| p == head))
}

/// Best-effort scan of `base`'s reflog for a recorded fast-forward merge of
/// `branch` — the only signal left once `HEAD` and `base` sit at the
/// identical commit (see [`is_merged_into_base`]). Worktrees share the reflog
/// with the main repo, so this works from the worktree dir even though
/// `base` is checked out elsewhere. Returns `false` (rather than erroring) if
/// the reflog is missing/expired — indistinguishable, at that point, from a
/// branch that never diverged.
///
/// Capped at the 100 most recent entries (`-n 100`): the merge entry of
/// interest is always recent, and a long-lived busy repo's reflog can hold
/// 100k+ entries that we'd otherwise read on every status poll.
fn base_has_ff_merge_reflog_entry(dir: &Path, base: &str, branch: &str) -> bool {
    let Ok(log) = git_untrimmed(dir, &["reflog", "show", "--format=%gs", "-n", "100", base]) else {
        return false;
    };
    let needle = format!("merge {branch}:");
    log.lines()
        .any(|line| line.starts_with(&needle) && line.contains("Fast-forward"))
}

/// The current branch's upstream tracking ref (e.g. `origin/feat/x`), or
/// `None` if it has none configured — i.e. it has never been pushed with
/// `-u`/`--set-upstream`.
fn upstream_ref(dir: &Path) -> Option<String> {
    git(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
}

/// Paths with committed changes in `HEAD` that haven't reached the branch's
/// upstream yet — i.e. committed but not yet pushed. `None` if the branch has
/// no upstream configured at all (skip the pushed/unpushed distinction).
///
/// Uses the `upstream...HEAD` range (diff from the merge-base, not the
/// upstream tip) so the classification is right in every case: a branch
/// that's merely *behind* its upstream, or that has been rebased/amended
/// locally without a force-push yet, has commits whose changes were
/// introduced since the fork point and are genuinely still unpushed — whereas
/// a two-commit `git diff <upstream> HEAD` compares tips directly and would
/// report them as "pushed" (or, when behind, mislabel the remote's own
/// changes as unpushed).
fn unpushed_files(dir: &Path) -> Option<std::collections::HashSet<String>> {
    let upstream = upstream_ref(dir)?;
    let range = format!("{upstream}...HEAD");
    let out = match git(dir, &["diff", "--name-only", &range]) {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!("unpushed_files: diff against upstream {upstream:?} failed: {e}");
            return None;
        }
    };
    Some(out.lines().map(|l| l.to_string()).collect())
}

fn parse_name_status(out: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    for line in out.lines() {
        let mut parts = line.split('\t');
        let Some(code) = parts.next() else { continue };
        let c = code.chars().next().unwrap_or('M');
        // Renames/copies are "R<score>\told\tnew".
        let (old_path, path) = if c == 'R' || c == 'C' {
            let Some(old) = parts.next() else { continue };
            let Some(new) = parts.next() else { continue };
            (Some(old.to_string()), new)
        } else {
            let Some(path) = parts.next() else { continue };
            (None, path)
        };
        let status = match c {
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            'C' => "copied",
            _ => "modified",
        };
        files.push(ChangedFile {
            path: path.to_string(),
            status: status.to_string(),
            old_path,
            staged: false,
            sync_state: SyncState::Committed,
            dirty: false,
            added: 0,
            removed: 0,
        });
    }
    files
}

/// Resolve a `git diff --numstat` path column to the final (new) path,
/// unwrapping git's rename shorthand: `old => new` or `dir/{old => new}/f`.
///
/// `known_paths` is the set of real new paths the caller already has (from
/// porcelain status or `--name-status`). A non-renamed file whose name
/// *literally* contains ` => ` (or `{… => …}`) is indistinguishable from
/// rename shorthand in numstat output, so when `raw` is itself a known path
/// that wins over the shorthand guess.
fn resolve_numstat_path(raw: &str, known_paths: &std::collections::HashSet<String>) -> String {
    if known_paths.contains(raw) {
        return raw.to_string();
    }
    if let Some(brace_start) = raw.find('{')
        && let Some(brace_end) = raw[brace_start..].find('}')
    {
        let brace_end = brace_start + brace_end;
        if let Some(arrow) = raw[brace_start..brace_end].find(" => ") {
            let new_part = &raw[brace_start + arrow + 4..brace_end];
            return format!("{}{new_part}{}", &raw[..brace_start], &raw[brace_end + 1..]);
        }
    }
    if let Some(arrow) = raw.find(" => ") {
        return raw[arrow + 4..].to_string();
    }
    raw.to_string()
}

/// Parse one `git diff --numstat` count column. Binary files report `-`,
/// which is an expected sentinel (not a parse failure) and maps to 0 without
/// logging — otherwise every changed image would emit a debug line on every
/// dirty-status poll. Anything else unparsable is logged and treated as 0.
fn numstat_count(value: &str, kind: &str, path: &str) -> u32 {
    if value == "-" {
        return 0;
    }
    value.parse().unwrap_or_else(|_| {
        tracing::debug!("numstat_map: unparsable {kind} count {value:?} for {path:?}");
        0
    })
}

/// Map of path -> (added, removed) from `git diff --numstat <args>`.
///
/// `known_paths` disambiguates numstat's rename shorthand from files that
/// literally contain ` => ` in their name — see [`resolve_numstat_path`].
fn numstat_map(
    dir: &Path,
    args: &[&str],
    known_paths: &std::collections::HashSet<String>,
) -> Result<HashMap<String, (u32, u32)>> {
    let mut full_args = vec!["diff", "--numstat"];
    full_args.extend_from_slice(args);
    let out = git_diff(dir, &full_args)?;
    let mut map = HashMap::new();
    for line in out.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(a), Some(r), Some(raw_path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        // Binary files report "-\t-\tpath"; both columns become (0, 0).
        let counts = (
            numstat_count(a, "added", raw_path),
            numstat_count(r, "removed", raw_path),
        );
        map.insert(resolve_numstat_path(raw_path, known_paths), counts);
    }
    Ok(map)
}

/// Count the lines in `path` with a buffered reader, so an untracked file is
/// never held in memory as a whole. Untracked files are line-counted on every
/// dirty status poll, and an untracked multi-GB dump (`dump.sql`, a
/// `node_modules` tarball) shouldn't be slurped into RAM just to count
/// newlines. Returns `None` on any read failure, which callers treat as 0.
fn count_lines(path: &Path) -> Option<u32> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    let mut count: u64 = 0;
    for line in std::io::BufReader::new(file).lines() {
        line.ok()?;
        count += 1;
    }
    u32::try_from(count).ok()
}

/// Line-count map for the no-base fallback: staged files diff against HEAD,
/// unstaged against the index, untracked files count as all-added.
fn working_tree_counts(dir: &Path, files: &[ChangedFile]) -> Result<HashMap<String, (u32, u32)>> {
    // Skip each `git diff --numstat` invocation when nothing needs it — e.g.
    // a fully-staged worktree has no unstaged files, so there's no point
    // asking git to diff the (empty) unstaged side. Untracked files never
    // show up in `git diff` output (they're counted from the file itself
    // below), so they don't justify running it either.
    let known: std::collections::HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    let unstaged = if files.iter().any(|f| !f.staged && f.status != "untracked") {
        numstat_map(dir, &[], &known)?
    } else {
        HashMap::new()
    };
    let staged = if files.iter().any(|f| f.staged) {
        numstat_map(dir, &["--cached"], &known)?
    } else {
        HashMap::new()
    };
    let mut counts = HashMap::new();
    for f in files {
        let map = if f.staged { &staged } else { &unstaged };
        if let Some(&c) = map.get(&f.path) {
            counts.insert(f.path.clone(), c);
        } else if f.status == "untracked"
            && let Some(lines) = count_lines(&dir.join(&f.path))
        {
            counts.insert(f.path.clone(), (lines, 0));
        }
    }
    Ok(counts)
}

fn apply_counts(files: &mut [ChangedFile], counts: &HashMap<String, (u32, u32)>) {
    for f in files.iter_mut() {
        if let Some(&(added, removed)) = counts.get(&f.path) {
            f.added = added;
            f.removed = removed;
        }
    }
}

/// Resolves the detected base branch together with its merge-base commit
/// with `HEAD`, if a base branch was found. Shared by [`branch_changes`] and
/// [`file_diff`], which both need "what to diff against" the same way.
///
/// Diffs against [`resolve_onto_ref`]'s ref (`origin/<base>` when it exists,
/// else local `<base>`), not the raw local branch — the same base ref
/// worktree creation branches off (`fetch_and_resolve_head`), rebase/merge
/// rebases onto, and `conflicts_with_base` simulates against. Diffing against
/// the local branch instead produces false-positive "changes" whenever local
/// `main` has drifted behind `origin/main` (e.g. fetched but not pulled): a
/// fresh worktree branched off the up-to-date remote would show every
/// remote-only commit as if it were its own uncommitted change.
fn resolve_diff_base(dir: &Path) -> Result<Option<(String, String)>> {
    let Some(base) = detect_base_branch(dir) else {
        return Ok(None);
    };
    let onto = resolve_onto_ref(dir, &base);
    let merge_base = match merge_base_for_onto(dir, &onto) {
        Ok(merge_base) => merge_base,
        Err(e) => {
            // No merge-base (e.g. an orphan branch with unrelated history)
            // means there's nothing to diff against; fall back to
            // working-tree-only changes, same as the no-base case, instead of
            // erroring out the whole Changes pane.
            tracing::warn!(
                "resolve_diff_base: no merge-base between {onto:?} and HEAD in {dir:?}: {e}"
            );
            return Ok(None);
        }
    };
    Ok(Some((base, merge_base)))
}

/// The merge-base commit between an already-resolved onto ref
/// (`origin/<base>` when present, else local `<base>`) and `HEAD` — what
/// [`branch_changes`]/[`file_diff`] diff against. Split out from
/// [`resolve_diff_base`] so [`status`] can reuse a base/onto it has already
/// detected instead of re-running `detect_base_branch` (up to ~4
/// subprocesses) a second time.
fn merge_base_for_onto(dir: &Path, onto: &str) -> Result<String> {
    git(dir, &["merge-base", onto, "HEAD"])
}

/// Files changed on this worktree's branch relative to its base, including
/// uncommitted edits. Falls back to working-tree changes if no base is found.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchChanges {
    /// The base branch compared against, if one was detected.
    pub base: Option<String>,
    pub files: Vec<ChangedFile>,
}

pub fn branch_changes(dir: &Path) -> Result<BranchChanges> {
    let diff_base = resolve_diff_base(dir)?;
    let mut files;
    let counts;
    match &diff_base {
        Some((_, merge_base)) => {
            // Diff fork point → working tree: all of the branch's changes.
            files = parse_name_status(&git(dir, &["diff", "--name-status", merge_base])?);
            let known: std::collections::HashSet<String> =
                files.iter().map(|f| f.path.clone()).collect();
            counts = numstat_map(dir, &[merge_base.as_str()], &known)?;

            // `git diff` never reports untracked files (they exist in
            // neither tree being compared), so a brand-new file wouldn't
            // otherwise show up until `git add`. Merge them in directly,
            // counting their lines as all-added like `working_tree_counts`
            // does for the no-base case.
            let working = changed_files(dir)?;
            let existing: std::collections::HashSet<String> =
                files.iter().map(|f| f.path.clone()).collect();
            for mut f in working
                .iter()
                .filter(|f| f.status == "untracked" && !existing.contains(&f.path))
                .cloned()
            {
                if let Some(lines) = count_lines(&dir.join(&f.path)) {
                    f.added = lines;
                }
                files.push(f);
            }

            // The diff above mixes already-committed changes with anything
            // still uncommitted. `sync_state` describes the *committed* copy
            // (pushed / committed-not-pushed / never committed) and `dirty`
            // records working-tree edits separately, so an already-pushed file
            // that's edited again still reads as `Pushed` + dirty instead of
            // collapsing to a plain "uncommitted" and losing the fact that
            // it's part of the branch's shipped work.
            let dirty: std::collections::HashSet<String> =
                working.iter().map(|f| f.path.clone()).collect();
            // Paths currently unmerged (mid-rebase/merge). The name-status
            // diff above reports them as a plain "M" — only porcelain status
            // exposes the conflict — so overwrite their `status` below, or the
            // Changes pane can't tell the user something needs resolving.
            let conflict_paths: std::collections::HashSet<String> = working
                .iter()
                .filter(|f| f.status == "conflict")
                .map(|f| f.path.clone())
                .collect();
            // Paths with no committed copy at HEAD: untracked, added to the
            // index but never committed, or currently conflicted (the working
            // tree is mid-merge/rebase, not a committed state). These can't be
            // "committed"/"pushed" no matter what the branch's upstream says.
            let new_paths: std::collections::HashSet<String> = working
                .iter()
                .filter(|f| matches!(f.status.as_str(), "untracked" | "added" | "conflict"))
                .map(|f| f.path.clone())
                .collect();
            let unpushed = unpushed_files(dir);
            // With no upstream to compare against, fall back to the branch's
            // own committed diff (merge-base → HEAD, working tree excluded) so
            // a file the branch actually committed can be told apart from one
            // inherited unchanged from the base. The latter's committed copy
            // is already on the remote via `origin/<base>`, so it reads as
            // Pushed even though this branch has no upstream yet — otherwise a
            // plain local edit to a base file would wrongly look "committed,
            // not pushed". Only computed in the no-upstream case; when an
            // upstream exists, `unpushed` already makes the distinction.
            let committed_on_branch: std::collections::HashSet<String> = if unpushed.is_none() {
                git(dir, &["diff", "--name-only", merge_base, "HEAD"])
                    .map(|out| out.lines().map(|l| l.to_string()).collect())
                    .unwrap_or_default()
            } else {
                std::collections::HashSet::new()
            };
            for f in &mut files {
                if conflict_paths.contains(&f.path) {
                    f.status = "conflict".to_string();
                }
                f.dirty = dirty.contains(&f.path);
                f.sync_state = if new_paths.contains(&f.path) {
                    SyncState::Uncommitted
                } else {
                    match &unpushed {
                        Some(set) if set.contains(&f.path) => SyncState::Committed,
                        Some(_) => SyncState::Pushed,
                        None if committed_on_branch.contains(&f.path) => SyncState::Committed,
                        None => SyncState::Pushed,
                    }
                };
            }
        }
        None => {
            files = changed_files(dir)?;
            counts = working_tree_counts(dir, &files)?;
        }
    }
    apply_counts(&mut files, &counts);
    // Natural (numeric-aware) ordering — git's own path order is plain
    // lexicographic, and the untracked files merged in above land at the end
    // regardless.
    files.sort_by(|a, b| crate::natural::compare(&a.path, &b.path));
    Ok(BranchChanges {
        base: diff_base.map(|(base, _)| base),
        files,
    })
}

/// Unified diff for a single file, relative to the branch base (or `HEAD` if
/// there's no base). Untracked/new files are shown as all-added.
pub fn file_diff(dir: &Path, path: &str) -> Result<String> {
    let diff = match resolve_diff_base(dir)? {
        Some((_, merge_base)) => git_diff(dir, &["diff", &merge_base, "--", path])?,
        // No base branch: diff HEAD → working tree, so a fully-staged change
        // isn't reported as empty (and a staged-then-modified file shows the
        // whole change, not just the unstaged half). Plain `git diff -- path`
        // only shows the unstaged (index → working tree) delta.
        None => git_diff(dir, &["diff", "HEAD", "--", path])?,
    };
    if !diff.trim().is_empty() {
        return Ok(diff);
    }
    // Untracked file (absent from HEAD): synthesize an all-added diff against
    // an empty tree.
    if dir.join(path).exists() {
        return Ok(
            git_diff(dir, &["diff", "--no-index", "--", "/dev/null", path]).unwrap_or_else(|e| {
                tracing::warn!("file_diff: --no-index diff for {path:?} in {dir:?} failed: {e}");
                String::new()
            }),
        );
    }
    Ok(diff)
}

/// Whether `path` exists as a blob at `HEAD` (i.e. the file is committed and
/// so survives a revert as a restore, rather than needing deletion).
fn exists_in_head(dir: &Path, path: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", &format!("HEAD:{path}")])
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Discard uncommitted changes to a single file, restoring it to `HEAD` (or
/// deleting it entirely if it isn't committed). Destructive with no undo — the
/// caller is expected to have confirmed with the user first.
///
/// Handles the three working-tree shapes the Changes pane can show:
/// - renamed/copied (`old_path` set): restore the original path from `HEAD`
///   and drop the new path;
/// - tracked (modified/added/deleted, staged or not): `git restore` back to
///   `HEAD`;
/// - untracked or staged-new (not in `HEAD`): unstage best-effort, then delete
///   the working file.
pub fn revert_file(dir: &Path, path: &str, old_path: Option<&str>) -> Result<()> {
    if let Some(old) = old_path {
        git(
            dir,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                old,
            ],
        )?;
        // `--ignore-unmatch` tolerates the new path already being gone (e.g. a
        // copy where only the index entry remains).
        git(dir, &["rm", "--force", "--ignore-unmatch", "--", path])?;
        return Ok(());
    }

    if exists_in_head(dir, path) {
        git(
            dir,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                path,
            ],
        )?;
        return Ok(());
    }

    // Untracked, or staged-new (in the index but not `HEAD`): unstage it if
    // present, then remove the working file. Tolerate both "not in the index"
    // (untracked) and "already gone" (a deleted file) as success.
    let _ = git(dir, &["restore", "--staged", "--", path]);
    match std::fs::remove_file(dir.join(path)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Io(e)),
    }
}

/// Parse the output of `git status --porcelain=v2 --branch`. Split out from
/// [`status`] so the parsing logic can be unit-tested without shelling out.
fn parse_status_porcelain_v2(out: &str) -> GitStatus {
    let mut st = GitStatus::default();

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            st.branch = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // Format: "+<ahead> -<behind>"
            let mut parts = rest.split_whitespace();
            if let Some(a) = parts.next() {
                st.ahead = a.trim_start_matches('+').parse().unwrap_or_else(|_| {
                    tracing::warn!("status: unparsable ahead count {a:?}");
                    0
                });
            }
            if let Some(b) = parts.next() {
                st.behind = b.trim_start_matches('-').parse().unwrap_or_else(|_| {
                    tracing::warn!("status: unparsable behind count {b:?}");
                    0
                });
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // Changed/renamed entry: second token is the two-char XY status.
            if let Some(xy) = line.split_whitespace().nth(1) {
                let mut chars = xy.chars();
                if chars.next().is_some_and(|c| c != '.') {
                    st.staged += 1;
                }
                if chars.next().is_some_and(|c| c != '.') {
                    st.modified += 1;
                }
            }
        } else if line.starts_with("u ") {
            st.conflicts += 1;
        } else if line.starts_with("? ") {
            st.untracked += 1;
        }
    }

    st
}

/// Parse `git status --porcelain=v2 --branch` for `dir`.
///
/// Uses `--untracked-files=all` for the same reason as [`changed_files`]: git's
/// default "normal" mode collapses a whole new untracked directory into one
/// entry, which would undercount `GitStatus::untracked` (and thus the
/// dirty/hover-card file count) whenever an agent creates new directories.
pub fn status(dir: &Path) -> Result<GitStatus> {
    let out = git(
        dir,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    let mut st = parse_status_porcelain_v2(&out);
    st.has_remote = has_remote(dir);

    // Working-tree line delta, not a full branch-vs-base diff (that's what
    // branch_changes() is for) — only computed when dirty so clean worktrees
    // (the common case) pay no extra git-process cost.
    if st.is_dirty() {
        let files = changed_files(dir).unwrap_or_default();
        let counts = working_tree_counts(dir, &files).unwrap_or_default();
        for &(added, removed) in counts.values() {
            st.added += added;
            st.removed += removed;
        }
    }

    if let Some(base) = detect_base_branch(dir) {
        // Count ahead-of-base against the same ref the diff and
        // conflict-simulation use (`origin/<base>` when present), so a
        // worktree branched off a fetched-but-unpulled remote doesn't look
        // like it has commits of its own. `is_merged_into_base` stays on the
        // local ref: its evidence (a merge commit in, or a "Fast-forward"
        // reflog entry on, `base`) is written to the local branch, never to
        // the remote-tracking ref.
        let onto = resolve_onto_ref(dir, &base);
        st.commits_ahead_of_base = commits_ahead_of_base(dir, &onto);
        st.merged_into_base = is_merged_into_base(dir, &base, &st.branch);
        st.conflicts_with_base = conflicts_with_base(dir, &onto);

        // Base line delta: diff merge-base -> HEAD via the same `onto`/
        // `base` detected above. `resolve_diff_base` would re-detect the
        // base (up to ~4 more subprocesses) and re-resolve `onto`, so reuse
        // them directly — this is the hottest git::status() code path
        // (every worktree, every dashboard refresh).
        if let Ok(merge_base) = merge_base_for_onto(dir, &onto) {
            // No known-paths set here: these counts are only ever summed
            // (the keys don't matter), so rename-shorthand resolution only
            // needs to not crash.
            if let Ok(counts) = numstat_map(
                dir,
                &[merge_base.as_str()],
                &std::collections::HashSet::new(),
            ) {
                for &(added, removed) in counts.values() {
                    st.base_added += added;
                    st.base_removed += removed;
                }
            }
        }
    }

    if let Some((message, time)) = last_commit_info(dir) {
        st.last_commit_message = message;
        st.last_commit_time = time;
    }

    Ok(st)
}

/// `HEAD`'s most recent commit subject and committer-date Unix timestamp,
/// `None` if there is no commit yet (a brand-new repo).
fn last_commit_info(dir: &Path) -> Option<(String, i64)> {
    let out = git(dir, &["log", "-1", "--format=%s%n%ct"]).ok()?;
    let mut lines = out.lines();
    let message = lines.next()?.to_string();
    let time = lines.next()?.parse().ok()?;
    Some((message, time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_untracked() {
        assert_eq!(classify('?', '?'), ("untracked", false));
    }

    #[test]
    fn classify_staged_takes_precedence_over_unstaged() {
        // "AM" = added in the index, modified in the working tree afterward.
        assert_eq!(classify('A', 'M'), ("added", true));
    }

    #[test]
    fn classify_unstaged_uses_second_column() {
        assert_eq!(classify(' ', 'M'), ("modified", false));
        assert_eq!(classify(' ', 'D'), ("deleted", false));
    }

    #[test]
    fn classify_renamed_and_copied() {
        assert_eq!(classify('R', ' '), ("renamed", true));
        assert_eq!(classify(' ', 'C'), ("copied", false));
    }

    #[test]
    fn classify_unknown_code_falls_back_to_modified() {
        assert_eq!(classify('T', ' '), ("modified", true));
    }

    #[test]
    fn classify_unmerged_paths_report_conflict() {
        for (x, y) in [
            ('U', 'U'),
            ('A', 'A'),
            ('D', 'D'),
            ('A', 'U'),
            ('U', 'A'),
            ('D', 'U'),
            ('U', 'D'),
        ] {
            assert_eq!(classify(x, y), ("conflict", true), "for {x}{y}");
        }
    }

    #[test]
    fn classify_added_then_deleted_is_not_a_conflict() {
        // "AD" = added to the index, then deleted from the working tree — a
        // normal, reachable state that the old A/D/U × A/D/U sweep mislabeled
        // as a conflict.
        assert_eq!(classify('A', 'D'), ("added", true));
        assert_eq!(classify('D', 'A'), ("deleted", true));
    }

    #[test]
    fn unquote_path_strips_quotes_and_unescapes() {
        assert_eq!(unquote_path("plain.txt"), "plain.txt");
        assert_eq!(unquote_path("\"quoted.txt\""), "quoted.txt");
        assert_eq!(unquote_path("\"a\\tb\""), "a\tb");
        assert_eq!(unquote_path("\"a\\nb\""), "a\nb");
        assert_eq!(unquote_path("\"a\\rb\""), "a\rb");
        assert_eq!(unquote_path("\"a\\\"b\""), "a\"b");
        assert_eq!(unquote_path("\"a\\\\b\""), "a\\b");
        // git's C-quoting also uses these control-char escapes; they must
        // decode rather than fall through to "keep verbatim".
        assert_eq!(unquote_path("\"a\\ab\""), "a\u{7}b");
        assert_eq!(unquote_path("\"a\\bb\""), "a\u{8}b");
        assert_eq!(unquote_path("\"a\\fb\""), "a\u{c}b");
        assert_eq!(unquote_path("\"a\\vb\""), "a\u{b}b");
        assert_eq!(unquote_path("\"uni-\\346\\227\\245.txt\""), "uni-日.txt");
    }

    #[test]
    fn c_unescape_keeps_unknown_or_truncated_escapes_verbatim() {
        assert_eq!(c_unescape("\\x"), "\\x");
        assert_eq!(c_unescape("trailing\\"), "trailing\\");
    }

    #[test]
    fn split_rename_arrow_plain() {
        assert_eq!(
            split_rename_arrow("old.txt -> new.txt"),
            Some(("old.txt", "new.txt"))
        );
    }

    #[test]
    fn split_rename_arrow_quoted_old_name_containing_arrow() {
        // The old name itself contains " -> " (git quotes it for the spaces);
        // the split must land on the real separator, not the arrow inside the
        // quoted name.
        assert_eq!(
            split_rename_arrow("\"a -> b.txt\" -> new.txt"),
            Some(("\"a -> b.txt\"", "new.txt"))
        );
    }

    #[test]
    fn split_rename_arrow_quoted_new_name_containing_arrow() {
        assert_eq!(
            split_rename_arrow("old.txt -> \"a -> b.txt\""),
            Some(("old.txt", "\"a -> b.txt\""))
        );
    }

    #[test]
    fn split_rename_arrow_no_arrow() {
        assert_eq!(split_rename_arrow("just-a-path.txt"), None);
    }

    #[test]
    fn parse_name_status_modified_and_added() {
        let out = "M\tsrc/main.rs\nA\tsrc/new.rs\nD\tsrc/old.rs";
        let files = parse_name_status(out);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].status, "modified");
        assert_eq!(files[1].status, "added");
        assert_eq!(files[2].status, "deleted");
        assert!(files.iter().all(|f| f.sync_state == SyncState::Committed));
    }

    #[test]
    fn parse_name_status_rename_keeps_new_path_and_records_old_path() {
        let out = "R100\told/name.rs\tnew/name.rs";
        let files = parse_name_status(out);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new/name.rs");
        assert_eq!(files[0].status, "renamed");
        assert_eq!(files[0].old_path, Some("old/name.rs".to_string()));
    }

    #[test]
    fn parse_name_status_non_rename_has_no_old_path() {
        let out = "M\tsrc/main.rs";
        let files = parse_name_status(out);
        assert_eq!(files[0].old_path, None);
    }

    #[test]
    fn parse_name_status_ignores_blank_lines() {
        let out = "M\ta.rs\n\nA\tb.rs\n";
        let files = parse_name_status(out);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn resolve_numstat_path_plain() {
        assert_eq!(
            resolve_numstat_path("src/main.rs", &std::collections::HashSet::new()),
            "src/main.rs"
        );
    }

    #[test]
    fn numstat_count_treats_binary_dash_as_zero_without_logging() {
        // "-" is git's binary sentinel, not a parse failure.
        assert_eq!(numstat_count("-", "added", "assets/icon-16.png"), 0);
        assert_eq!(numstat_count("-", "removed", "assets/icon-16.png"), 0);
        assert_eq!(numstat_count("12", "added", "src/main.rs"), 12);
        // A genuinely unparsable value still falls back to 0.
        assert_eq!(numstat_count("nope", "added", "src/main.rs"), 0);
    }

    #[test]
    fn resolve_numstat_path_full_rename() {
        assert_eq!(
            resolve_numstat_path(
                "old/name.rs => new/name.rs",
                &std::collections::HashSet::new()
            ),
            "new/name.rs"
        );
    }

    #[test]
    fn resolve_numstat_path_brace_rename() {
        assert_eq!(
            resolve_numstat_path(
                "src/{old => new}/file.rs",
                &std::collections::HashSet::new()
            ),
            "src/new/file.rs"
        );
    }

    #[test]
    fn resolve_numstat_path_brace_rename_with_empty_side() {
        // Git renders a pure move (no rename within the dir) as "{a => b}".
        assert_eq!(
            resolve_numstat_path("{a => b}/file.rs", &std::collections::HashSet::new()),
            "b/file.rs"
        );
    }

    #[test]
    fn resolve_numstat_path_keeps_a_literal_arrow_name_when_known() {
        // A non-renamed file whose name literally contains " => " (or
        // "{.. => ..}") must not be mistaken for rename shorthand. `known`
        // mirrors the caller's file list from porcelain/name-status parsing.
        let known: std::collections::HashSet<String> =
            ["a{old => new}.rs".to_string()].into_iter().collect();
        assert_eq!(
            resolve_numstat_path("a{old => new}.rs", &known),
            "a{old => new}.rs"
        );
        assert_eq!(
            resolve_numstat_path("foo => bar.txt", &known),
            "bar.txt",
            "an unknown path still resolves as rename shorthand"
        );
    }

    #[test]
    fn parse_status_porcelain_v2_branch_header() {
        let out = "# branch.head main\n# branch.ab +2 -3\n";
        let st = parse_status_porcelain_v2(out);
        assert_eq!(st.branch, "main");
        assert_eq!(st.ahead, 2);
        assert_eq!(st.behind, 3);
    }

    #[test]
    fn parse_status_porcelain_v2_staged_and_modified() {
        // "1 MM ..." = staged in the index (col 1) and modified after (col 2).
        let out = "1 MM N... 100644 100644 100644 abc123 def456 src/main.rs";
        let st = parse_status_porcelain_v2(out);
        assert_eq!(st.staged, 1);
        assert_eq!(st.modified, 1);
    }

    #[test]
    fn parse_status_porcelain_v2_untracked_and_conflicts() {
        let out = "? new_file.txt\nu UU N... 100644 100644 100644 100644 aaa bbb ccc conflicted.rs";
        let st = parse_status_porcelain_v2(out);
        assert_eq!(st.untracked, 1);
        assert_eq!(st.conflicts, 1);
    }

    #[test]
    fn is_dirty_counts_conflicts_alone() {
        // A worktree with only conflict markers (e.g. mid-rebase, before any
        // manual resolution) must count as dirty: merge/rebase gate on
        // is_dirty() and would otherwise proceed to `git rebase` a conflicted
        // worktree.
        let out = "u UU N... 100644 100644 100644 100644 aaa bbb ccc conflicted.rs";
        let st = parse_status_porcelain_v2(out);
        assert_eq!(st.staged, 0);
        assert_eq!(st.modified, 0);
        assert_eq!(st.untracked, 0);
        assert_eq!(st.conflicts, 1);
        assert!(st.is_dirty());
    }

    #[test]
    fn parse_status_porcelain_v2_unparsable_ahead_behind_defaults_to_zero() {
        let out = "# branch.ab +? -?\n";
        let st = parse_status_porcelain_v2(out);
        assert_eq!(st.ahead, 0);
        assert_eq!(st.behind, 0);
    }

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// Real `git` integration tests for [`status`]'s added/removed line
    /// counts, which exercise `working_tree_counts`'s staged/unstaged
    /// `numstat` skip-when-empty optimization end to end.
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

    #[test]
    fn has_remote_false_when_none_configured() {
        let repo = init_repo();
        assert!(!has_remote(repo.path()));
    }

    #[test]
    fn has_remote_true_once_origin_is_added() {
        let repo = init_repo();
        let origin = tempfile::tempdir().unwrap();
        run_git(origin.path(), &["init", "--quiet", "--bare"]);
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        assert!(has_remote(repo.path()));
    }

    #[test]
    fn fetch_remote_false_when_no_origin() {
        let repo = init_repo();
        assert!(!fetch_remote(repo.path()));
    }

    #[test]
    fn fetch_remote_true_when_origin_is_reachable() {
        let repo = init_repo();
        let origin = tempfile::tempdir().unwrap();
        run_git(origin.path(), &["init", "--quiet", "--bare"]);
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        run_git(repo.path(), &["push", "origin", "main"]);
        assert!(fetch_remote(repo.path()));
    }

    #[test]
    fn rebase_onto_base_succeeds_when_already_up_to_date() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        let outcome = rebase_onto_base(repo.path(), "main").unwrap();
        assert_eq!(outcome, RebaseOutcome::Success);
    }

    #[test]
    fn rebase_onto_base_aborts_and_reports_conflict() {
        let repo = init_repo();
        // Diverge main and a feature branch on the same line of the same file.
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
        run_git(repo.path(), &["checkout", "feat/thing"]);
        let original_head = git(repo.path(), &["rev-parse", "HEAD"]).unwrap();

        let outcome = rebase_onto_base(repo.path(), "main").unwrap();

        assert_eq!(outcome, RebaseOutcome::Conflict);
        // Aborted cleanly: HEAD is back where it started, no rebase in progress.
        assert_eq!(
            git(repo.path(), &["rev-parse", "HEAD"]).unwrap(),
            original_head
        );
        assert!(git(repo.path(), &["rebase", "--abort"]).is_err());
    }

    #[test]
    fn rebase_onto_base_errors_when_rebase_never_started() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);
        // Diverge main so a rebase would actually be attempted...
        run_git(repo.path(), &["checkout", "main"]);
        std::fs::write(repo.path().join("main.txt"), "x\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "main work"]);
        run_git(repo.path(), &["checkout", "feat/thing"]);
        // ...then dirty the worktree so `git rebase` refuses to even start.
        std::fs::write(repo.path().join("feature.txt"), "edited\n").unwrap();

        // Must surface as an error, not be reported as a rebase conflict.
        assert!(rebase_onto_base(repo.path(), "main").is_err());
    }

    #[test]
    fn fast_forward_merge_merges_when_repo_is_on_base() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);
        let feature_head = git(repo.path(), &["rev-parse", "HEAD"]).unwrap();
        run_git(repo.path(), &["checkout", "main"]);

        let outcome = fast_forward_merge(repo.path(), "main", "feat/thing").unwrap();

        assert!(matches!(outcome, MergeOutcome::Merged));
        assert_eq!(
            git(repo.path(), &["rev-parse", "HEAD"]).unwrap(),
            feature_head
        );
    }

    #[test]
    fn fast_forward_merge_reports_when_repo_is_on_a_different_branch() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);
        run_git(repo.path(), &["checkout", "-b", "wip"]);

        let outcome = fast_forward_merge(repo.path(), "main", "feat/thing").unwrap();

        assert!(matches!(
            outcome,
            MergeOutcome::NotOnBaseBranch { current } if current == "wip"
        ));
    }

    #[test]
    fn fast_forward_merge_reports_base_diverged() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);
        // Diverge `main` with its own commit, so `feat/thing` is no longer a
        // fast-forward descendant of it.
        run_git(repo.path(), &["checkout", "main"]);
        std::fs::write(repo.path().join("main.txt"), "x\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "main work"]);

        let outcome = fast_forward_merge(repo.path(), "main", "feat/thing").unwrap();

        assert!(matches!(outcome, MergeOutcome::BaseDiverged));
    }

    #[test]
    fn push_base_if_remote_false_when_no_remote() {
        let repo = init_repo();
        assert!(!push_base_if_remote(repo.path(), "main").unwrap());
    }

    #[test]
    fn push_base_if_remote_pushes_when_origin_exists() {
        let repo = init_repo();
        let origin = tempfile::tempdir().unwrap();
        run_git(origin.path(), &["init", "--quiet", "--bare"]);
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );

        let pushed = push_base_if_remote(repo.path(), "main").unwrap();

        assert!(pushed);
        let local = git(repo.path(), &["rev-parse", "main"]).unwrap();
        let remote = git(origin.path(), &["rev-parse", "main"]).unwrap();
        assert_eq!(local, remote);
    }

    #[test]
    fn status_reports_has_remote() {
        let repo = init_repo();
        let st = status(repo.path()).unwrap();
        assert!(!st.has_remote);

        let origin = tempfile::tempdir().unwrap();
        run_git(origin.path(), &["init", "--quiet", "--bare"]);
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        let st = status(repo.path()).unwrap();
        assert!(st.has_remote);
    }

    #[test]
    fn force_push_branch_pushes_to_origin_and_sets_upstream() {
        let repo = init_repo();
        let origin = tempfile::tempdir().unwrap();
        run_git(origin.path(), &["init", "--quiet", "--bare"]);
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );

        force_push_branch(repo.path(), "main").unwrap();

        let local = git(repo.path(), &["rev-parse", "main"]).unwrap();
        let remote = git(origin.path(), &["rev-parse", "main"]).unwrap();
        assert_eq!(local, remote);
        let upstream = git(
            repo.path(),
            &["rev-parse", "--abbrev-ref", "main@{upstream}"],
        )
        .unwrap();
        assert_eq!(upstream, "origin/main");
    }

    #[test]
    fn force_push_branch_overwrites_diverged_remote_history() {
        let repo = init_repo();
        let origin = tempfile::tempdir().unwrap();
        run_git(origin.path(), &["init", "--quiet", "--bare"]);
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        run_git(repo.path(), &["push", "-u", "origin", "main"]);
        // Rewrite local history (simulating a post-rebase force-push) so
        // origin/main now diverges from local main.
        run_git(repo.path(), &["commit", "--amend", "--no-edit"]);

        force_push_branch(repo.path(), "main").unwrap();

        let local = git(repo.path(), &["rev-parse", "main"]).unwrap();
        let remote = git(origin.path(), &["rev-parse", "main"]).unwrap();
        assert_eq!(local, remote);
    }

    #[test]
    fn branch_changes_includes_untracked_files_not_in_diff() {
        // Regression test: `git diff --name-status <merge_base>` never lists
        // untracked files (manually verified against a real corral worktree —
        // a brand-new file showed "Working tree clean" in the Changes pane
        // despite `git status` correctly seeing it as `??`).
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "--quiet", "-b", "feature"]);
        std::fs::write(repo.path().join("new_file.txt"), "a\nb\n").unwrap();

        let changes = branch_changes(repo.path()).unwrap();
        assert_eq!(changes.base.as_deref(), Some("main"));
        let f = changes
            .files
            .iter()
            .find(|f| f.path == "new_file.txt")
            .expect("untracked file should appear in branch_changes");
        assert_eq!(f.status, "untracked");
        assert_eq!(f.sync_state, SyncState::Uncommitted);
        assert!(f.dirty);
        assert_eq!(f.added, 2);
        assert_eq!(f.removed, 0);
    }

    #[test]
    fn branch_changes_orders_files_naturally() {
        // The Changes pane lists numbered files in numeric order (icon-2
        // before icon-16 before icon-128), not git's plain lexicographic order.
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "--quiet", "-b", "feature"]);
        std::fs::create_dir_all(repo.path().join("assets")).unwrap();
        for name in [
            "assets/icon-128.png",
            "assets/icon-16.png",
            "assets/icon-2.png",
        ] {
            std::fs::write(repo.path().join(name), "x\n").unwrap();
        }

        let paths: Vec<_> = branch_changes(repo.path())
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(
            paths,
            vec![
                "assets/icon-2.png",
                "assets/icon-16.png",
                "assets/icon-128.png",
            ]
        );
    }

    #[test]
    fn branch_changes_reports_pushed_files_when_branch_is_behind_its_upstream() {
        // Regression test for unpushed_files(): when the branch's upstream has
        // advanced past it (e.g. another machine pushed), a change this branch
        // already shipped must still read as Pushed — not "committed, not
        // pushed". A two-commit `git diff <upstream> HEAD` lists the remote's
        // newer copy of the file as "unpushed"; the `upstream..HEAD` range
        // (merge-base → HEAD) correctly reports nothing to push.
        let repo = init_repo();

        let origin = tempfile::tempdir().unwrap();
        std::fs::remove_dir(origin.path()).unwrap();
        run_git(
            repo.path(),
            &[
                "clone",
                "--quiet",
                "--bare",
                ".",
                origin.path().to_str().unwrap(),
            ],
        );
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        run_git(repo.path(), &["push", "--quiet", "-u", "origin", "main"]);

        run_git(repo.path(), &["checkout", "-b", "feat/x"]);
        std::fs::write(repo.path().join("file.txt"), "one\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);
        run_git(repo.path(), &["push", "--quiet", "-u", "origin", "feat/x"]);

        // Another machine modifies file.txt and pushes, advancing origin/feat/x
        // past the local tip.
        let clone_dir = tempfile::tempdir().unwrap();
        run_git(
            clone_dir.path(),
            &["clone", "--quiet", origin.path().to_str().unwrap(), "."],
        );
        run_git(
            clone_dir.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(clone_dir.path(), &["config", "user.name", "Test"]);
        run_git(
            clone_dir.path(),
            &["checkout", "-b", "feat/x", "origin/feat/x"],
        );
        std::fs::write(clone_dir.path().join("file.txt"), "one\nchanged\n").unwrap();
        run_git(clone_dir.path(), &["commit", "-am", "remote change"]);
        run_git(clone_dir.path(), &["push", "--quiet", "origin", "feat/x"]);

        // Bring the local remote-tracking ref up to date so the branch is now
        // behind its upstream.
        run_git(repo.path(), &["fetch", "--quiet", "origin"]);

        let changes = branch_changes(repo.path()).unwrap();
        let f = changes
            .files
            .iter()
            .find(|f| f.path == "file.txt")
            .expect("file.txt should be listed as changed vs base");
        assert_eq!(f.sync_state, SyncState::Pushed);
    }

    /// A bare `origin` for `repo`, with `main` already pushed to it, plus
    /// `origin` configured as the remote — the starting point for
    /// pushed/unpushed classification tests.
    fn add_bare_origin(repo: &tempfile::TempDir) -> tempfile::TempDir {
        let origin = tempfile::tempdir().unwrap();
        std::fs::remove_dir(origin.path()).unwrap();
        run_git(
            repo.path(),
            &[
                "clone",
                "--quiet",
                "--bare",
                ".",
                origin.path().to_str().unwrap(),
            ],
        );
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        run_git(repo.path(), &["push", "--quiet", "-u", "origin", "main"]);
        origin
    }

    #[test]
    fn branch_changes_keeps_pushed_state_when_a_pushed_file_is_edited_locally() {
        // The headline case for `dirty`: a file that's already committed and
        // pushed, then edited again, must still read as Pushed (its committed
        // copy is on the remote) *and* report `dirty` — not collapse to plain
        // "uncommitted" and lose the fact that it's part of the shipped work.
        let repo = init_repo();
        let _origin = add_bare_origin(&repo);

        run_git(repo.path(), &["checkout", "--quiet", "-b", "feat/x"]);
        std::fs::write(repo.path().join("file.txt"), "one\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);
        run_git(repo.path(), &["push", "--quiet", "-u", "origin", "feat/x"]);

        let clean = branch_changes(repo.path()).unwrap();
        let f = clean
            .files
            .iter()
            .find(|f| f.path == "file.txt")
            .expect("file.txt should be listed");
        assert_eq!(f.sync_state, SyncState::Pushed);
        assert!(!f.dirty);

        std::fs::write(repo.path().join("file.txt"), "one\ntwo\n").unwrap();
        let edited = branch_changes(repo.path()).unwrap();
        let f = edited
            .files
            .iter()
            .find(|f| f.path == "file.txt")
            .expect("file.txt should be listed");
        assert_eq!(f.sync_state, SyncState::Pushed);
        assert!(f.dirty);
    }

    #[test]
    fn branch_changes_keeps_committed_state_when_an_unpushed_file_is_edited_locally() {
        // Same idea one rung earlier: committed locally but never pushed, then
        // edited again → Committed + dirty, not a downgrade to "uncommitted".
        let repo = init_repo();
        let _origin = add_bare_origin(&repo);

        run_git(repo.path(), &["checkout", "--quiet", "-b", "feat/x"]);
        std::fs::write(repo.path().join("file.txt"), "one\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        let clean = branch_changes(repo.path()).unwrap();
        let f = clean
            .files
            .iter()
            .find(|f| f.path == "file.txt")
            .expect("file.txt should be listed");
        assert_eq!(f.sync_state, SyncState::Committed);
        assert!(!f.dirty);

        std::fs::write(repo.path().join("file.txt"), "one\ntwo\n").unwrap();
        let changes = branch_changes(repo.path()).unwrap();
        let f = changes
            .files
            .iter()
            .find(|f| f.path == "file.txt")
            .expect("file.txt should be listed");
        assert_eq!(f.sync_state, SyncState::Committed);
        assert!(f.dirty);
    }

    #[test]
    fn branch_changes_reports_pushed_for_a_base_file_edited_with_no_branch_upstream() {
        // A fresh corral worktree: branched off origin/main, never pushed, so
        // the branch has no upstream at all. A local edit to a file inherited
        // from the base must still read as Pushed (its committed copy is on
        // the remote via origin/main) + dirty — not "committed, not pushed",
        // which would wrongly imply the file has branch commits awaiting a
        // push.
        let repo = init_repo();
        let _origin = add_bare_origin(&repo);
        run_git(repo.path(), &["checkout", "--quiet", "-b", "feat/x"]);
        std::fs::write(
            repo.path().join("committed.txt"),
            "one\ntwo\nthree\nlocal\n",
        )
        .unwrap();

        let changes = branch_changes(repo.path()).unwrap();
        let f = changes
            .files
            .iter()
            .find(|f| f.path == "committed.txt")
            .expect("committed.txt should be listed");
        assert_eq!(f.sync_state, SyncState::Pushed);
        assert!(f.dirty);
    }

    #[test]
    fn branch_changes_marks_new_files_uncommitted_and_dirty() {
        // Untracked/new files have no committed copy at HEAD, so no matter
        // what the branch's upstream says they can only be Uncommitted.
        let repo = init_repo();
        let _origin = add_bare_origin(&repo);

        run_git(repo.path(), &["checkout", "--quiet", "-b", "feat/x"]);
        std::fs::write(repo.path().join("new.txt"), "hello\n").unwrap();

        let changes = branch_changes(repo.path()).unwrap();
        let f = changes
            .files
            .iter()
            .find(|f| f.path == "new.txt")
            .expect("new.txt should be listed");
        assert_eq!(f.sync_state, SyncState::Uncommitted);
        assert!(f.dirty);
    }

    #[test]
    fn branch_changes_reports_an_unmerged_file_as_conflict() {
        // Regression test: `git diff --name-status <merge_base>` reports a
        // conflicted path as a plain "M" (only `git status` porcelain exposes
        // the unmerged state), so a branch-with-base worktree used to show a
        // mid-rebase conflict as an ordinary modification — the Changes pane
        // had no way to tell the user something needed resolving.
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "--quiet", "-b", "feat/thing"]);
        std::fs::write(
            repo.path().join("committed.txt"),
            "one\ntwo\nthree\nfeature\n",
        )
        .unwrap();
        run_git(repo.path(), &["commit", "-qam", "feature edit"]);
        run_git(repo.path(), &["checkout", "--quiet", "main"]);
        std::fs::write(repo.path().join("committed.txt"), "one\ntwo\nthree\nmain\n").unwrap();
        run_git(repo.path(), &["commit", "-qam", "main edit"]);
        run_git(repo.path(), &["checkout", "--quiet", "feat/thing"]);

        // Merge main into the feature branch and expect a conflict.
        let merged = std::process::Command::new("git")
            .args(["merge", "main"])
            .current_dir(repo.path())
            .status()
            .unwrap();
        assert!(!merged.success(), "the merge should have conflicted");

        let changes = branch_changes(repo.path()).unwrap();
        assert_eq!(changes.base.as_deref(), Some("main"));
        let f = changes
            .files
            .iter()
            .find(|f| f.path == "committed.txt")
            .expect("the conflicted file should be listed");
        assert_eq!(f.status, "conflict");
        assert_eq!(f.sync_state, SyncState::Uncommitted);
        assert!(f.dirty);
    }

    #[test]
    fn branch_changes_falls_back_for_unrelated_history() {
        // Regression test for resolve_diff_base(): a branch with no common
        // ancestor with its base (e.g. an orphan root commit) has no
        // merge-base, which used to error out the whole Changes pane instead
        // of falling back to working-tree-only changes.
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "--quiet", "--orphan", "orphan"]);
        run_git(repo.path(), &["rm", "--quiet", "-rf", "."]);
        std::fs::write(repo.path().join("orphan.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "orphan root"]);

        let changes = branch_changes(repo.path()).unwrap();
        assert_eq!(changes.base, None);
        assert!(changes.files.is_empty());
    }

    #[test]
    fn branch_changes_diffs_against_origin_when_local_base_is_behind() {
        // Regression test for the Changes pane "vs main" false positives: a
        // worktree branched off a freshly-fetched origin/main must not show
        // every remote-only commit as its own change when local main has
        // drifted behind origin/main (fetched but never pulled). The diff
        // base must be the same ref worktree creation branches from —
        // `origin/<base>`, not the local branch.
        let repo = init_repo();

        // Give `repo` a bare origin, then advance origin/main past local
        // main from a separate clone — simulating another machine having
        // pushed since this repo was last pulled.
        let origin = tempfile::tempdir().unwrap();
        std::fs::remove_dir(origin.path()).unwrap();
        run_git(
            repo.path(),
            &[
                "clone",
                "--quiet",
                "--bare",
                ".",
                origin.path().to_str().unwrap(),
            ],
        );
        run_git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );

        let clone_dir = tempfile::tempdir().unwrap();
        run_git(
            clone_dir.path(),
            &["clone", "--quiet", origin.path().to_str().unwrap(), "."],
        );
        run_git(
            clone_dir.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(clone_dir.path(), &["config", "user.name", "Test"]);
        std::fs::write(clone_dir.path().join("remote-only.txt"), "new\n").unwrap();
        run_git(clone_dir.path(), &["add", "."]);
        run_git(
            clone_dir.path(),
            &["commit", "--quiet", "-m", "remote-only commit"],
        );
        run_git(clone_dir.path(), &["push", "--quiet", "origin", "main"]);

        // Fetch but don't pull: origin/main is now ahead of local main.
        run_git(repo.path(), &["fetch", "--quiet", "origin"]);
        assert_ne!(
            git(repo.path(), &["rev-parse", "main"]).unwrap(),
            git(repo.path(), &["rev-parse", "origin/main"]).unwrap()
        );

        // A clean worktree branched off the fetched remote tip, exactly as
        // worktree::create_under does via fetch_and_resolve_head.
        let remote_head = git(repo.path(), &["rev-parse", "origin/main"]).unwrap();
        let worktree = repo.path().join("wt");
        run_git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feat/thing",
                worktree.to_str().unwrap(),
                &remote_head,
            ],
        );

        // Before the fix this diffed against local main and reported
        // remote-only.txt as a change on a pristine worktree.
        let changes = branch_changes(&worktree).unwrap();
        assert_eq!(changes.base.as_deref(), Some("main"));
        assert!(
            changes.files.is_empty(),
            "a pristine worktree must show no changes vs the remote base, got: {:?}",
            changes.files
        );

        let st = status(&worktree).unwrap();
        assert_eq!(st.commits_ahead_of_base, 0);
        assert_eq!(st.base_added, 0);
        assert_eq!(st.base_removed, 0);

        // A real commit still shows up — and only that commit, diffed from
        // the true fork point rather than from the stale local main.
        std::fs::write(worktree.join("feature.txt"), "hello\n").unwrap();
        run_git(&worktree, &["add", "."]);
        run_git(&worktree, &["commit", "--quiet", "-m", "feature work"]);

        let changes = branch_changes(&worktree).unwrap();
        let paths: Vec<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["feature.txt"]);

        let st = status(&worktree).unwrap();
        assert_eq!(st.commits_ahead_of_base, 1);
        assert_eq!(st.base_added, 1);
        assert_eq!(st.base_removed, 0);
    }

    #[test]
    fn changed_files_expands_new_untracked_directory() {
        // Regression test: git's default "normal" untracked mode collapses a
        // whole new directory into one `?? subdir/` entry instead of listing
        // the files inside it, which undercounted/hid files in the Changes
        // pane whenever an agent created a new directory (e.g. `app/...`).
        let repo = init_repo();
        std::fs::create_dir(repo.path().join("subdir")).unwrap();
        std::fs::write(repo.path().join("subdir/one.txt"), "a\n").unwrap();
        std::fs::write(repo.path().join("subdir/two.txt"), "b\n").unwrap();

        let files = changed_files(repo.path()).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"subdir/one.txt"), "paths: {paths:?}");
        assert!(paths.contains(&"subdir/two.txt"), "paths: {paths:?}");
        assert!(!paths.contains(&"subdir/"), "paths: {paths:?}");
    }

    #[test]
    fn changed_files_reports_old_path_for_a_rename() {
        let repo = init_repo();
        run_git(repo.path(), &["mv", "committed.txt", "renamed.txt"]);

        let files = changed_files(repo.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "renamed.txt");
        assert_eq!(files[0].status, "renamed");
        assert_eq!(files[0].old_path, Some("committed.txt".to_string()));
    }

    #[test]
    fn changed_files_has_no_old_path_for_a_plain_edit() {
        let repo = init_repo();
        std::fs::write(repo.path().join("committed.txt"), "changed\n").unwrap();

        let files = changed_files(repo.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path, None);
    }

    #[test]
    fn changed_files_does_not_split_a_modified_file_named_with_an_arrow() {
        // Regression: a *modified* (not renamed) file whose name literally
        // contains " -> " must not be mis-parsed as a rename. Git quotes the
        // name (for its spaces), so a naive split on " -> " would leave a
        // stray quote on each side and lose the middle of the name.
        let repo = init_repo();
        std::fs::write(repo.path().join("a -> b.txt"), "one\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(
            repo.path(),
            &["commit", "--quiet", "-m", "add arrow-named file"],
        );
        std::fs::write(repo.path().join("a -> b.txt"), "one\ntwo\n").unwrap();

        let files = changed_files(repo.path()).unwrap();
        let f = files
            .iter()
            .find(|f| f.path == "a -> b.txt")
            .expect("the arrow-named file should be listed");
        assert_eq!(f.status, "modified");
        assert_eq!(f.old_path, None);
    }

    #[test]
    fn changed_files_reports_a_conflicted_file_as_conflict() {
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

        // Merge feat/thing into main, expecting a conflict in committed.txt
        // (both branches edited the same line). `run_git` asserts success, so
        // drive this one directly and expect the non-zero conflict exit.
        let merged = std::process::Command::new("git")
            .args(["merge", "feat/thing"])
            .current_dir(repo.path())
            .status()
            .unwrap();
        assert!(!merged.success(), "the merge should have conflicted");

        let files = changed_files(repo.path()).unwrap();
        let f = files
            .iter()
            .find(|f| f.path == "committed.txt")
            .expect("the conflicted file should be listed");
        assert_eq!(f.status, "conflict");
    }

    #[test]
    fn status_counts_untracked_files_inside_new_directory() {
        // Same collapse bug as changed_files_expands_new_untracked_directory,
        // but for the porcelain=v2 path feeding GitStatus::untracked (used by
        // the sidebar dirty count / hover card).
        let repo = init_repo();
        std::fs::create_dir(repo.path().join("subdir")).unwrap();
        std::fs::write(repo.path().join("subdir/one.txt"), "a\n").unwrap();
        std::fs::write(repo.path().join("subdir/two.txt"), "b\n").unwrap();

        let st = status(repo.path()).unwrap();
        assert_eq!(st.untracked, 2);
    }

    #[test]
    fn status_counts_lines_for_unstaged_only() {
        let repo = init_repo();
        std::fs::write(repo.path().join("committed.txt"), "one\ntwo\nthree\nfour\n").unwrap();

        let st = status(repo.path()).unwrap();
        assert_eq!(st.modified, 1);
        assert_eq!(st.added, 1);
        assert_eq!(st.removed, 0);
    }

    #[test]
    fn status_counts_lines_for_staged_only() {
        let repo = init_repo();
        std::fs::write(repo.path().join("committed.txt"), "one\ntwo\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo.path())
            .status()
            .unwrap();

        let st = status(repo.path()).unwrap();
        assert_eq!(st.staged, 1);
        assert_eq!(st.added, 0);
        assert_eq!(st.removed, 1);
    }

    #[test]
    fn status_counts_lines_for_mixed_staged_and_unstaged() {
        let repo = init_repo();
        std::fs::write(repo.path().join("committed.txt"), "one\ntwo\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo.path())
            .status()
            .unwrap();
        std::fs::write(repo.path().join("untracked.txt"), "a\nb\nc\n").unwrap();

        let st = status(repo.path()).unwrap();
        // committed.txt: staged removal of "three" (1 removed). untracked.txt:
        // 3 new lines, counted as all-added via the untracked fallback path.
        assert_eq!(st.staged, 1);
        assert_eq!(st.untracked, 1);
        assert_eq!(st.added, 3);
        assert_eq!(st.removed, 1);
    }

    #[test]
    fn status_counts_lines_for_untracked_only() {
        let repo = init_repo();
        std::fs::write(repo.path().join("untracked.txt"), "a\nb\n").unwrap();

        let st = status(repo.path()).unwrap();
        assert_eq!(st.untracked, 1);
        assert_eq!(st.added, 2);
        assert_eq!(st.removed, 0);
    }

    #[test]
    fn file_diff_shows_a_fully_staged_change_when_there_is_no_base() {
        // `init_repo` has a lone `main` branch, so there's no base branch and
        // file_diff falls back to `HEAD -> working tree`. A fully-staged edit
        // must show that edit (not an all-added /dev/null dump, which is what
        // plain `git diff -- path` + the untracked fallback used to produce).
        let repo = init_repo();
        std::fs::write(repo.path().join("committed.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        run_git(repo.path(), &["add", "committed.txt"]);

        let diff = file_diff(repo.path(), "committed.txt").unwrap();

        assert!(
            diff.contains("+four"),
            "expected the added line, got: {diff}"
        );
        assert!(
            !diff.contains("/dev/null"),
            "should be a real HEAD diff, not the all-added fallback: {diff}"
        );
    }

    #[test]
    fn file_diff_reports_an_untracked_file_as_all_added() {
        let repo = init_repo();
        std::fs::write(repo.path().join("new.txt"), "a\nb\n").unwrap();

        let diff = file_diff(repo.path(), "new.txt").unwrap();

        assert!(diff.contains("/dev/null"), "got: {diff}");
        assert!(diff.contains("+a"));
        assert!(diff.contains("+b"));
    }

    #[test]
    fn revert_file_restores_a_modified_tracked_file_to_head() {
        let repo = init_repo();
        std::fs::write(repo.path().join("committed.txt"), "changed\n").unwrap();

        revert_file(repo.path(), "committed.txt", None).unwrap();

        assert_eq!(
            std::fs::read_to_string(repo.path().join("committed.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
        assert!(changed_files(repo.path()).unwrap().is_empty());
    }

    #[test]
    fn revert_file_restores_a_deleted_tracked_file() {
        let repo = init_repo();
        std::fs::remove_file(repo.path().join("committed.txt")).unwrap();

        revert_file(repo.path(), "committed.txt", None).unwrap();

        assert_eq!(
            std::fs::read_to_string(repo.path().join("committed.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
    }

    #[test]
    fn revert_file_deletes_an_untracked_file() {
        let repo = init_repo();
        std::fs::write(repo.path().join("new.txt"), "a\nb\n").unwrap();

        revert_file(repo.path(), "new.txt", None).unwrap();

        assert!(!repo.path().join("new.txt").exists());
    }

    #[test]
    fn revert_file_removes_a_staged_new_file() {
        let repo = init_repo();
        std::fs::write(repo.path().join("new.txt"), "a\nb\n").unwrap();
        run_git(repo.path(), &["add", "new.txt"]);

        revert_file(repo.path(), "new.txt", None).unwrap();

        assert!(!repo.path().join("new.txt").exists());
        assert!(changed_files(repo.path()).unwrap().is_empty());
    }

    #[test]
    fn revert_file_restores_a_staged_rename() {
        let repo = init_repo();
        run_git(repo.path(), &["mv", "committed.txt", "renamed.txt"]);

        revert_file(repo.path(), "renamed.txt", Some("committed.txt")).unwrap();

        assert!(repo.path().join("committed.txt").exists());
        assert!(!repo.path().join("renamed.txt").exists());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("committed.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
        assert!(changed_files(repo.path()).unwrap().is_empty());
    }

    #[test]
    fn status_clean_repo_reports_zero_counts() {
        let repo = init_repo();

        let st = status(repo.path()).unwrap();
        assert!(!st.is_dirty());
        assert_eq!(st.added, 0);
        assert_eq!(st.removed, 0);
    }

    #[test]
    fn status_reports_new_when_branch_has_never_diverged_from_base() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/never-diverged"]);

        let st = status(repo.path()).unwrap();
        assert_eq!(st.commits_ahead_of_base, 0);
        assert!(!st.merged_into_base);
    }

    #[test]
    fn status_reports_commits_ahead_of_base_for_a_diverged_branch() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/diverged"]);
        std::fs::write(repo.path().join("new.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        let st = status(repo.path()).unwrap();
        assert_eq!(st.commits_ahead_of_base, 1);
        assert!(!st.merged_into_base);
    }

    #[test]
    fn status_reports_last_commit_message_and_time() {
        let repo = init_repo();

        let st = status(repo.path()).unwrap();
        assert_eq!(st.last_commit_message, "initial");
        assert!(st.last_commit_time > 0);
    }

    #[test]
    fn status_reports_base_added_and_removed_for_a_diverged_branch() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/diverged"]);
        std::fs::write(repo.path().join("new.txt"), "one\ntwo\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        let st = status(repo.path()).unwrap();
        assert_eq!(st.base_added, 2);
        assert_eq!(st.base_removed, 0);
    }

    #[test]
    fn status_reports_merged_once_base_has_moved_past_the_branch() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/soon-merged"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        // Fast-forward main to this branch's tip, then land one more commit
        // on main — simulating other work landing after this branch merged.
        // Covers the ancestor+strict-inequality fast path in
        // is_merged_into_base, as opposed to the reflog fallback exercised
        // below by status_reports_merged_immediately_after_a_bare_fast_forward.
        run_git(repo.path(), &["checkout", "main"]);
        run_git(repo.path(), &["merge", "--ff-only", "feat/soon-merged"]);
        std::fs::write(repo.path().join("later.txt"), "later\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "later main work"]);
        run_git(repo.path(), &["checkout", "feat/soon-merged"]);

        let st = status(repo.path()).unwrap();
        assert_eq!(st.commits_ahead_of_base, 0);
        assert!(st.merged_into_base);
    }

    #[test]
    fn status_reports_not_merged_when_branch_never_diverged_and_base_moves_on() {
        // Regression test: a branch created off base but never committed to
        // (e.g. a worktree that was set up but never used) is a strict
        // ancestor of base forever, and stays one as base advances via
        // unrelated commits — identical to what a real merge looks like from
        // ancestry alone. Without corroborating evidence (a merge commit or
        // reflog entry actually naming this branch), it must not be reported
        // as merged.
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/never-touched"]);
        run_git(repo.path(), &["checkout", "main"]);
        std::fs::write(repo.path().join("later.txt"), "later\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(
            repo.path(),
            &["commit", "--quiet", "-m", "unrelated main work"],
        );
        run_git(repo.path(), &["checkout", "feat/never-touched"]);

        let st = status(repo.path()).unwrap();
        assert_eq!(st.commits_ahead_of_base, 0);
        assert!(!st.merged_into_base);
    }

    #[test]
    fn status_reports_merged_via_a_real_merge_commit() {
        // Covers the non-fast-forward path of base_has_merge_of: a `--no-ff`
        // merge leaves a merge commit naming the branch as a parent, with no
        // "Fast-forward" reflog entry to fall back on.
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/no-ff"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        run_git(repo.path(), &["checkout", "main"]);
        run_git(
            repo.path(),
            &[
                "merge",
                "--no-ff",
                "--quiet",
                "-m",
                "merge it",
                "feat/no-ff",
            ],
        );
        run_git(repo.path(), &["checkout", "feat/no-ff"]);

        let st = status(repo.path()).unwrap();
        assert!(st.merged_into_base);
    }

    #[test]
    fn status_reports_merged_immediately_after_a_bare_fast_forward() {
        // Regression test: right after `git merge --ff-only`, main and the
        // branch sit at the identical commit, and nothing else has landed on
        // main since — the one case the ancestor+inequality check alone
        // can't tell apart from a branch that never diverged. This is
        // exactly what corral's own merge button does, and what a manual
        // `git merge --ff-only` from a terminal does too, so it must be
        // reported as merged from the reflog fallback alone.
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/instant-merge"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        run_git(repo.path(), &["checkout", "main"]);
        run_git(repo.path(), &["merge", "--ff-only", "feat/instant-merge"]);
        run_git(repo.path(), &["checkout", "feat/instant-merge"]);

        let st = status(repo.path()).unwrap();
        assert_eq!(st.commits_ahead_of_base, 0);
        assert!(st.merged_into_base);
    }

    #[test]
    fn status_defaults_new_fields_to_zero_false_when_no_base_branch_is_found() {
        // A lone `main` branch with no main/master/develop sibling and no
        // origin/HEAD: detect_base_branch() returns None, so the new fields
        // should stay at their Default (0, false) rather than erroring.
        let repo = init_repo();

        let st = status(repo.path()).unwrap();
        assert_eq!(st.commits_ahead_of_base, 0);
        assert!(!st.merged_into_base);
    }

    #[test]
    fn status_reports_no_conflict_for_a_cleanly_rebasable_branch() {
        let repo = init_repo();
        run_git(repo.path(), &["checkout", "-b", "feat/thing"]);
        std::fs::write(repo.path().join("feature.txt"), "hello\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "--quiet", "-m", "feature work"]);

        let st = status(repo.path()).unwrap();
        assert!(!st.conflicts_with_base);
    }

    #[test]
    fn status_reports_conflict_for_a_branch_that_would_conflict_on_rebase() {
        let repo = init_repo();
        // Diverge main and a feature branch on the same line of the same file
        // — the same setup rebase_onto_base_aborts_and_reports_conflict uses.
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
        run_git(repo.path(), &["checkout", "feat/thing"]);
        let head_before = git(repo.path(), &["rev-parse", "HEAD"]).unwrap();

        let st = status(repo.path()).unwrap();

        assert!(st.conflicts_with_base);
        // Non-destructive: HEAD is untouched and there's no rebase in progress.
        assert_eq!(
            git(repo.path(), &["rev-parse", "HEAD"]).unwrap(),
            head_before
        );
        assert!(git(repo.path(), &["rebase", "--abort"]).is_err());
    }
}
