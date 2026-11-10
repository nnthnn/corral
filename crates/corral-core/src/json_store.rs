//! Shared "JSON blob on disk" primitives used by `settings`, `project_settings`,
//! and `agent_session` — each persists a different `serde`-able type under
//! `~/.config/corral` or `~/.local/share/corral`, but all three want the same
//! read-or-default-on-missing, reset-to-default-on-corruption, and
//! write-pretty-with-parent-dir-creation behavior.

use std::fs::{self, File};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Result;

/// Monotonic per-process counter disambiguating `save_pretty`'s temp files —
/// a temp name keyed only on `pid` would collide when two threads of the same
/// process save the same store concurrently (e.g. two corral windows share a
/// process, or a future single-process multi-window layout), letting one
/// writer clobber the other's temp file mid-write before the rename.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Load and parse a JSON file, returning `T::default()` if the file doesn't
/// exist yet or fails to parse (a warning is logged in the parse-failure
/// case, tagged with `context` to identify which store it came from).
///
/// A *present but unparseable* file is snapshotted to `<name>.bak` before
/// defaults are handed back: load-or-default semantics invite the caller to
/// later write the defaults back over the file, and when the file was written
/// by a different (typically newer) corral build or hand-edited, that write
/// would silently destroy contents this build simply couldn't read.
pub fn load_or_default<T: DeserializeOwned + Default>(path: &Path, context: &str) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Ok(value),
            Err(e) => {
                tracing::warn!("{context}: failed to parse {path:?}, resetting to default: {e}");
                backup_file(path, context, "unparseable");
                Ok(T::default())
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

/// The sibling backup path `<name>.bak` for a store file (e.g.
/// `settings.json` → `settings.json.bak`).
fn backup_path(path: &Path) -> std::path::PathBuf {
    let mut file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    file_name.push_str(".bak");
    path.with_file_name(file_name)
}

/// Snapshot `path` to `<name>.bak` (if the file exists), copying rather than
/// moving so the original stays put for anything that may still read it.
/// Best-effort: a failed copy only logs, tagged with `why` to make the
/// failure readable.
pub(crate) fn backup_file(path: &Path, context: &str, why: &str) {
    let backup = backup_path(path);
    if let Err(e) = fs::copy(path, &backup) {
        tracing::warn!("{context}: failed to back up {path:?} ({why}) to {backup:?}: {e}");
    }
}

/// Preserve the on-disk state of `path` (copy to `<name>.bak`) before a
/// caller overwrites it with [`save_pretty`], so the write can never destroy
/// the file's previous contents — e.g. an externally edited or
/// version-skewed file that a different corral build wrote. No-op if `path`
/// doesn't exist yet. Used by `settings::save_under`, where overwrites are
/// meaningful and infrequent (unlike high-churn stores like `sessions.json`).
pub(crate) fn backup_existing(path: &Path, context: &str) {
    if path.exists() {
        backup_file(path, context, "about to be overwritten");
    }
}

/// Serialize `value` as pretty JSON and write it to `path`, creating the
/// parent directory if needed. Writes via a same-directory temp file plus a
/// rename, so a concurrent reader never observes a partially-written file.
pub fn save_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp_path = dir.join(format!(
        "{}.{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    fs::write(&tmp_path, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Hold an exclusive lock (on a sibling `<file_name>.lock` file in `dir`)
/// across `f`, which receives the store's full path. The lock is advisory
/// (`std::fs::File::lock`, `flock` under the hood on Unix) and only needs to
/// cover writers; readers always see either the old or the new file since
/// `save_pretty` writes via a rename, which is atomic on POSIX.
///
/// This is the lower-level primitive behind [`with_locked_store`]; use it
/// when the write is conditional (e.g. skipped when nothing changed) or when
/// the payload isn't a typed struct loaded through [`load_or_default`].
pub fn with_locked_path<R>(
    dir: &Path,
    file_name: &str,
    f: impl FnOnce(&Path) -> Result<R>,
) -> Result<R> {
    fs::create_dir_all(dir)?;
    let lock_file = File::create(dir.join(format!("{file_name}.lock")))?;
    lock_file.lock()?;
    // The lock (and thus the file descriptor) is released when `lock_file`
    // drops at function end.
    f(&dir.join(file_name))
}

/// Hold an exclusive lock across a read-modify-write of `<dir>/<file_name>`,
/// so two processes (the GUI and CLI, or multiple corral windows) calling
/// this concurrently for the same store can't race a plain read-modify-write
/// and silently drop one side's update. Loads the store, applies `f`, and
/// saves unconditionally — see [`with_locked_path`] for conditional writes.
pub fn with_locked_store<T, R>(
    dir: &Path,
    file_name: &str,
    context: &str,
    f: impl FnOnce(&mut T) -> R,
) -> Result<R>
where
    T: DeserializeOwned + Serialize + Default,
{
    with_locked_path(dir, file_name, |path| {
        let mut store: T = load_or_default(path, context)?;
        let result = f(&mut store);
        save_pretty(path, &store)?;
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Widget {
        name: String,
    }

    #[test]
    fn load_or_default_returns_default_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let w: Widget = load_or_default(&dir.path().join("missing.json"), "test").unwrap();
        assert_eq!(w, Widget::default());
    }

    #[test]
    fn load_or_default_resets_corrupt_file_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("widget.json");
        fs::write(&path, b"not json").unwrap();
        let w: Widget = load_or_default(&path, "test").unwrap();
        assert_eq!(w, Widget::default());
    }

    #[test]
    fn load_or_default_backs_up_an_unparseable_file() {
        // A present-but-unparseable file must be preserved to `<name>.bak`
        // before defaults are returned: load-or-default semantics invite a
        // later save to write the defaults back over it, and that write must
        // not destroy what was actually on disk.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("widget.json");
        fs::write(&path, b"not json").unwrap();
        let w: Widget = load_or_default(&path, "test").unwrap();
        assert_eq!(w, Widget::default());
        assert_eq!(
            fs::read_to_string(dir.path().join("widget.json.bak")).unwrap(),
            "not json"
        );
    }

    #[test]
    fn load_or_default_leaves_a_parseable_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("widget.json");
        fs::write(&path, r#"{"name": "gizmo"}"#).unwrap();
        let w: Widget = load_or_default(&path, "test").unwrap();
        assert_eq!(
            w,
            Widget {
                name: "gizmo".into()
            }
        );
        assert!(!dir.path().join("widget.json.bak").exists());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("widget.json");
        let w = Widget {
            name: "gizmo".into(),
        };
        save_pretty(&path, &w).unwrap();
        let loaded: Widget = load_or_default(&path, "test").unwrap();
        assert_eq!(loaded, w);
    }

    #[test]
    fn with_locked_path_hands_over_the_store_path_under_lock() {
        let dir = tempfile::tempdir().unwrap();
        with_locked_path(dir.path(), "widget.json", |path| {
            assert_eq!(path, dir.path().join("widget.json"));
            save_pretty(
                path,
                &Widget {
                    name: "gizmo".into(),
                },
            )
        })
        .unwrap();
        assert!(dir.path().join("widget.json.lock").exists());
        let loaded: Widget = load_or_default(&dir.path().join("widget.json"), "test").unwrap();
        assert_eq!(
            loaded,
            Widget {
                name: "gizmo".into()
            }
        );
    }
}
