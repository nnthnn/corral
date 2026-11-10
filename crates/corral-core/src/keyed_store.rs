//! Generic "keyed JSON store": a `HashMap<String, V>` persisted to disk under
//! `crate::paths::data_root` (`~/.local/share/corral` by default), backing the
//! per-worktree key/value modules (`agent_session`, `worktree_notes`,
//! `worktree_settings`).
//!
//! Those three modules used to each carry a copy of the same locked-HashMap
//! plumbing (a private store struct, `load_from`, `with_locked_store_in`, and
//! the 20-thread concurrency test). This centralizes it so the lock discipline
//! is uniform and the read-modify-write behavior lives in exactly one place
//! (see [`json_store::with_locked_store`]).
//!
//! The on-disk shape is preserved from the pre-refactor wrapper structs:
//! the map is stored under a per-store JSON key (`{"sessions": {...}}`,
//! `{"notes": {...}}`, `{"entries": {...}}`), so files written by older
//! corral builds keep loading.

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Result;
use crate::json_store;

/// A `HashMap<String, V>` persisted to `<dir>/<file>` under the JSON object
/// key `field` (the wrapper key each module historically used).
///
/// `V` is the per-key payload. Callers wrap this in their own public API —
/// which owns key formatting, trimming, and "empty value means delete"
/// semantics — and expose testable `*_in` helpers that take the store as a
/// parameter, matching the crate's `*_under`/`*_in` convention.
pub struct KeyedStore<V> {
    dir: PathBuf,
    file: &'static str,
    context: &'static str,
    field: &'static str,
    _value: PhantomData<V>,
}

impl<V> KeyedStore<V>
where
    V: Serialize + DeserializeOwned,
{
    pub fn new(
        dir: PathBuf,
        file: &'static str,
        context: &'static str,
        field: &'static str,
    ) -> Self {
        KeyedStore {
            dir,
            file,
            context,
            field,
            _value: PhantomData,
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(self.file)
    }

    fn load_from(&self, path: &Path) -> Result<HashMap<String, V>> {
        let raw: serde_json::Value = json_store::load_or_default(path, self.context)?;
        let map = raw
            .get(self.field)
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        match serde_json::from_value::<HashMap<String, V>>(map) {
            Ok(entries) => Ok(entries),
            Err(e) => {
                // Valid JSON, but not this store's shape (e.g. hand-edited or
                // written by a different build). `load_or_default` only resets
                // *unparseable* files; a shape mismatch would otherwise error
                // forever — and break writes too, since `set`/`remove` load
                // before saving. Snapshot to `.bak` and reset to empty, the
                // same recovery `load_or_default` gives unparseable files.
                tracing::warn!(
                    "{}: {} at {:?} has the wrong shape, resetting to empty: {e}",
                    self.context,
                    self.field,
                    path
                );
                json_store::backup_file(path, self.context, "wrong shape");
                Ok(HashMap::new())
            }
        }
    }

    fn save_to(&self, path: &Path, map: &HashMap<String, V>) -> Result<()> {
        let mut wrapper = serde_json::Map::new();
        wrapper.insert(self.field.to_string(), serde_json::to_value(map)?);
        json_store::save_pretty(path, &wrapper)
    }

    fn load(&self) -> Result<HashMap<String, V>> {
        self.load_from(&self.path())
    }

    fn with_locked<R>(&self, f: impl FnOnce(&mut HashMap<String, V>) -> R) -> Result<R> {
        json_store::with_locked_path(&self.dir, self.file, |path| {
            let mut map = self.load_from(path)?;
            let result = f(&mut map);
            self.save_to(path, &map)?;
            Ok(result)
        })
    }

    /// The saved value for `key`, or `None`.
    pub fn get(&self, key: &str) -> Result<Option<V>> {
        Ok(self.load()?.remove(key))
    }

    /// Whether `key` has a saved value.
    pub fn has(&self, key: &str) -> Result<bool> {
        Ok(self.load()?.contains_key(key))
    }

    /// Every saved key/value pair.
    pub fn all(&self) -> Result<HashMap<String, V>> {
        self.load()
    }

    /// Save `value` under `key`, overwriting any existing value.
    pub fn set(&self, key: &str, value: V) -> Result<()> {
        self.with_locked(|store| {
            store.insert(key.to_string(), value);
        })
    }

    /// Forget any saved value for `key`.
    pub fn remove(&self, key: &str) -> Result<()> {
        self.with_locked(|store| {
            store.remove(key);
        })
    }

    /// Drop every entry whose key isn't in `live`. Returns the number dropped.
    pub fn retain_keys(&self, live: &HashSet<String>) -> Result<usize> {
        self.with_locked(|store| {
            let before = store.len();
            store.retain(|key, _| live.contains(key));
            before - store.len()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Payload {
        title: String,
    }

    fn store(dir: &std::path::Path) -> KeyedStore<Payload> {
        KeyedStore::new(dir.to_path_buf(), "store.json", "test", "entries")
    }

    #[test]
    fn set_then_get_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .set(
                "proj/feat",
                Payload {
                    title: "Fix".into(),
                },
            )
            .unwrap();
        assert_eq!(
            store.get("proj/feat").unwrap(),
            Some(Payload {
                title: "Fix".into()
            })
        );
    }

    #[test]
    fn persists_under_the_wrapper_field() {
        // Regression: the on-disk document must keep the wrapper key (e.g.
        // `{"entries": {...}}`), matching what the pre-refactor per-module
        // structs wrote — otherwise older files silently stop loading.
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .set(
                "proj/feat",
                Payload {
                    title: "Fix".into(),
                },
            )
            .unwrap();

        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("store.json")).unwrap()).unwrap();
        assert_eq!(
            raw.get("entries").unwrap().get("proj/feat").unwrap()["title"],
            "Fix"
        );
        assert!(raw.get("proj/feat").is_none(), "map must be wrapped");
    }

    #[test]
    fn loads_the_legacy_wrapper_shape() {
        // A file written by a pre-refactor build loads unchanged.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("store.json"),
            r#"{"entries":{"proj/feat":{"title":"Fix"}}}"#,
        )
        .unwrap();
        let store = store(dir.path());
        assert_eq!(
            store.get("proj/feat").unwrap(),
            Some(Payload {
                title: "Fix".into()
            })
        );
    }

    #[test]
    fn get_on_a_missing_key_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert_eq!(store.get("nope").unwrap(), None);
    }

    #[test]
    fn has_reflects_presence() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert!(!store.has("k").unwrap());
        store.set("k", Payload { title: "x".into() }).unwrap();
        assert!(store.has("k").unwrap());
    }

    #[test]
    fn remove_forgets_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store.set("k", Payload { title: "x".into() }).unwrap();
        store.remove("k").unwrap();
        assert!(!store.has("k").unwrap());
    }

    #[test]
    fn retain_keys_drops_entries_not_in_the_live_set() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .set("proj/alive", Payload { title: "a".into() })
            .unwrap();
        store
            .set("proj/orphaned", Payload { title: "b".into() })
            .unwrap();

        let live: HashSet<String> = ["proj/alive".to_string()].into_iter().collect();
        let dropped = store.retain_keys(&live).unwrap();

        assert_eq!(dropped, 1);
        assert!(store.has("proj/alive").unwrap());
        assert!(!store.has("proj/orphaned").unwrap());
    }

    #[test]
    fn a_missing_file_loads_empty_and_a_corrupt_one_resets_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert!(store.all().unwrap().is_empty());

        std::fs::write(dir.path().join("store.json"), b"not json").unwrap();
        assert!(store.all().unwrap().is_empty());
    }

    #[test]
    fn a_wrong_shape_file_loads_empty_and_is_backed_up() {
        // Valid JSON, but not a keyed map of `Payload` — `{"entries": {"title":
        // ""}}` puts a string where a `Payload` object belongs. This must reset
        // to empty (not error) so a later write can cleanly overwrite the file,
        // and the original must survive to `.bak`.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.json");
        let corrupt = r#"{"entries":{"title":""}}"#;
        std::fs::write(&path, corrupt).unwrap();

        let store = store(dir.path());
        assert!(store.all().unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("store.json.bak")).unwrap(),
            corrupt
        );

        // A write after recovery must produce a well-formed file.
        store
            .set(
                "proj/feat",
                Payload {
                    title: "Fix".into(),
                },
            )
            .unwrap();
        assert_eq!(
            store.get("proj/feat").unwrap(),
            Some(Payload {
                title: "Fix".into()
            })
        );
    }

    /// The whole point of routing through `json_store::with_locked_path`:
    /// without the lock, two threads racing a plain read-modify-write would
    /// each load the same starting state and one's insert would clobber the
    /// other's on save.
    #[test]
    fn concurrent_writers_do_not_drop_each_others_updates() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        let handles: Vec<_> = (0..20)
            .map(|i| {
                let dir_path = dir_path.clone();
                std::thread::spawn(move || {
                    let store = KeyedStore::new(dir_path, "store.json", "test", "entries");
                    store
                        .set(
                            &format!("proj/feat-{i}"),
                            Payload {
                                title: format!("title-{i}"),
                            },
                        )
                        .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        let store: KeyedStore<Payload> = KeyedStore::new(dir_path, "store.json", "test", "entries");
        assert_eq!(
            store.all().unwrap().len(),
            20,
            "expected all 20 concurrent writes to survive"
        );
    }
}
