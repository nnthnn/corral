//! Resolving executable paths for the agent CLIs corral spawns (`claude`,
//! `opencode`, …). Kept separate from `settings.rs` so both the Tauri app and
//! the CLI can share the "is this binary usable?" check and fail with a
//! friendly message before a raw spawn error surfaces.

use std::path::{Path, PathBuf};

/// Resolve `bin` (a command name like `opencode` or an explicit path like
/// `/opt/opencode/bin/opencode`) to the executable corral would actually
/// spawn, or `None` if it can't be found.
///
/// A bare name is searched against each directory in `$PATH`; a path is
/// checked as-is. In both cases the candidate must exist and be executable.
pub fn resolve_executable(bin: &str) -> Option<PathBuf> {
    let trimmed = bin.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_path(trimmed) {
        return exists_and_executable(Path::new(trimmed)).then(|| PathBuf::from(trimmed));
    }
    let path_var = std::env::var_os("PATH")?.to_string_lossy().into_owned();
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(trimmed);
        if exists_and_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// A string that looks like a path rather than a bare command name — anything
/// with a separator, or a leading `.`/`/`/`~`. `Command` treats these the
/// same way (a `PATH` search only happens for names without separators).
fn is_path(bin: &str) -> bool {
    bin.contains(std::path::MAIN_SEPARATOR)
        || bin.starts_with('.')
        || bin.starts_with('/')
        || bin.starts_with('~')
}

fn exists_and_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// A human-readable error for "the agent binary can't be found", naming what
/// corral looked for and how to fix it. Surfaced as-is by both the Tauri app
/// (a toast on a failed session start) and the CLI.
pub fn missing_executable_error(harness: crate::harness::Harness, bin: &str) -> String {
    format!(
        "couldn't find the {} executable (looked for \"{bin}\"). {}",
        harness.display_name(),
        harness.install_hint()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests below mutate `$PATH`, which is process-global — serialize
    /// them so two parallel tests can't see each other's half-set value.
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A temp dir with one executable (`run-me`) and one non-executable
    /// (`lazy`), used to seed a synthetic `$PATH` for the tests below.
    fn bin_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let exec = dir.path().join("run-me");
        let lazy = dir.path().join("lazy");
        std::fs::write(&exec, "#!/bin/sh\n").unwrap();
        std::fs::write(&lazy, "nothing\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&exec).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&exec, perms).unwrap();
        (dir, exec)
    }

    #[test]
    fn resolves_a_bare_name_from_path() {
        let _guard = PATH_LOCK.lock().unwrap();
        let (dir, exec) = bin_dir();
        let path_var = format!(
            "{}:{}",
            dir.path().display(),
            std::env::var("PATH").unwrap()
        );
        // SAFETY: `set_var` is process-global and only sound when no other
        // thread is reading the environment concurrently. PATH_LOCK serializes
        // these tests, so no other test thread is touching `$PATH`.
        unsafe { std::env::set_var("PATH", path_var) };
        assert_eq!(resolve_executable("run-me"), Some(exec));
        dir.close().unwrap();
    }

    #[test]
    fn skips_non_executable_files_in_path() {
        let _guard = PATH_LOCK.lock().unwrap();
        let (dir, _) = bin_dir();
        let path_var = format!(
            "{}:{}",
            dir.path().display(),
            std::env::var("PATH").unwrap()
        );
        // SAFETY: `set_var` is process-global and only sound when no other
        // thread is reading the environment concurrently. PATH_LOCK serializes
        // these tests, so no other test thread is touching `$PATH`.
        unsafe { std::env::set_var("PATH", path_var) };
        assert_eq!(resolve_executable("lazy"), None);
        dir.close().unwrap();
    }

    #[test]
    fn resolves_an_absolute_path() {
        let (dir, exec) = bin_dir();
        assert_eq!(resolve_executable(exec.to_str().unwrap()), Some(exec));
        dir.close().unwrap();
    }

    #[test]
    fn returns_none_for_a_missing_binary() {
        assert_eq!(
            resolve_executable("definitely-not-a-real-corral-agent"),
            None
        );
    }

    #[test]
    fn returns_none_for_a_missing_absolute_path() {
        assert_eq!(resolve_executable("/no/such/binary"), None);
    }

    #[test]
    fn returns_none_for_empty_or_whitespace() {
        assert_eq!(resolve_executable(""), None);
        assert_eq!(resolve_executable("   "), None);
    }
}
