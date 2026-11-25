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

/// Name of the CLI binary corral bundles next to the desktop app. It's the
/// `[[bin]]` name from `crates/corral-cli`, so the same name works for a dev
/// build (`target/debug/corral`), a Linux package (`/usr/bin/corral`), and a
/// macOS bundle (`Corral.app/Contents/MacOS/corral`).
const CLI_BINARY_NAME: &str = "corral";

/// Resolve the `corral` CLI that belongs to this app install, preferring the
/// binary bundled alongside the running app over anything already on `$PATH`.
///
/// The app and CLI ship together — as a Tauri `externalBin` sidecar on macOS
/// (`Contents/MacOS/corral`) and under `/usr/bin` in the Linux packages — so
/// `current_exe`'s parent is the authoritative location there. The `$PATH`
/// fallback keeps `cargo build -p corral-app` without `-p corral-cli` working,
/// and picks up an independently installed CLI in dev.
///
/// A sibling wins whenever one exists — the bundled sidecar, and for a dev
/// build the `corral` that `just build` / `just build-release` compile into the
/// same profile dir. When there is no sibling (a `cargo build -p corral-app`
/// on its own), `$PATH` decides. Because the result is baked into every
/// project's hooks as an absolute path, preferring the sibling keeps a dev
/// instance on *its own* CLI instead of an unrelated `corral` earlier on
/// `$PATH` (say, a symlink into another checkout).
pub fn corral_cli_path() -> Option<PathBuf> {
    match std::env::current_exe() {
        Ok(exe) => cli_for_app(exe.as_path()),
        Err(_) => resolve_executable(CLI_BINARY_NAME),
    }
}

/// [`corral_cli_path`] for a known app executable, split out so the
/// sibling-vs-`$PATH` choice can be tested without moving the test binary.
fn cli_for_app(app: &Path) -> Option<PathBuf> {
    if let Some(dir) = app.parent()
        && let Some(path) = sibling_executable(dir, CLI_BINARY_NAME)
    {
        return Some(path);
    }
    resolve_executable(CLI_BINARY_NAME)
}

/// Path to bake into Claude Code's hook commands, or `None` to keep the bare
/// `corral` name.
///
/// An AppImage is mounted at a fresh `/tmp/.mount_*` path on every launch, so
/// an absolute path written into a project's `.claude/settings.local.json`
/// would go stale immediately. In that case the bare name is better — the
/// session PTY gets the mounted CLI directory prepended to `PATH` (see
/// `commands::session`), and only corral-spawned sessions can report hook
/// status anyway.
pub fn hook_cli_path() -> Option<PathBuf> {
    // `APPIMAGE` is set by the AppImage runtime to the archive path. Treat an
    // empty value as "not an AppImage" too — some launchers export the name
    // with no value, and a bogus flag would silently downgrade hooks to the
    // bare `corral` name.
    let appimage = std::env::var_os("APPIMAGE").is_some_and(|value| !value.is_empty());
    hook_cli_given(appimage, corral_cli_path())
}

/// The AppImage decision split out from the environment so it can be tested.
fn hook_cli_given(appimage: bool, cli: Option<PathBuf>) -> Option<PathBuf> {
    if appimage { None } else { cli }
}

/// The `name` binary in `dir`, if it exists and is executable.
fn sibling_executable(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(name);
    exists_and_executable(&candidate).then_some(candidate)
}

/// Put `dir` at the front of a `PATH`-style string, keeping `base` intact.
///
/// Used to make the bundled CLI callable from an agent's shell without losing
/// a project-configured `PATH` (nvm, toolchains, …). A no-op when `dir` is
/// already the first entry, and an empty `base` yields just `dir` (rather than
/// a trailing `:` that would put the current directory on `PATH`).
pub fn prepend_to_path(dir: &Path, base: &str) -> String {
    let dir_str = dir.to_string_lossy();
    if base.is_empty() || base.split(':').next() == Some(dir_str.as_ref()) {
        return if base.is_empty() {
            dir_str.into_owned()
        } else {
            base.to_string()
        };
    }
    format!("{dir_str}:{base}")
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

    #[test]
    fn sibling_executable_finds_an_executable_in_the_same_directory() {
        let (dir, exec) = bin_dir();
        assert_eq!(sibling_executable(dir.path(), "run-me"), Some(exec));
        assert_eq!(sibling_executable(dir.path(), "lazy"), None);
        assert_eq!(sibling_executable(dir.path(), "missing"), None);
        dir.close().unwrap();
    }

    #[test]
    fn hook_cli_path_keeps_absolute_paths_except_in_an_appimage() {
        let cli = PathBuf::from("/Applications/Corral.app/Contents/MacOS/corral");
        assert_eq!(
            hook_cli_given(false, Some(cli.clone())),
            Some(cli.clone()),
            "normal installs should bake the absolute path"
        );
        assert_eq!(
            hook_cli_given(true, Some(cli)),
            None,
            "an AppImage mount path changes every launch, so keep the bare name"
        );
    }

    /// Write an executable `name` into `dir` and return its path.
    fn make_executable(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn cli_for_app_prefers_a_sibling_over_path() {
        let _guard = PATH_LOCK.lock().unwrap();
        // The app's own directory holds a `corral` (the `just build` layout);
        // a different `corral` comes first on `$PATH`. The sibling must win,
        // so a dev instance stays on its own CLI.
        let app_dir = tempfile::tempdir().unwrap();
        let sibling = make_executable(app_dir.path(), CLI_BINARY_NAME);
        let app = make_executable(app_dir.path(), "corral-app");

        let path_dir = tempfile::tempdir().unwrap();
        make_executable(path_dir.path(), CLI_BINARY_NAME);
        // SAFETY: process-global, serialized by PATH_LOCK (see above).
        unsafe {
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    path_dir.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
        };

        assert_eq!(cli_for_app(&app), Some(sibling));
    }

    #[test]
    fn cli_for_app_falls_back_to_path_without_a_sibling() {
        let _guard = PATH_LOCK.lock().unwrap();
        let app_dir = tempfile::tempdir().unwrap();
        let app = make_executable(app_dir.path(), "corral-app");

        let path_dir = tempfile::tempdir().unwrap();
        let on_path = make_executable(path_dir.path(), CLI_BINARY_NAME);
        // SAFETY: process-global, serialized by PATH_LOCK (see above).
        unsafe {
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    path_dir.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
        };

        assert_eq!(cli_for_app(&app), Some(on_path));
    }

    #[test]
    fn prepend_to_path_puts_the_dir_first_and_keeps_the_base() {
        let dir = Path::new("/Applications/Corral.app/Contents/MacOS");
        assert_eq!(
            prepend_to_path(dir, "/usr/bin:/bin"),
            "/Applications/Corral.app/Contents/MacOS:/usr/bin:/bin"
        );
    }

    #[test]
    fn prepend_to_path_handles_empty_base_and_existing_first_entry() {
        let dir = Path::new("/opt/corral/bin");
        assert_eq!(prepend_to_path(dir, ""), "/opt/corral/bin");
        assert_eq!(
            prepend_to_path(dir, "/opt/corral/bin:/usr/bin"),
            "/opt/corral/bin:/usr/bin",
            "already-first should stay unchanged"
        );
    }
}
