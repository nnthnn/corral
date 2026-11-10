//! Watches exactly one worktree at a time — whichever is currently selected
//! in the frontend — recursively, so the Changes pane learns about edits,
//! new/deleted files, and commits within ~200ms regardless of what caused
//! them (an agent's PTY, another terminal, an editor, `git checkout`, etc).
//! Distinct from `watch.rs`, which stays non-recursive and scoped to
//! `~/source` for detecting new/removed *worktrees themselves*.
//!
//! One background thread owns the currently-active `notify::Watcher` and
//! swaps it out synchronously on `switch()`, so switching worktrees always
//! drops (and thus unwatches) the previous watcher rather than leaking an
//! inotify watch on an old selection.

use std::path::PathBuf;
use std::sync::mpsc;

use ignore::gitignore::Gitignore;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

enum Msg {
    Switch {
        project: String,
        feature: String,
        root: PathBuf,
    },
    Fs(notify::Result<notify::Event>),
}

#[derive(Clone, Serialize)]
struct WorktreeChanged {
    project: String,
    feature: String,
    /// True when the burst included a git-metadata change (commit/checkout/
    /// stage) — the frontend responds to those with a full `git::status`
    /// refresh (expensive merge-base/merge-tree/numstat derived fields), but
    /// skips it for ordinary file edits, which only move the Changes pane.
    git_changed: bool,
}

/// Handle held in Tauri's managed state; the `watch_worktree` command uses
/// it to tell the background thread which worktree to watch next.
pub struct WorktreeWatch {
    tx: mpsc::Sender<Msg>,
}

impl WorktreeWatch {
    pub fn switch(&self, project: String, feature: String, root: PathBuf) {
        let _ = self.tx.send(Msg::Switch {
            project,
            feature,
            root,
        });
    }
}

pub fn spawn(app: AppHandle) -> WorktreeWatch {
    let (tx, rx) = mpsc::channel::<Msg>();
    let worker_tx = tx.clone();
    std::thread::spawn(move || worker_loop(app, rx, worker_tx));
    WorktreeWatch { tx }
}

struct WatchedWorktree {
    project: String,
    feature: String,
    root: PathBuf,
    ignore: Gitignore,
    git_paths: Vec<PathBuf>,
    // Kept alive only so its Drop (on reassignment) stops watching; never read.
    _watcher: Option<RecommendedWatcher>,
}

fn worker_loop(app: AppHandle, rx: mpsc::Receiver<Msg>, tx: mpsc::Sender<Msg>) {
    let mut current: Option<WatchedWorktree> = None;

    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Switch {
                project,
                feature,
                root,
            } => {
                current = Some(start_watch(&tx, project, feature, root));
            }
            Msg::Fs(event) => {
                let Some(cur) = &current else { continue };
                if !corral_core::watch::is_relevant(&event, &cur.root, &cur.git_paths, &cur.ignore)
                {
                    continue;
                }
                let mut git_changed =
                    corral_core::watch::is_git_metadata_event(&event, &cur.git_paths);
                match drain_burst(&rx, &cur.git_paths) {
                    Burst::Switch {
                        project,
                        feature,
                        root,
                    } => {
                        current = Some(start_watch(&tx, project, feature, root));
                    }
                    Burst::Done {
                        git_changed: drained,
                    } => {
                        git_changed |= drained;
                        let _ = app.emit(
                            "worktree-changed",
                            WorktreeChanged {
                                project: cur.project.clone(),
                                feature: cur.feature.clone(),
                                git_changed,
                            },
                        );
                    }
                }
            }
        }
    }
}

/// How a debounce-window burst resolved.
enum Burst {
    /// A worktree switch arrived mid-burst — the caller should switch
    /// immediately instead of emitting for the worktree being left behind.
    Switch {
        project: String,
        feature: String,
        root: PathBuf,
    },
    /// The debounce window elapsed with no switch; the caller should emit for
    /// the current worktree. `git_changed` reports whether any fs event in the
    /// burst touched git metadata (commit/checkout/stage).
    Done { git_changed: bool },
}

/// Coalesces a burst of fs events into a single refresh, tracking whether any
/// of them touched git metadata so the caller can decide between a cheap
/// Changes-pane refresh (ordinary edits) and a full status refresh (commits).
fn drain_burst(rx: &mpsc::Receiver<Msg>, git_paths: &[PathBuf]) -> Burst {
    let mut git_changed = false;
    loop {
        match rx.recv_timeout(corral_core::watch::WATCH_DEBOUNCE) {
            Ok(Msg::Switch {
                project,
                feature,
                root,
            }) => {
                return Burst::Switch {
                    project,
                    feature,
                    root,
                };
            }
            // Deliberately not re-checked against `is_relevant` here: sustained irrelevant
            // churn (e.g. a build writing to a gitignored `target/`) can keep resetting this
            // timer and delay (never drop or duplicate) the eventual emit until it pauses.
            Ok(Msg::Fs(event)) => {
                git_changed |= corral_core::watch::is_git_metadata_event(&event, git_paths);
                continue;
            }
            Err(_) => return Burst::Done { git_changed },
        }
    }
}

fn start_watch(
    tx: &mpsc::Sender<Msg>,
    project: String,
    feature: String,
    root: PathBuf,
) -> WatchedWorktree {
    let ignore = corral_core::watch::build_gitignore(&root);
    // Resolved via `git rev-parse --git-path` (not `root.join(".git")`,
    // which is a *file* for a real worktree, not a directory) — see
    // `corral_core::watch`'s module doc.
    let git_paths = corral_core::watch::git_metadata_paths(&root);
    let tx_fs = tx.clone();
    let watcher = RecommendedWatcher::new(
        move |event| {
            let _ = tx_fs.send(Msg::Fs(event));
        },
        notify::Config::default(),
    )
    .ok()
    .map(|mut w| {
        if w.watch(&root, RecursiveMode::Recursive).is_err() {
            tracing::warn!(?root, "failed to start recursive worktree watch");
        }
        for path in &git_paths {
            if w.watch(path, RecursiveMode::Recursive).is_err() {
                tracing::warn!(?path, "failed to watch git metadata path");
            }
        }
        w
    });
    if watcher.is_none() {
        tracing::warn!(?root, "could not construct filesystem watcher for worktree");
    }
    WatchedWorktree {
        project,
        feature,
        root,
        ignore,
        git_paths,
        _watcher: watcher,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{Event, EventKind};

    fn switch(project: &str) -> Msg {
        Msg::Switch {
            project: project.to_string(),
            feature: "f".to_string(),
            root: PathBuf::from("/tmp/root"),
        }
    }

    fn fs() -> Msg {
        Msg::Fs(Ok(Event::new(EventKind::Any)))
    }

    #[test]
    fn no_messages_times_out_to_done_without_git_changes() {
        let (_tx, rx) = mpsc::channel();
        match drain_burst(&rx, &[]) {
            Burst::Done { git_changed } => assert!(!git_changed),
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn fs_events_alone_time_out_to_done_without_git_changes() {
        let (tx, rx) = mpsc::channel();
        tx.send(fs()).unwrap();
        tx.send(fs()).unwrap();
        match drain_burst(&rx, &[]) {
            Burst::Done { git_changed } => assert!(!git_changed),
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn git_metadata_event_marks_the_burst_as_git_changed() {
        let (tx, rx) = mpsc::channel();
        let head = PathBuf::from("/tmp/root/.git/HEAD");
        tx.send(Msg::Fs(Ok(
            Event::new(EventKind::Any).add_path(head.clone())
        )))
        .unwrap();
        match drain_burst(&rx, &[head]) {
            Burst::Done { git_changed } => assert!(git_changed),
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn switch_returns_immediately() {
        let (tx, rx) = mpsc::channel();
        tx.send(switch("p")).unwrap();
        match drain_burst(&rx, &[]) {
            Burst::Switch {
                project,
                feature,
                root,
            } => {
                assert_eq!(project, "p");
                assert_eq!(feature, "f");
                assert_eq!(root, PathBuf::from("/tmp/root"));
            }
            _ => panic!("switch should short-circuit"),
        }
    }

    #[test]
    fn switch_mid_burst_is_returned_over_fs_noise() {
        let (tx, rx) = mpsc::channel();
        tx.send(fs()).unwrap();
        tx.send(fs()).unwrap();
        tx.send(switch("p2")).unwrap();
        match drain_burst(&rx, &[]) {
            Burst::Switch { project, .. } => assert_eq!(project, "p2"),
            _ => panic!("switch should be returned"),
        }
    }
}
