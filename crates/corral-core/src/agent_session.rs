//! Persisted mapping from worktree (`<project>/<feature>`) to the last known
//! agent session id, tagged with which harness it belongs to, so reopening a
//! worktree can `--resume` the prior conversation instead of starting a
//! fresh one.
//!
//! Stored at `~/.local/share/corral/sessions.json` (XDG data dir, or
//! `CORRAL_DATA_DIR` — see `crate::paths`) rather than
//! inside the worktree itself, so it doesn't touch git state. Lock discipline
//! is shared via `crate::keyed_store` — a sibling `sessions.json.lock` file is
//! held across each read-modify-write so two processes (or corral windows)
//! can't race and silently drop one side's update.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::harness::Harness;
use crate::keyed_store::KeyedStore;
use crate::paths::data_root;

/// A resumable agent session id, tagged with which harness it belongs to
/// (Claude and opencode session ids are not interchangeable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub harness: Harness,
    pub session_id: String,
}

fn store() -> Result<KeyedStore<StoredSession>> {
    Ok(KeyedStore::new(
        data_root()?,
        "sessions.json",
        "agent_session",
        "sessions",
    ))
}

/// Build the `<project>/<feature>` key used to index a worktree's session.
pub fn key(project: &str, feature: &str) -> String {
    format!("{project}/{feature}")
}

/// Look up the last known agent session for a worktree.
pub fn get(key: &str) -> Result<Option<StoredSession>> {
    store()?.get(key)
}

/// Whether a worktree has a prior agent session id it could `--resume`.
pub fn has(key: &str) -> Result<bool> {
    store()?.has(key)
}

/// Every worktree key with a resumable session, for checking many worktrees
/// at once (e.g. `worktree::list()`) without re-reading the store per lookup.
pub fn resumable_keys() -> Result<HashSet<String>> {
    Ok(store()?.all()?.into_keys().collect())
}

/// Record the agent session id in use for a worktree, tagged with `harness`.
pub fn set(key: &str, harness: Harness, session_id: &str) -> Result<()> {
    store()?.set(
        key,
        StoredSession {
            harness,
            session_id: session_id.to_string(),
        },
    )
}

/// Forget the stored session id for a worktree (e.g. starting fresh).
pub fn remove(key: &str) -> Result<()> {
    store()?.remove(key)
}

/// Drop every stored entry whose key isn't in `live` (e.g. a worktree removed
/// out-of-band, bypassing [`crate::worktree::remove`], which would otherwise
/// have called [`remove`] itself). Returns the number of entries dropped.
pub fn retain_keys(live: &HashSet<String>) -> Result<usize> {
    store()?.retain_keys(live)
}

/// Generate a new session id, in the UUID format `claude --session-id` expects.
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_formats_project_and_feature() {
        assert_eq!(key("corral", "weeding"), "corral/weeding");
    }
}
