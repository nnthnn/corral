//! Persisted free-text note per worktree (`<project>/<feature>`), shown as a
//! subtitle under the worktree's name in the sidebar — a git-untracked memory
//! aid for juggling many parallel worktrees, independent of any code state.
//!
//! Stored at `~/.local/share/corral/worktree_notes.json` (XDG data dir, or
//! `CORRAL_DATA_DIR` — see `crate::paths`),
//! alongside `agent_session`'s `sessions.json` — this is app-generated
//! working state, not user config. Lock discipline is shared via
//! `crate::keyed_store`.

use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::keyed_store::KeyedStore;
use crate::paths::data_root;

fn store() -> Result<KeyedStore<String>> {
    Ok(KeyedStore::new(
        data_root()?,
        "worktree_notes.json",
        "worktree_notes",
        "notes",
    ))
}

/// The saved note for a worktree, or an empty string if none is set.
pub fn get(key: &str) -> Result<String> {
    Ok(store()?.get(key)?.unwrap_or_default())
}

/// Every worktree key with a non-empty saved note, for populating
/// `worktree::list()` in one read instead of a lookup per worktree.
pub fn all() -> Result<HashMap<String, String>> {
    store()?.all()
}

/// Save `note` for a worktree, or forget it entirely if `note` is empty
/// after trimming — keeps the store from accumulating empty-string entries
/// for every worktree ever touched.
pub fn set(key: &str, note: &str) -> Result<()> {
    set_in(&store()?, key, note)
}

/// [`set`] against an explicit store, so the trim-or-forget semantics are
/// testable without touching `~/.local/share/corral`.
fn set_in(store: &KeyedStore<String>, key: &str, note: &str) -> Result<()> {
    let trimmed = note.trim().to_string();
    if trimmed.is_empty() {
        store.remove(key)
    } else {
        store.set(key, trimmed)
    }
}

/// Drop every stored note whose key isn't in `live` (e.g. a worktree removed
/// out-of-band, bypassing [`crate::worktree::remove`], which would otherwise
/// have cleared its own entry). Returns the number of entries dropped.
pub fn retain_keys(live: &HashSet<String>) -> Result<usize> {
    store()?.retain_keys(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, KeyedStore<String>) {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyedStore::new(dir.path().to_path_buf(), "notes.json", "test", "notes");
        (dir, store)
    }

    #[test]
    fn set_in_trims_whitespace_before_saving() {
        let (_dir, store) = temp_store();
        set_in(&store, "proj/feat", "  padded  ").unwrap();
        assert_eq!(store.get("proj/feat").unwrap(), Some("padded".to_string()));
    }

    #[test]
    fn set_in_removes_the_key_for_an_empty_note() {
        let (_dir, store) = temp_store();
        store.set("proj/feat", "note".to_string()).unwrap();
        set_in(&store, "proj/feat", "   ").unwrap();
        assert_eq!(store.get("proj/feat").unwrap(), None);
    }
}
