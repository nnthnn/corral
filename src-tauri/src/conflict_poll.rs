//! Periodically re-fetches `origin` for every worktree with a remote, so
//! `GitStatus.conflicts_with_base` (computed locally on every status
//! refresh, with no fetch of its own — see `corral_core::git::status`)
//! doesn't go stale. Mirrors `idle_timeout.rs`'s shape: one thread spawned
//! once at app startup, running for the process's lifetime.

use std::time::Duration;

use tauri::{AppHandle, Emitter};

/// How often to re-fetch every worktree's `origin`. Coarse on purpose: this
/// only needs to catch drift on the order of minutes, and each tick costs
/// one network round-trip per worktree that has a remote.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(POLL_INTERVAL);

            let worktrees = match corral_core::worktree::list() {
                Ok(worktrees) => worktrees,
                Err(e) => {
                    tracing::warn!("conflict_poll: failed to list worktrees: {e}");
                    continue;
                }
            };

            let fetched = worktrees
                .iter()
                .filter(|wt| corral_core::git::fetch_remote(&wt.path))
                .count();
            if fetched > 0 {
                tracing::info!("conflict_poll: fetched origin for {fetched} worktree(s)");
                // Reuses the same event `watch.rs` emits for worktree
                // create/rename/remove — the frontend's existing `fs-changed`
                // listener (bootstrap.ts) already does a full `loadAll()`
                // refresh in response, which picks up the now-fresh
                // `conflicts_with_base` on every worktree's status.
                let _ = app.emit("fs-changed", ());
            }
        }
    });
}
