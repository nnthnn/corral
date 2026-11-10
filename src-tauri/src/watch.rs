//! Watches each project's worktree root so the app notices renames,
//! creates, and removes done outside the GUI — e.g. an agent running
//! `corral rename` from its own PTY session — without waiting for a manual
//! refresh. Deliberately non-recursive: a recursive watch on the worktrees
//! root would also fire on every file save/git operation *inside* each
//! worktree, which is both noisy and irrelevant here. We only care about
//! entries appearing/disappearing directly under `~/source/worktrees/
//! <project>` (new/removed worktrees, and the slug symlink
//! `worktree::rename` swaps) — project discovery itself is no longer
//! filesystem-driven (see `corral_core::project_settings`'s registry), so
//! this module no longer watches `~/source` at all.
//!
//! Event *payloads* aren't inspected beyond their kind — any create/remove
//! just re-triggers the frontend's existing `list_projects`/`list_worktrees`
//! refresh, since re-deriving state from git (rather than trusting fs event
//! details) is already this codebase's convention. Event *kind* is filtered,
//! though: a non-recursive watch on `~/source/worktrees/<project>` still
//! reports Modify/Access events for a worktree directory whenever something
//! changes *inside* it (e.g. `git status`, a build writing to `target/`, an
//! agent saving a file) — Linux surfaces those as metadata-modify events on
//! the immediate child entry itself, not just the true create/remove events
//! this module cares about. Reacting to all of them turned a single active
//! worktree into a full-app refresh storm several times a second. Only
//! `Create`/`Remove` kinds represent an actual worktree appearing/
//! disappearing, so everything else is ignored.

use std::collections::HashSet;
use std::path::Path;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || watch_loop(app));
}

fn watch_loop(app: AppHandle) {
    let Ok(worktrees_root) = corral_core::config::worktrees_root() else {
        return;
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let Ok(mut watcher) = RecommendedWatcher::new(tx, notify::Config::default()) else {
        return;
    };

    // Best-effort: the root may not exist yet on a fresh machine.
    if let Err(e) = watcher.watch(&worktrees_root, RecursiveMode::NonRecursive) {
        tracing::warn!(
            "failed to watch worktrees root {}: {e}",
            worktrees_root.display()
        );
    }

    let mut watched_projects: HashSet<String> = HashSet::new();
    watch_new_projects(&mut watcher, &worktrees_root, &mut watched_projects);

    while let Ok(first) = rx.recv() {
        let mut relevant = is_relevant(&first);
        // Coalesce a burst of events (e.g. a rename's unlink + symlink) into
        // one refresh instead of one per fs event.
        while let Ok(next) = rx.recv_timeout(corral_core::watch::WATCH_DEBOUNCE) {
            relevant = relevant || is_relevant(&next);
        }
        if !relevant {
            continue;
        }

        watch_new_projects(&mut watcher, &worktrees_root, &mut watched_projects);
        let _ = app.emit("fs-changed", ());
    }
}

/// Whether an event represents a real create/remove (a worktree or project
/// appearing/disappearing) rather than routine churn inside an existing one.
fn is_relevant(event: &notify::Result<notify::Event>) -> bool {
    matches!(
        event,
        Ok(notify::Event {
            kind: EventKind::Create(_) | EventKind::Remove(_),
            ..
        })
    )
}

/// Start watching any project's worktree dir we haven't seen before, so a
/// newly-onboarded project's renames are picked up without an app restart.
fn watch_new_projects(
    watcher: &mut RecommendedWatcher,
    worktrees_root: &Path,
    watched: &mut HashSet<String>,
) {
    for project in corral_core::project::list().unwrap_or_default() {
        if watched.insert(project.clone()) {
            let project_root = worktrees_root.join(&project);
            if let Err(e) = watcher.watch(&project_root, RecursiveMode::NonRecursive) {
                tracing::warn!("failed to watch {}: {e}", project_root.display());
            }
        }
    }
}
