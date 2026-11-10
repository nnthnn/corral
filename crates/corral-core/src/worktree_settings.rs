//! Persisted per-worktree settings (`<project>/<feature>`) beyond the
//! git-visible state — currently just a human-readable `title`, e.g. "Fix
//! Login Timeout", shown in the UI in place of the kebab-case `slug` used
//! for the branch and symlink alias.
//!
//! Stored at `~/.local/share/corral/worktree_settings.json` (XDG data dir, or
//! `CORRAL_DATA_DIR` — see `crate::paths`),
//! alongside `worktree_notes.json` and `sessions.json` — this is
//! app-generated working state, not user config. Uses a struct-per-key
//! shape (rather than a flat `HashMap<String, String>` like
//! `worktree_notes`) so a future field (e.g. a lifecycle/workflow status)
//! can be added without a schema rewrite. Lock discipline is shared via
//! `crate::keyed_store`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::keyed_store::KeyedStore;
use crate::paths::data_root;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// Human-readable name, e.g. "Fix Login Timeout". Empty means unset.
    #[serde(default)]
    title: String,
}

impl Entry {
    fn is_empty(&self) -> bool {
        self.title.is_empty()
    }
}

fn store() -> Result<KeyedStore<Entry>> {
    Ok(KeyedStore::new(
        data_root()?,
        "worktree_settings.json",
        "worktree_settings",
        "entries",
    ))
}

/// The saved title for a worktree, or an empty string if none is set.
pub fn get_title(key: &str) -> Result<String> {
    Ok(store()?.get(key)?.map(|e| e.title).unwrap_or_default())
}

/// Every worktree key with a non-empty saved title, for populating
/// `worktree::list()` in one read instead of a lookup per worktree.
pub fn all_titles() -> Result<HashMap<String, String>> {
    Ok(store()?
        .all()?
        .into_iter()
        .filter(|(_, e)| !e.is_empty())
        .map(|(k, e)| (k, e.title))
        .collect())
}

/// Save `title` for a worktree, or forget its entry entirely if `title` is
/// empty after trimming — keeps the store from accumulating empty entries
/// for every worktree ever renamed.
pub fn set_title(key: &str, title: &str) -> Result<()> {
    set_title_in(&store()?, key, title)
}

/// [`set_title`] against an explicit store, so the trim-or-forget semantics
/// are testable without touching `~/.local/share/corral`.
fn set_title_in(store: &KeyedStore<Entry>, key: &str, title: &str) -> Result<()> {
    let trimmed = title.trim().to_string();
    if trimmed.is_empty() {
        store.remove(key)
    } else {
        store.set(key, Entry { title: trimmed })
    }
}

/// Drop any stored entry whose key isn't in `live` (e.g. a worktree removed
/// out-of-band, bypassing [`crate::worktree::remove`], which would
/// otherwise have cleared its own entry). Returns the number of entries
/// dropped.
pub fn retain_keys(live: &HashSet<String>) -> Result<usize> {
    store()?.retain_keys(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_title_in_trims_and_removes_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyedStore::new(dir.path().to_path_buf(), "settings.json", "test", "entries");

        set_title_in(&store, "proj/feat", "  Fix Login Timeout  ").unwrap();
        assert_eq!(
            store.get("proj/feat").unwrap().map(|e| e.title),
            Some("Fix Login Timeout".to_string())
        );

        set_title_in(&store, "proj/feat", "   ").unwrap();
        assert_eq!(store.get("proj/feat").unwrap(), None);
    }

    #[test]
    fn all_titles_excludes_empty_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyedStore::new(dir.path().to_path_buf(), "settings.json", "test", "entries");
        store
            .set(
                "proj/alive",
                Entry {
                    title: "Fix Login Timeout".into(),
                },
            )
            .unwrap();
        store
            .set(
                "proj/blank",
                Entry {
                    title: String::new(),
                },
            )
            .unwrap();

        let titles: HashMap<String, String> = store
            .all()
            .unwrap()
            .into_iter()
            .filter(|(_, e)| !e.is_empty())
            .map(|(k, e)| (k, e.title))
            .collect();
        assert_eq!(titles.len(), 1);
        assert_eq!(
            titles.get("proj/alive").map(String::as_str),
            Some("Fix Login Timeout")
        );
    }
}
