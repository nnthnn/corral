//! Worktree lifecycle: list, create, rename, and remove.
//!
//! `feature` is the physical directory name under
//! `<worktrees_root>/<project>/<feature>` — assigned once at [`create`] and
//! never changed afterward, since it's the stable identity that session keys,
//! PTY `cwd`s, and `config::worktree_path` all key off. [`rename`] only ever
//! renames the git branch and swaps a cosmetic `slug` symlink
//! (`<worktrees_root>/<project>/<slug>` -> the physical dir), so a
//! running session's cwd is never invalidated by a rename.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::agent_session;
use crate::checks;
use crate::config;
use crate::error::{Error, Result};
use crate::git::{self, GitStatus};
use crate::log_err::LogErr;
use crate::names;
use crate::pr;

/// A single git worktree managed by corral.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub project: String,
    pub feature: String,
    /// The current rename slug — starts as the physical `feature` name and
    /// becomes `slugify(new_name)` on [`rename`]. Tracked via a slug symlink
    /// (`<project>/<slug>` -> the physical dir), so it's never
    /// persisted directly; it's reconstructed from that symlink on each
    /// [`list`]. Distinct from `title` (the human-readable name).
    pub slug: String,
    pub branch: String,
    pub path: PathBuf,
    /// `None` if the directory isn't a readable git worktree.
    pub status: Option<GitStatus>,
    /// Whether a prior Claude Code session id is on file for this worktree,
    /// so starting an agent here would `--resume` rather than start fresh.
    pub has_resumable_session: bool,
    /// Free-text note the user saved for this worktree (e.g. "blocked on
    /// review"), shown as a sidebar subtitle. Empty string if none is set.
    pub note: String,
    /// Human-readable title (e.g. "Fix Login Timeout"), set via [`rename`].
    /// Empty string if never set — the UI falls back to `slug` in that
    /// case. Unlike `slug`, this is pure display metadata: it never
    /// touches the branch name or filesystem.
    pub title: String,
    /// This worktree's GitHub PR state, if known — populated by the
    /// `corral-app` command layer from `pr_poll.rs`'s background cache
    /// (see `commands/worktrees.rs::list_worktrees`), never by
    /// `corral-core` itself (a live `gh` call per worktree is too slow to
    /// do inline on every `list()`). Always `None` from `worktree::list()`,
    /// `create()`, and `rename()` directly.
    #[serde(default)]
    pub pr_state: Option<pr::PrInfo>,
    /// Coarse CI status for the open PR above, when polled — same sourcing
    /// caveat as `pr_state`. `None` means "no PR, or checks not fetched yet";
    /// `Some` with `total == 0` means "PR open, no checks configured".
    #[serde(default)]
    pub pr_checks: Option<checks::ChecksSummary>,
    /// Whether this worktree's project has a non-empty `setup_command`
    /// configured. The Setup tab is always present, so this lets it show a
    /// "no setup command configured" hint instead of offering a Re-run that
    /// would just error. Computed for free from the already-loaded project
    /// registry (see `filter_to_registered_projects`).
    #[serde(default)]
    pub has_setup_command: bool,
}

/// `git::status`, logging (rather than propagating) failure — used wherever a
/// missing/unreadable status shouldn't block returning the rest of a
/// [`Worktree`].
fn best_effort_status(path: &Path, context: &str) -> Option<GitStatus> {
    git::status(path)
        .inspect_err(|e| tracing::warn!("{context}: git status failed for {path:?}: {e}"))
        .ok()
}

/// A worktree found on disk, before its (potentially slow) git status has
/// been fetched.
struct FoundWorktree {
    project: String,
    feature: String,
    path: PathBuf,
    slug: String,
    created: std::time::SystemTime,
}

/// The worktree directory's birth time, so worktrees can be listed in
/// creation order — falls back to the Unix epoch (sorting last-ish, but
/// stably) if the filesystem doesn't report one.
fn created_time(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.created())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

/// List every worktree under `<worktrees_root>/<project>/<feature>`,
/// filtered to only those whose project is currently registered.
pub fn list() -> Result<Vec<Worktree>> {
    let registered = crate::project_settings::all()?;
    Ok(filter_to_registered_projects(
        list_under(&config::worktrees_root()?)?,
        &registered,
    ))
}

/// The physical feature names (directory names) directly under `project`'s
/// worktree dir — a cheap "which names are taken" query for `names::generate`
/// that avoids [`list`]'s per-worktree git status spawns. Slug symlinks are
/// included too: they're not worktrees, but they still occupy a directory
/// name under `<project>/`, so a generated name matching one would make
/// [`create`] fail with `WorktreeExists` (its `path.exists()` follows the
/// symlink).
pub(crate) fn list_feature_names(project: &str) -> Result<std::collections::HashSet<String>> {
    list_feature_names_under(&config::worktrees_root()?, project)
}

/// [`list_feature_names`] with an explicit worktrees root, so it's testable
/// without touching `$HOME`.
fn list_feature_names_under(
    root: &Path,
    project: &str,
) -> Result<std::collections::HashSet<String>> {
    let project_root = root.join(project);
    let mut names = std::collections::HashSet::new();
    let Ok(entries) = std::fs::read_dir(&project_root) else {
        return Ok(names);
    };
    for entry in entries.flatten() {
        if entry
            .file_type()
            .map(|t| t.is_dir() || t.is_symlink())
            .unwrap_or(false)
        {
            names.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(names)
}

/// Drop any worktree whose project isn't currently registered — e.g. one
/// left behind on disk after `project_settings::unregister` (via
/// `remove_project`), which deliberately never touches worktree
/// directories. Pure and independently testable, unlike `list()` itself
/// (which touches the real registry store).
fn filter_to_registered_projects(
    worktrees: Vec<Worktree>,
    registered: &crate::project_settings::Store,
) -> Vec<Worktree> {
    worktrees
        .into_iter()
        .filter(|w| registered.contains_key(&w.project))
        .map(|mut w| {
            w.has_setup_command = registered
                .get(&w.project)
                .is_some_and(|s| !s.setup_command.trim().is_empty());
            w
        })
        .collect()
}

/// [`list`] with an explicit worktrees root, so it's testable without
/// touching `$HOME`.
fn list_under(root: &Path) -> Result<Vec<Worktree>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    // Loaded once rather than per-worktree: avoids re-reading and re-parsing
    // the same session store file on every iteration below.
    let resumable_keys = agent_session::resumable_keys().unwrap_or_else(|e| {
        tracing::warn!(
            "list: failed to read session store, no worktree will show as resumable: {e}"
        );
        Default::default()
    });
    let notes = crate::worktree_notes::all().unwrap_or_else(|e| {
        tracing::warn!(
            "list: failed to read worktree notes store, no worktree will show a note: {e}"
        );
        Default::default()
    });
    let titles = crate::worktree_settings::all_titles().unwrap_or_else(|e| {
        tracing::warn!(
            "list: failed to read worktree settings store, no worktree will show a title: {e}"
        );
        Default::default()
    });

    let mut found = Vec::new();
    for project_entry in std::fs::read_dir(root)? {
        let project_entry = project_entry?;
        if !project_entry.file_type()?.is_dir() {
            continue;
        }
        let project = project_entry.file_name().to_string_lossy().into_owned();
        let project_root = project_entry.path();
        let slugs = slug_symlinks(&project_root)?;

        for feature_entry in std::fs::read_dir(&project_root)? {
            let feature_entry = feature_entry?;
            if !feature_entry.file_type()?.is_dir() {
                continue;
            }
            let path = feature_entry.path();
            let feature = feature_entry.file_name().to_string_lossy().into_owned();
            // `slugs` is keyed by the *canonical* target path (each slug
            // symlink is resolved in [`slug_symlinks`]), so look it up by the
            // canonical feature path too — otherwise a worktrees root that
            // itself traverses a symlink (`~/source` -> external drive) would
            // spell the feature path differently than the resolved symlink
            // target, and every rename slug would silently fall back to the
            // physical `feature` name.
            let slug = std::fs::canonicalize(&path)
                .ok()
                .and_then(|canon| slugs.get(&canon).cloned())
                .unwrap_or_else(|| feature.clone());
            let created = created_time(&path);
            found.push(FoundWorktree {
                project: project.clone(),
                feature,
                path,
                slug,
                created,
            });
        }
    }

    // Projects stay alphabetical (so the sidebar's project sections don't
    // reorder as worktrees come and go); within a project, oldest-created
    // worktree first.
    found.sort_by(|a, b| {
        (a.project.as_str(), a.created, a.feature.as_str()).cmp(&(
            b.project.as_str(),
            b.created,
            b.feature.as_str(),
        ))
    });

    // Each worktree's status is an independent `git` process spawn/wait;
    // fetch them concurrently rather than one at a time — but bounded to a
    // fixed number of worker threads, since one thread per worktree (each of
    // which itself spawns several `git` subprocesses) would thrash for a
    // project with a large number of worktrees.
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16);

    // Shared atomic cursor for the workers below. Declared outside the scope
    // so the spawned threads can borrow it (threads may only borrow data that
    // outlives the scope, not locals declared inside it).
    let next = std::sync::atomic::AtomicUsize::new(0);

    let statuses: Vec<Option<GitStatus>> = std::thread::scope(|scope| {
        // Work-stealing over the shared cursor: each worker grabs the next
        // unclaimed index until the list is exhausted, then reports its
        // results back by index so the final vector stays in `found` order
        // and exactly `found.len()` long.
        let handles: Vec<_> = (0..worker_count.min(found.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut results = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= found.len() {
                            break;
                        }
                        results.push((i, best_effort_status(&found[i].path, "list")));
                    }
                    results
                })
            })
            .collect();

        // A panicked worker (which `best_effort_status` shouldn't cause) must
        // not silently drop worktrees from the list — its slots stay `None`.
        let mut out = vec![None; found.len()];
        for handle in handles {
            for (i, status) in handle.join().unwrap_or_default() {
                out[i] = status;
            }
        }
        out
    });

    let mut out = Vec::with_capacity(found.len());
    for (w, status) in found.into_iter().zip(statuses) {
        let branch = status
            .as_ref()
            .map(|s| s.branch.clone())
            .unwrap_or_else(|| config::branch_name(&w.feature));
        let key = agent_session::key(&w.project, &w.feature);
        let has_resumable_session = resumable_keys.contains(&key);
        let note = notes.get(&key).cloned().unwrap_or_default();
        let title = titles.get(&key).cloned().unwrap_or_default();

        out.push(Worktree {
            project: w.project,
            feature: w.feature,
            slug: w.slug,
            branch,
            path: w.path,
            status,
            has_resumable_session,
            note,
            title,
            pr_state: None,
            pr_checks: None,
            has_setup_command: false,
        });
    }

    Ok(out)
}

/// Map of physical worktree path -> slug, from every symlink directly under
/// `project_root` (each is a rename alias for the worktree it points at).
fn slug_symlinks(project_root: &Path) -> Result<HashMap<PathBuf, String>> {
    let mut map = HashMap::new();
    for entry in std::fs::read_dir(project_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.symlink_metadata()?.file_type().is_symlink()
            && let Ok(target) = std::fs::canonicalize(&path)
        {
            map.insert(target, entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(map)
}

/// Remove whichever symlink under `project_root` currently aliases `target`,
/// if any (there's at most one at a time — [`rename`] replaces it in place).
fn remove_slug_symlink(project_root: &Path, target: &Path) -> Result<()> {
    if !project_root.exists() {
        return Ok(());
    }
    // Canonicalize `target` once so it compares equal to each symlink's own
    // canonicalized target even when `target` itself is spelled through a
    // symlinked ancestor (see `list_under`'s slug lookup for the same issue).
    let resolved_target = std::fs::canonicalize(target).ok();
    for entry in std::fs::read_dir(project_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.symlink_metadata()?.file_type().is_symlink() {
            match std::fs::canonicalize(&path) {
                Ok(resolved) if resolved_target.as_deref() == Some(resolved.as_path()) => {
                    std::fs::remove_file(&path)?
                }
                Ok(_) => {}
                Err(e) => tracing::debug!(
                    "remove_slug_symlink: couldn't resolve symlink {path:?}, leaving it in place: {e}"
                ),
            }
        }
    }
    Ok(())
}

/// Builds a matcher from `patterns`, skipping (and logging) any individual
/// pattern that fails to parse rather than failing the whole build — a
/// typo'd glob shouldn't block worktree creation.
fn build_globset(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(e) => tracing::warn!("create: skipping unparsable copy glob {pattern:?}: {e}"),
        }
    }
    builder.build().unwrap_or_else(|e| {
        tracing::warn!("create: failed to build glob matcher, no files will be copied: {e}");
        GlobSetBuilder::new()
            .build()
            .expect("empty GlobSet always builds")
    })
}

/// Copy every file under `repo` matching one of `patterns` into the same
/// relative path under `worktree_path`, creating parent directories as
/// needed. Best-effort, like `link_claude_dir` — a single file failing to
/// copy is logged, not propagated, so it never fails worktree creation.
/// Always skips `.git/`, regardless of how permissive `patterns` is (e.g.
/// `**/*`), since copying git's internal directory would corrupt the new
/// worktree's own git state.
fn copy_matching_files(repo: &Path, worktree_path: &Path, patterns: &[String]) {
    let globset = build_globset(patterns);
    if globset.is_empty() {
        return;
    }
    let mut builder = ignore::WalkBuilder::new(repo);
    builder
        .hidden(false)
        .git_ignore(false)
        .git_exclude(false)
        // Prune `.git` during traversal rather than enumerating its whole
        // object store and skipping each entry afterward: the per-entry check
        // below would still prevent *copying* from it, but a broad glob
        // (`**/*`) would otherwise walk every loose object/pack of a large
        // repo for nothing. `filter_entry` skips the directory outright, so
        // the walker never descends into it.
        .filter_entry(|entry| !entry.path().components().any(|c| c.as_os_str() == ".git"));
    let walker = builder.build();
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!("create: error walking {repo:?} for file copy: {e}");
                continue;
            }
        };
        let path = entry.path();
        if path.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(repo) else {
            continue;
        };
        if !globset.is_match(rel) {
            continue;
        }
        let dest = worktree_path.join(rel);
        if let Some(parent) = dest.parent()
            && std::fs::create_dir_all(parent)
                .log_warn(&format!(
                    "create: failed to create {parent:?} for copied file"
                ))
                .is_none()
        {
            continue;
        }
        std::fs::copy(path, &dest).log_warn(&format!(
            "create: failed to copy {rel:?} into new worktree {worktree_path:?}"
        ));
    }
}

/// Symlink `<worktree>/.claude` to `<repo>/.claude`, creating the latter if
/// needed, so every worktree of a project shares one settings/permissions
/// file instead of accumulating separate ones. Best-effort, like the
/// glob-based file copy above — a failure here shouldn't fail worktree
/// creation.
fn link_claude_dir(repo: &Path, worktree_path: &Path) {
    let src = repo.join(".claude");
    if !src.exists()
        && std::fs::create_dir_all(&src)
            .log_warn(&format!("create: failed to create {src:?}"))
            .is_none()
    {
        return;
    }
    let dest = worktree_path.join(".claude");
    if dest.exists() {
        return;
    }
    std::os::unix::fs::symlink(&src, &dest).log_warn(&format!(
        "create: failed to symlink .claude into new worktree {dest:?}"
    ));
    exclude_claude_symlink(worktree_path);
}

/// Keep corral's own `.claude` symlink out of git status, so a worktree
/// whose project doesn't gitignore `.claude` doesn't show corral's
/// shared-settings link as an untracked change in the Changes pane for its
/// whole life. Writes `.claude` into the repo's `.git/info/exclude` —
/// untracked repo metadata, never committed, and (via the shared `.git`
/// dir) applied to every worktree of the repo, which matches corral
/// symlinking `.claude` into each of them. Only called when this function
/// actually creates the symlink; a worktree where `.claude` already existed
/// (e.g. the project tracks it in git) is left alone. Best-effort like the
/// rest of `link_claude_dir` — a failure to exclude is logged, not
/// propagated, and the symlink itself is what matters.
fn exclude_claude_symlink(worktree_path: &Path) {
    let Ok(exclude_path) = git::git(worktree_path, &["rev-parse", "--git-path", "info/exclude"])
    else {
        return;
    };
    let exclude_path = PathBuf::from(exclude_path);
    let existing = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == ".claude") {
        return;
    }
    if let Some(parent) = exclude_path.parent()
        && std::fs::create_dir_all(parent)
            .log_warn(&format!(
                "create: failed to create {parent:?} for .git/info/exclude"
            ))
            .is_none()
    {
        return;
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(".claude\n");
    if let Err(e) = std::fs::write(&exclude_path, updated) {
        tracing::warn!("create: failed to write {exclude_path:?}: {e}");
    }
}

/// Create a worktree for `project`/`feature` on branch `feat/<feature>`.
///
/// Creates the branch if it doesn't exist, otherwise checks out the existing
/// branch. Copies files matching the project's configured glob patterns
/// (none for a project with no saved settings — `ProjectSettings::default()`
/// has an empty `copy_globs` list) from the main repo, and symlinks
/// `.claude` back to the main repo so settings are shared.
pub fn create(project: &str, feature: &str) -> Result<Worktree> {
    let project_settings = crate::project_settings::get(project)?;
    let repo = config::repo_path(project)?;
    create_under(
        &repo,
        &config::worktrees_root()?,
        project,
        feature,
        &project_settings,
    )
}

/// [`create`] with explicit repo/worktrees roots and project settings, so
/// it's testable without touching `$HOME` or the real settings store. Takes
/// the registered repo path directly (rather than reconstructing it from a
/// source root + project name) so `create` never operates on a path derived
/// from the unvalidated `name == basename(path)` invariant.
fn create_under(
    repo: &Path,
    worktrees_root: &Path,
    project: &str,
    feature: &str,
    project_settings: &crate::project_settings::ProjectSettings,
) -> Result<Worktree> {
    if !repo.join(".git").exists() {
        return Err(Error::NotAGitRepo(repo.to_path_buf()));
    }

    let path = worktrees_root.join(project).join(feature);
    if path.exists() {
        return Err(Error::WorktreeExists(path));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let branch = config::branch_name(feature);
    let path_str = path.to_string_lossy();

    // Best-effort fetch so a fresh worktree branches off the latest fetched
    // code, not whatever `repo` happened to have checked out locally.
    let start_point = git::fetch_and_resolve_head(repo);

    // Decide up front whether the branch already exists (e.g. a prior
    // worktree of this feature was removed with `--keep-branch`) rather than
    // treating *any* `worktree add -b` failure as "branch exists": doing the
    // latter runs the fallback for unrelated failures (a bad start point, a
    // locked index, permissions) and then reports the fallback's error, hiding
    // the real cause.
    if git::git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
    {
        // Branch already exists — check it out instead of recreating it.
        git::git(repo, &["worktree", "add", &path_str, &branch])?;
    } else {
        // Fresh branch, off the latest fetched base. `--no-track` is required
        // because `start_point` is usually a remote-tracking ref (`origin/main`
        // when it exists) and git's `branch.autoSetupMerge` default would
        // otherwise set `branch.<name>.merge` to the base branch, so a bare
        // `git pull`/`git push` would target the base instead of the fresh
        // branch. `force_push_branch`'s `-u` sets the real upstream on first
        // push.
        git::git(
            repo,
            &[
                "worktree",
                "add",
                "--no-track",
                "-b",
                &branch,
                &path_str,
                &start_point,
            ],
        )?;
    }

    copy_matching_files(repo, &path, &project_settings.copy_globs);

    link_claude_dir(repo, &path);

    let status = best_effort_status(&path, "create");
    tracing::info!("created worktree {project}/{feature} at {path:?}");
    Ok(Worktree {
        project: project.to_string(),
        feature: feature.to_string(),
        slug: feature.to_string(),
        branch,
        path,
        status,
        has_resumable_session: false,
        note: String::new(),
        title: String::new(),
        pr_state: None,
        pr_checks: None,
        has_setup_command: !project_settings.setup_command.trim().is_empty(),
    })
}

/// Rename `project`/`feature`'s branch, swap its slug alias, and save
/// `new_name` as its human-readable title.
///
/// The physical worktree directory and its git worktree registration are
/// never touched — only `git branch -m`, a cosmetic slug symlink,
/// and the saved title change — so a running session's cwd (and any
/// editor/shell already pointed at the physical path) stays valid across a
/// rename.
pub fn rename(project: &str, feature: &str, new_name: &str) -> Result<Worktree> {
    rename_under(&config::worktrees_root()?, project, feature, new_name)
}

/// [`rename`] with an explicit worktrees root, so the git/filesystem side
/// is testable without touching `$HOME` — the note and title side-stores
/// still read/write the real `~/.local/share/corral` store, since they
/// aren't parameterized by a root the way the worktree path is.
fn rename_under(
    worktrees_root: &Path,
    project: &str,
    feature: &str,
    new_name: &str,
) -> Result<Worktree> {
    let new_slug = names::slugify(new_name);
    if new_slug.is_empty() {
        return Err(Error::InvalidName(new_name.to_string()));
    }

    let repo_path = worktrees_root.join(project).join(feature);
    if !repo_path.exists() {
        return Err(Error::WorktreeNotFound(format!("{project}/{feature}")));
    }
    let project_root = worktrees_root.join(project);

    // Reject a slug that collides with a sibling's physical directory or slug
    // alias *before* mutating anything: the branch rename and slug-symlink
    // swap below aren't atomic, so failing at the symlink step (which would
    // `EEXIST` on an occupied path) would otherwise leave the worktree
    // half-renamed — branch already moved, alias already gone. Renaming back
    // to the physical `feature` name needs no alias, so that path existing is
    // fine. `symlink_metadata` (not `exists`) so a *dangling* alias at the
    // name also counts as occupied — `exists` follows symlinks and would miss
    // it, only to have `symlink()` fail with `EEXIST` a moment later.
    if new_slug != feature {
        let new_slug_path = project_root.join(&new_slug);
        if new_slug_path.symlink_metadata().is_ok() {
            return Err(Error::WorktreeExists(new_slug_path));
        }
    }

    // Only the current branch name is needed — a full `git::status` would
    // shell out to conflict-simulation/base-detection subprocesses just to
    // read one ref, and would fail outright on a detached HEAD.
    let current_branch = git::git(&repo_path, &["branch", "--show-current"])?;
    let new_branch = config::branch_name(&new_slug);
    if new_branch != current_branch {
        git::git(&repo_path, &["branch", "-m", &current_branch, &new_branch])?;
    }

    remove_slug_symlink(&project_root, &repo_path)?;
    // Renaming back to the physical dir's own name needs no alias.
    if new_slug != feature {
        std::os::unix::fs::symlink(&repo_path, project_root.join(&new_slug))?;
    }

    let status = best_effort_status(&repo_path, "rename");
    let branch = status
        .as_ref()
        .map(|s| s.branch.clone())
        .unwrap_or(new_branch);
    let has_resumable_session = agent_session::has(&agent_session::key(project, feature))
        .inspect_err(|e| tracing::warn!("rename: failed to check resumable session: {e}"))
        .unwrap_or(false);
    let note = crate::worktree_notes::get(&agent_session::key(project, feature))
        .inspect_err(|e| tracing::warn!("rename: failed to read worktree note: {e}"))
        .unwrap_or_default();
    // Use the trimmed input directly rather than reading the title back from
    // the store: a separate read-back would need its own lock acquisition
    // (set_title's lock is released before this point), leaving a window
    // where a concurrent rename of the same worktree (e.g. two corral
    // windows, or the GUI and the CLI) could interleave and hand this call
    // back someone else's title. Best-effort like `note` above — a
    // persistence failure here is warned, not propagated, since the branch
    // rename itself already succeeded.
    let title = new_name.trim().to_string();
    if let Err(e) =
        crate::worktree_settings::set_title(&agent_session::key(project, feature), &title)
    {
        tracing::warn!("rename: failed to save worktree title: {e}");
    }
    // Best-effort: a failed settings read just means the Setup tab's hint is
    // conservative (Re-run enabled), never that the rename itself failed.
    let has_setup_command = crate::project_settings::get(project)
        .map(|s| !s.setup_command.trim().is_empty())
        .unwrap_or(false);
    Ok(Worktree {
        project: project.to_string(),
        feature: feature.to_string(),
        slug: new_slug,
        branch,
        path: repo_path,
        status,
        has_resumable_session,
        note,
        title,
        pr_state: None,
        pr_checks: None,
        has_setup_command,
    })
}

/// Remove `project`/`feature`'s worktree (and any slug alias),
/// optionally deleting its branch (safe `git branch -d`).
pub fn remove(project: &str, feature: &str, delete_branch: bool) -> Result<()> {
    let repo = config::repo_path(project)?;
    let worktrees_root = config::worktrees_root()?;
    let path = worktrees_root.join(project).join(feature);
    // Bail before the opencode cleanup below if there's nothing to remove —
    // otherwise a `WorktreeNotFound` would still shell out to `opencode
    // session list` (which would list/delete zero sessions) for nothing.
    if !path.exists() {
        return Err(Error::WorktreeNotFound(format!("{project}/{feature}")));
    }
    // Clear the harness's (opencode2's) own persisted sessions for this
    // directory *before* removing it, so a future worktree reusing the same
    // feature name (and thus the same path) doesn't resurface this worktree's
    // old agent conversations. Deleting first is the belt-and-braces order: an
    // earlier opencode2 build resolved the session's directory via
    // `FileSystem.realPath` and refused to delete a session whose directory
    // was already gone (ENOENT). The installed 2.0.3 succeeds either way
    // (verified), but there's no upside to removing first — so keep deleting
    // first. Best-effort; see `opencode::remove_sessions_for_directory`.
    crate::opencode::remove_sessions_for_directory(&path);
    remove_under(&repo, &worktrees_root, project, feature, delete_branch)?;
    Ok(())
}

/// [`remove`] with explicit repo/worktrees roots, so it's testable without
/// touching `$HOME`.
fn remove_under(
    repo: &Path,
    worktrees_root: &Path,
    project: &str,
    feature: &str,
    delete_branch: bool,
) -> Result<()> {
    let path = worktrees_root.join(project).join(feature);
    if !path.exists() {
        return Err(Error::WorktreeNotFound(format!("{project}/{feature}")));
    }

    let project_root = worktrees_root.join(project);
    remove_slug_symlink(&project_root, &path)?;

    // Not a real git worktree — e.g. a stale leftover directory whose git
    // registration is already gone (or was never created). `git worktree
    // remove` only knows about registered worktrees, so just delete the
    // directory directly instead of shelling out to it.
    if !path.join(".git").exists() {
        std::fs::remove_dir_all(&path)?;
        return Ok(());
    }

    // Capture the *live* branch before removal — it may differ from the
    // feature-derived name if this worktree was renamed.
    let branch = git::status(&path).map(|s| s.branch).unwrap_or_else(|e| {
        tracing::warn!(
            "remove: git status failed for {path:?}, falling back to derived branch name: {e}"
        );
        config::branch_name(feature)
    });

    // `--force` is required whenever the worktree has modified or untracked
    // files — `git worktree remove` refuses otherwise. The frontend already
    // warns the user that uncommitted changes will be lost and requires
    // explicit confirmation before calling this, so proceed unconditionally.
    git::git(
        repo,
        &["worktree", "remove", "--force", &path.to_string_lossy()],
    )?;

    if delete_branch {
        match git::git(repo, &["branch", "-d", &branch]) {
            Ok(_) => {
                // Local branch was merged and deleted — mirror that on the
                // remote so a future worktree reusing this feature name
                // doesn't see a stale `origin/<branch>`. Best-effort: no
                // remote configured, an unpushed branch, or a dead network
                // all just log here.
                //
                // `--no-verify` is load-bearing, not just tidiness: a repo
                // with a `pre-push` hook (e.g. a local CI gate running
                // lint/build/clippy) otherwise runs that entire gate before
                // this throwaway cleanup push. Its output is swallowed by
                // `git::git`'s captured `Command::output()`, so from the UI
                // the "Remove worktree" confirm dialog just spins for however
                // long the hook takes (minutes, or forever if it prompts),
                // with nothing to explain why. Deleting a remote branch is
                // pure cleanup — it must never be gated behind the user's
                // local pre-push checks, which can also fail for reasons
                // unrelated to this deletion and leave a stale remote branch.
                if let Err(e) = git::git(
                    repo,
                    &["push", "--no-verify", "origin", "--delete", &branch],
                ) {
                    tracing::debug!("remove: git push origin --delete {branch} failed: {e}");
                }
            }
            // Best-effort: an unmerged branch refuses `-d`; surface nothing here.
            Err(e) => {
                tracing::debug!("remove: git branch -d {branch} failed (likely unmerged): {e}")
            }
        }
    }

    // Otherwise a later worktree reusing this project/feature slug would
    // inherit this session id and offer to `--resume` an unrelated worktree.
    if let Err(e) = agent_session::remove(&agent_session::key(project, feature)) {
        tracing::warn!(
            "remove: failed to clear session store entry for {project}/{feature}, a future worktree reusing this slug may wrongly offer --resume: {e}"
        );
    }
    // Otherwise a later worktree reusing this project/feature slug would
    // inherit this note and display an unrelated worktree's leftover text.
    if let Err(e) = crate::worktree_notes::set(&agent_session::key(project, feature), "") {
        tracing::warn!(
            "remove: failed to clear worktree note for {project}/{feature}, a future worktree reusing this slug may wrongly inherit it: {e}"
        );
    }
    // Otherwise a later worktree reusing this project/feature slug would
    // inherit this title and display an unrelated worktree's leftover name.
    if let Err(e) = crate::worktree_settings::set_title(&agent_session::key(project, feature), "") {
        tracing::warn!(
            "remove: failed to clear worktree title for {project}/{feature}, a future worktree reusing this slug may wrongly inherit it: {e}"
        );
    }
    tracing::info!("removed worktree {project}/{feature}");
    Ok(())
}

/// Drop any session-store, worktree-note, and worktree-title entries left
/// behind by a worktree that no longer exists on disk — e.g. one removed
/// out-of-band (`git worktree remove` run directly, or by an agent
/// bypassing corral) rather than through [`remove`], which already clears
/// its own entries. Meant to run once at app startup as a self-healing
/// sweep; scans the worktree list once and shares it across all three
/// prunes, rather than each running its own independent `list()` (dir walk
/// + a `git status` spawn per worktree). Returns `(sessions_pruned,
/// notes_pruned, titles_pruned)`.
pub fn prune_orphaned_state() -> Result<(usize, usize, usize)> {
    let live_keys = live_session_keys(&list()?);
    Ok((
        agent_session::retain_keys(&live_keys)?,
        crate::worktree_notes::retain_keys(&live_keys)?,
        crate::worktree_settings::retain_keys(&live_keys)?,
    ))
}

/// The session-store keys for every worktree currently on disk, so
/// [`agent_session::retain_keys`] knows which entries are still live.
fn live_session_keys(worktrees: &[Worktree]) -> std::collections::HashSet<String> {
    worktrees
        .iter()
        .map(|w| agent_session::key(&w.project, &w.feature))
        .collect()
}

/// Find a worktree by feature name across all projects.
///
/// Used by the CLI's `open` / `done`, which take only a feature. Errors if the
/// feature is ambiguous across multiple projects.
pub fn find_by_feature(feature: &str) -> Result<Worktree> {
    let mut matches: Vec<Worktree> = list()?
        .into_iter()
        .filter(|w| w.feature == feature)
        .collect();

    if matches.len() > 1 {
        return Err(Error::AmbiguousFeature {
            feature: feature.to_string(),
            projects: matches
                .iter()
                .map(|w| w.project.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    matches
        .pop()
        .ok_or_else(|| Error::WorktreeNotFound(feature.to_string()))
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

    /// Clears a worktree's title in the real (un-parameterized) settings
    /// store on drop, including on panic (e.g. a failed assertion) — used by
    /// any test whose `rename_under`/`remove_under` call writes through to
    /// the developer's actual `~/.local/share/corral` store, since that
    /// store isn't parameterized by a test's tempdir the way the
    /// filesystem/`worktrees_root` side is.
    struct RealTitleStoreGuard {
        key: String,
    }

    impl Drop for RealTitleStoreGuard {
        fn drop(&mut self) {
            let _ = crate::worktree_settings::set_title(&self.key, "");
        }
    }

    /// A bare `~/source/<project>` main repo with one commit on `main`.
    fn init_repo(source_root: &Path, project: &str) -> PathBuf {
        let repo = source_root.join(project);
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init", "--quiet", "--initial-branch=main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("README.md"), "hello\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "--quiet", "-m", "initial"]);
        repo
    }

    #[test]
    fn create_under_makes_a_worktree_on_a_new_branch() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");

        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        assert_eq!(wt.project, "corral");
        assert_eq!(wt.feature, "weeding");
        assert_eq!(wt.branch, "feat/weeding");
        assert!(wt.path.exists());
        assert!(wt.status.is_some());
    }

    #[test]
    fn create_under_branches_off_the_latest_fetched_remote_commit() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");

        // Give `repo` a remote whose `main` has a commit `repo`'s local
        // `main` doesn't have yet — simulates another machine having pushed
        // since this repo was last pulled.
        let origin = tempfile::tempdir().unwrap();
        std::fs::remove_dir(origin.path()).unwrap();
        run_git(
            &repo,
            &[
                "clone",
                "--quiet",
                "--bare",
                ".",
                origin.path().to_str().unwrap(),
            ],
        );
        run_git(
            &repo,
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

        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        assert!(wt.path.join("remote-only.txt").exists());

        // Regression: the new worktree's Changes pane must not report the
        // remote-only commit as a change, even though the main repo's local
        // `main` is still behind `origin/main` — the diff base is
        // `origin/<base>`, not the local branch ref (see `resolve_diff_base`
        // in git.rs).
        let changes = crate::git::branch_changes(&wt.path).unwrap();
        assert_eq!(changes.base.as_deref(), Some("main"));
        assert!(
            changes.files.is_empty(),
            "a fresh worktree branched off the fetched remote must be clean, got: {:?}",
            changes.files
        );
    }

    #[test]
    fn create_under_leaves_a_new_branch_without_an_upstream() {
        // Regression: git's `branch.autoSetupMerge` default makes
        // `worktree add -b <branch> origin/main` configure the new branch to
        // track `origin/main`. A fresh corral worktree must instead start out
        // with no upstream, so `git status`/`git pull`/`git push` don't target
        // the base branch before `force_push_branch`'s `-u` sets the real one.
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");

        // Give `repo` an `origin` so `create_under` branches off the
        // remote-tracking `origin/main` rather than local `main`.
        let origin = tempfile::tempdir().unwrap();
        std::fs::remove_dir(origin.path()).unwrap();
        run_git(
            &repo,
            &[
                "clone",
                "--quiet",
                "--bare",
                ".",
                origin.path().to_str().unwrap(),
            ],
        );
        run_git(
            &repo,
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );

        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        let output = std::process::Command::new("git")
            .args(["config", "--get", "branch.feat/weeding.merge"])
            .current_dir(&wt.path)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "fresh worktree branch must have no upstream, but branch.feat/weeding.merge was {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    fn create_under_keeps_its_claude_symlink_out_of_the_changes_pane() {
        // Regression: corral symlinks `<worktree>/.claude` back to the main
        // repo's shared settings dir at creation. For a project that doesn't
        // gitignore `.claude` itself, that symlink would otherwise show up as
        // an untracked change in the Changes pane for the whole life of the
        // worktree. Masked on machines with a global `.claude` ignore; caught
        // on CI, which has none (see
        // create_under_branches_off_the_latest_fetched_remote_commit).
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");

        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        assert!(wt.path.join(".claude").symlink_metadata().is_ok());
        let changes = crate::git::changed_files(&wt.path).unwrap();
        assert!(
            !changes.iter().any(|f| f.path == ".claude"),
            "corral's own .claude symlink must not show as an untracked change, got: {:?}",
            changes
        );
    }

    #[test]
    fn create_under_copies_configured_files_from_the_repo() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        std::fs::write(repo.join("CLAUDE.md"), "notes\n").unwrap();

        let project_settings = crate::project_settings::ProjectSettings {
            path: PathBuf::new(),
            copy_globs: vec!["CLAUDE.md".into()],
            setup_command: String::new(),
            run_command: String::new(),
            project_prompt: String::new(),
            env: vec![],
        };
        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &project_settings,
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(wt.path.join("CLAUDE.md")).unwrap(),
            "notes\n"
        );
    }

    #[test]
    fn create_under_copies_files_matching_a_nested_glob_pattern() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        std::fs::write(repo.join("docs").join("guide.md"), "guide\n").unwrap();
        std::fs::write(repo.join("docs").join("notes.txt"), "skip me\n").unwrap();

        let project_settings = crate::project_settings::ProjectSettings {
            path: PathBuf::new(),
            copy_globs: vec!["docs/*.md".into()],
            setup_command: String::new(),
            run_command: String::new(),
            project_prompt: String::new(),
            env: vec![],
        };
        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &project_settings,
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(wt.path.join("docs").join("guide.md")).unwrap(),
            "guide\n"
        );
        assert!(!wt.path.join("docs").join("notes.txt").exists());
    }

    #[test]
    fn create_under_never_copies_the_git_directory_even_with_a_catch_all_glob() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");

        let project_settings = crate::project_settings::ProjectSettings {
            path: PathBuf::new(),
            copy_globs: vec!["**/*".into()],
            setup_command: String::new(),
            run_command: String::new(),
            project_prompt: String::new(),
            env: vec![],
        };
        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &project_settings,
        )
        .unwrap();

        // `git worktree add` already creates its own `.git` file/pointer;
        // the copy step must never overwrite it with the main repo's real
        // `.git` directory contents.
        assert!(wt.path.join(".git").exists());
        assert!(!wt.path.join(".git").is_dir());
    }

    #[test]
    fn create_under_checks_out_an_existing_branch_instead_of_recreating_it() {
        // Regression: the old create path treated *any* `worktree add -b`
        // failure as "branch exists" and retried with a plain checkout,
        // masking unrelated failures behind the fallback's error. Now the
        // branch-existence check is explicit, so a pre-existing branch (e.g.
        // from a prior worktree removed with `--keep-branch`) checks out its
        // current content rather than branching fresh off main.
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        run_git(&repo, &["checkout", "--quiet", "-b", "feat/weeding"]);
        std::fs::write(repo.join("existing.txt"), "existing\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "--quiet", "-m", "existing branch work"]);
        run_git(&repo, &["checkout", "--quiet", "main"]);

        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        assert_eq!(wt.branch, "feat/weeding");
        // The worktree checked out the existing branch's content, which a
        // fresh branch off `main` would not have.
        assert!(wt.path.join("existing.txt").exists());
    }

    #[test]
    fn create_under_errors_if_repo_is_not_a_git_repo() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source_root.path().join("corral")).unwrap();

        let result = create_under(
            &source_root.path().join("corral"),
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        );
        assert!(matches!(result, Err(Error::NotAGitRepo(_))));
    }

    #[test]
    fn create_under_errors_if_worktree_path_already_exists() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        std::fs::create_dir_all(worktrees_root.path().join("corral").join("weeding")).unwrap();

        let result = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        );
        assert!(matches!(result, Err(Error::WorktreeExists(_))));
    }

    #[test]
    fn list_under_returns_empty_for_a_missing_root() {
        let missing = tempfile::tempdir().unwrap();
        let root = missing.path().join("does-not-exist");
        assert!(list_under(&root).unwrap().is_empty());
    }

    #[test]
    fn list_under_finds_a_created_worktree() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        let found = list_under(worktrees_root.path()).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].project, "corral");
        assert_eq!(found[0].feature, "weeding");
        assert_eq!(found[0].branch, "feat/weeding");
        assert_eq!(found[0].slug, "weeding");
    }

    #[test]
    fn list_under_returns_every_worktree_in_order_across_many_worktrees() {
        // Exercises the bounded status-fetch worker pool: every worktree must
        // come back, in `found` order, with a status — a dropped or reordered
        // slot (e.g. a panicked worker shrinking the results vec) would show
        // up here.
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        let features = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
        for f in features {
            create_under(
                &repo,
                worktrees_root.path(),
                "corral",
                f,
                &crate::project_settings::ProjectSettings::default(),
            )
            .unwrap();
        }

        let found = list_under(worktrees_root.path()).unwrap();

        let got: Vec<&str> = found.iter().map(|w| w.feature.as_str()).collect();
        assert_eq!(got, features);
        assert!(found.iter().all(|w| w.status.is_some()));
    }

    #[test]
    fn list_feature_names_under_lists_physical_dirs_and_slug_symlinks() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "herding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        // A slug symlink (as `rename` creates) occupies a directory name under
        // `<project>/`, so it must be treated as "taken": `names::generate`
        // must not pick it, or a subsequent `create` of that name would fail
        // `WorktreeExists` (its `path.exists()` follows the symlink).
        std::os::unix::fs::symlink(
            worktrees_root.path().join("corral").join("weeding"),
            worktrees_root.path().join("corral").join("pretty-alias"),
        )
        .unwrap();

        let names = list_feature_names_under(worktrees_root.path(), "corral").unwrap();

        let expected: std::collections::HashSet<String> = [
            "weeding".to_string(),
            "herding".to_string(),
            "pretty-alias".to_string(),
        ]
        .into_iter()
        .collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn list_under_includes_an_empty_note_for_a_fresh_worktree() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        let found = list_under(worktrees_root.path()).unwrap();

        assert_eq!(found[0].note, "");
    }

    /// Builds a minimal registered-projects `Store` containing exactly
    /// `names`, with otherwise-default `ProjectSettings` — enough for
    /// `filter_to_registered_projects`, which only inspects key membership.
    fn registered(names: &[&str]) -> crate::project_settings::Store {
        names
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    crate::project_settings::ProjectSettings::default(),
                )
            })
            .collect()
    }

    #[test]
    fn filter_to_registered_projects_keeps_worktrees_of_a_registered_project() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        let found = list_under(worktrees_root.path()).unwrap();

        let filtered = filter_to_registered_projects(found, &registered(&["corral"]));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].project, "corral");
    }

    #[test]
    fn filter_to_registered_projects_drops_worktrees_of_an_unregistered_project() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        let found = list_under(worktrees_root.path()).unwrap();

        // Simulates `remove_project` unregistering "corral" while its
        // worktree directory is deliberately left on disk.
        let filtered = filter_to_registered_projects(found, &registered(&["other-project"]));

        assert!(filtered.is_empty());
    }

    #[test]
    fn filter_to_registered_projects_with_empty_registry_drops_everything() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        let found = list_under(worktrees_root.path()).unwrap();

        let filtered = filter_to_registered_projects(found, &registered(&[]));

        assert!(filtered.is_empty());
    }

    #[test]
    fn filter_to_registered_projects_drops_all_worktrees_of_the_same_unregistered_project() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "herding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        let found = list_under(worktrees_root.path()).unwrap();
        assert_eq!(found.len(), 2);

        let filtered = filter_to_registered_projects(found, &registered(&["other-project"]));

        assert!(filtered.is_empty());
    }

    #[test]
    fn live_session_keys_matches_worktrees_on_disk() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        let found = list_under(worktrees_root.path()).unwrap();
        let keys = live_session_keys(&found);

        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&agent_session::key("corral", "weeding")));
        // A worktree removed out-of-band no longer appears here, so
        // `agent_session::retain_keys` would drop its now-orphaned entry.
        assert!(!keys.contains(&agent_session::key("corral", "gone")));
    }

    #[test]
    fn list_under_orders_by_creation_time_not_name() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        // Named so alphabetical order would be the reverse of creation order.
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "zebra",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "apple",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        let found = list_under(worktrees_root.path()).unwrap();

        let features: Vec<&str> = found.iter().map(|w| w.feature.as_str()).collect();
        assert_eq!(features, vec!["zebra", "apple"]);
    }

    #[test]
    fn rename_under_renames_branch_and_adds_a_slug_symlink() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        // `rename_under` below writes a real title into the developer's
        // actual `~/.local/share/corral` settings store (see the guard's
        // doc comment) — this guard clears it on drop, even if an
        // assertion below panics.
        let _title_guard = RealTitleStoreGuard {
            key: crate::agent_session::key("corral", "weeding"),
        };

        let renamed =
            rename_under(worktrees_root.path(), "corral", "weeding", "Herding Cats").unwrap();

        assert_eq!(renamed.slug, "herding-cats");
        assert_eq!(renamed.branch, "feat/herding-cats");
        assert_eq!(renamed.title, "Herding Cats");
        // Physical path is unchanged — only the branch and a cosmetic alias moved.
        assert_eq!(
            renamed.path,
            worktrees_root.path().join("corral").join("weeding")
        );
        assert!(
            worktrees_root
                .path()
                .join("corral")
                .join("herding-cats")
                .symlink_metadata()
                .is_ok()
        );
    }

    #[test]
    fn rename_through_a_symlinked_root_is_reflected_by_list() {
        // Regression: `list_under` looked up slug aliases by the raw
        // `read_dir` path, while `slug_symlinks` keyed them by the *resolved*
        // (canonicalized) symlink target. When the worktrees root itself
        // traverses a symlink (e.g. `~/source` -> an external drive), those
        // two spellings differ and every rename slug silently fell back to the
        // physical feature name.
        let real_root = tempfile::tempdir().unwrap();
        let link_parent = tempfile::tempdir().unwrap();
        let link_root = link_parent.path().join("worktrees");
        std::os::unix::fs::symlink(real_root.path(), &link_root).unwrap();

        let source_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            &link_root,
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        let _title_guard = RealTitleStoreGuard {
            key: crate::agent_session::key("corral", "weeding"),
        };

        rename_under(&link_root, "corral", "weeding", "Herding Cats").unwrap();

        let found = list_under(&link_root).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].slug, "herding-cats");
    }

    #[test]
    fn rename_under_errors_if_worktree_is_missing() {
        let worktrees_root = tempfile::tempdir().unwrap();
        let result = rename_under(worktrees_root.path(), "corral", "missing", "new-name");
        assert!(matches!(result, Err(Error::WorktreeNotFound(_))));
    }

    #[test]
    fn rename_under_refuses_a_slug_that_collides_with_a_sibling_before_mutating() {
        // Regression: the branch rename and slug-symlink swap aren't atomic,
        // so renaming onto a sibling's occupied name used to move the branch
        // and remove the old alias, then fail creating the new alias — leaving
        // the worktree half-renamed. The collision must be rejected up front.
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "herding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        let _title_guard = RealTitleStoreGuard {
            key: crate::agent_session::key("corral", "weeding"),
        };

        // "herding" is another worktree's physical directory.
        let result = rename_under(worktrees_root.path(), "corral", "weeding", "herding");
        assert!(matches!(result, Err(Error::WorktreeExists(_))));

        // Nothing mutated: the original branch and alias are intact.
        let found = list_under(worktrees_root.path()).unwrap();
        let weeding = found.iter().find(|w| w.feature == "weeding").unwrap();
        assert_eq!(weeding.branch, "feat/weeding");
        assert_eq!(weeding.slug, "weeding");
    }

    #[test]
    fn remove_under_deletes_the_worktree_directory() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = init_repo(source_root.path(), "corral");
        create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();

        remove_under(&repo, worktrees_root.path(), "corral", "weeding", false).unwrap();

        assert!(
            !worktrees_root
                .path()
                .join("corral")
                .join("weeding")
                .exists()
        );
    }

    /// Regression: the remote-branch cleanup push must bypass `pre-push`
    /// hooks. `git push --delete` runs them too, and a repo whose hook runs a
    /// local CI gate (lint/build/clippy) would otherwise turn the Remove
    /// action into an apparent hang — the hook's output is captured by
    /// `git::git`, so the UI just spins with nothing to explain why. A hook
    /// that rejects the push would also leave the remote branch behind even
    /// though the local branch was already deleted.
    #[test]
    fn remove_under_deletes_remote_branch_without_running_pre_push_hooks() {
        let source_root = tempfile::tempdir().unwrap();
        let worktrees_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let remote = remote_root.path().join("origin.git");

        let repo = init_repo(source_root.path(), "corral");
        run_git(
            remote_root.path(),
            &["init", "--quiet", "--bare", remote.to_str().unwrap()],
        );
        run_git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        run_git(&repo, &["push", "--quiet", "origin", "main"]);

        // Installed after the setup pushes so only the remove-path push is
        // subject to it. `exit 1` makes any hook-running push fail outright.
        let hook = repo.join(".git").join("hooks").join("pre-push");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let wt = create_under(
            &repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            &crate::project_settings::ProjectSettings::default(),
        )
        .unwrap();
        std::fs::write(wt.path.join("work.txt"), "work\n").unwrap();
        run_git(&wt.path, &["add", "."]);
        run_git(&wt.path, &["commit", "--quiet", "-m", "work"]);
        // `-u` gives the branch an upstream equal to its own tip, so
        // `git branch -d` treats it as merged and the remote-delete push is
        // actually reached (`--no-verify` bypasses the failing hook here).
        run_git(
            &wt.path,
            &[
                "push",
                "--quiet",
                "--no-verify",
                "-u",
                "origin",
                "feat/weeding",
            ],
        );
        assert!(
            git::git(
                &repo,
                &["rev-parse", "--verify", "refs/remotes/origin/feat/weeding"]
            )
            .is_ok(),
            "precondition: remote branch exists"
        );

        remove_under(&repo, worktrees_root.path(), "corral", "weeding", true).unwrap();

        assert!(
            git::git(
                &repo,
                &["rev-parse", "--verify", "refs/remotes/origin/feat/weeding"]
            )
            .is_err(),
            "remote branch should be deleted without invoking the failing pre-push hook"
        );
    }

    #[test]
    fn remove_under_removes_a_stale_non_git_directory_directly() {
        let worktrees_root = tempfile::tempdir().unwrap();
        let stale = worktrees_root.path().join("corral").join("weeding");
        std::fs::create_dir_all(&stale).unwrap();
        // No repo needed for this path: a directory with no `.git` is deleted
        // outright, without shelling out to `git worktree remove`.
        let unused_repo = worktrees_root.path().join("unused-repo");

        remove_under(
            &unused_repo,
            worktrees_root.path(),
            "corral",
            "weeding",
            false,
        )
        .unwrap();

        assert!(!stale.exists());
    }

    #[test]
    fn remove_under_errors_if_worktree_is_missing() {
        let worktrees_root = tempfile::tempdir().unwrap();
        let repo = worktrees_root.path().join("unused-repo");
        let result = remove_under(&repo, worktrees_root.path(), "corral", "missing", false);
        assert!(matches!(result, Err(Error::WorktreeNotFound(_))));
    }

    #[test]
    fn slug_symlinks_maps_target_to_alias_name() {
        let dir = tempfile::tempdir().unwrap();
        let worktree = dir.path().join("physical-feature");
        std::fs::create_dir(&worktree).unwrap();
        std::os::unix::fs::symlink(&worktree, dir.path().join("pretty-alias")).unwrap();

        let map = slug_symlinks(dir.path()).unwrap();
        assert_eq!(
            map.get(&std::fs::canonicalize(&worktree).unwrap()),
            Some(&"pretty-alias".to_string())
        );
    }

    #[test]
    fn slug_symlinks_ignores_plain_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("not-a-symlink")).unwrap();

        let map = slug_symlinks(dir.path()).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn slug_symlinks_skips_dangling_symlink() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("does-not-exist"),
            dir.path().join("dangling"),
        )
        .unwrap();

        let map = slug_symlinks(dir.path()).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn remove_slug_symlink_removes_matching_alias_only() {
        let dir = tempfile::tempdir().unwrap();
        let target_a = dir.path().join("worktree-a");
        let target_b = dir.path().join("worktree-b");
        std::fs::create_dir(&target_a).unwrap();
        std::fs::create_dir(&target_b).unwrap();
        std::os::unix::fs::symlink(&target_a, dir.path().join("alias-a")).unwrap();
        std::os::unix::fs::symlink(&target_b, dir.path().join("alias-b")).unwrap();

        remove_slug_symlink(dir.path(), &target_a).unwrap();

        assert!(!dir.path().join("alias-a").exists());
        assert!(dir.path().join("alias-b").symlink_metadata().is_ok());
    }

    #[test]
    fn remove_slug_symlink_on_missing_root_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let missing_root = dir.path().join("does-not-exist");
        remove_slug_symlink(&missing_root, &dir.path().join("whatever")).unwrap();
    }

    #[test]
    fn remove_slug_symlink_no_match_leaves_symlinks_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("worktree");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("alias")).unwrap();

        remove_slug_symlink(dir.path(), &dir.path().join("some-other-target")).unwrap();

        assert!(dir.path().join("alias").symlink_metadata().is_ok());
    }

    #[test]
    fn remove_slug_symlink_resolves_a_target_spelled_through_a_symlinked_root() {
        // Regression: `remove_slug_symlink` compared each symlink's
        // canonicalized target against the raw `target` passed in, so a target
        // spelled through a symlinked ancestor never matched and the alias was
        // left behind (dangling) after the worktree was removed.
        let real = tempfile::tempdir().unwrap();
        let link_parent = tempfile::tempdir().unwrap();
        let link = link_parent.path().join("root");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();
        let project_root = link.join("corral");
        std::fs::create_dir_all(&project_root).unwrap();
        let target = link.join("corral").join("weeding");
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, project_root.join("alias")).unwrap();

        remove_slug_symlink(&project_root, &target).unwrap();

        assert!(!project_root.join("alias").exists());
    }
}
