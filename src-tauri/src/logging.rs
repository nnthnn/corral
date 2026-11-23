//! Tracing setup for the desktop app.
//!
//! Logs go to **stderr** (the dev terminal) *and* to a rolling file under the
//! corral data dir (`~/.local/share/corral/logs/corral.log`, honouring
//! `CORRAL_DATA_DIR`). The file exists because a bundled Linux/macOS launch
//! — `.desktop` entry, Dock, `open` — has no attached terminal, so every
//! `tracing::warn!` on a failure path (a session that won't spawn, a watcher
//! that won't start, a config that won't parse) was written into a stderr
//! nobody could read. `corral doctor` points at this file, and the Settings
//! panel's "Copy diagnostics" includes its tail, so a user can paste what
//! actually went wrong instead of "it doesn't work".

use std::path::PathBuf;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// Directory holding the rolling log files, under [`corral_core::paths::data_root`].
pub fn logs_dir() -> Option<PathBuf> {
    corral_core::paths::data_root()
        .ok()
        .map(|root| root.join("logs"))
}

/// Frames the non-blocking writer owns. Generous: this is one GUI process, and
/// the writer thread drops (rather than blocks) when it falls behind, so an
/// oversized buffer costs a little memory instead of stalling the UI.
const BUFFER_LINES: usize = 8192;

/// Install the global subscriber. Must be called before any thread that logs
/// is spawned (same ordering constraint `shell_env` relies on), and only once.
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("corral_app_lib=debug,corral_core=debug"));
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true);

    match logs_dir() {
        Some(dir) => {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                // Nothing to do but keep logging to stderr.
                tracing_subscriber::registry()
                    .with(filter)
                    .with(stderr_layer)
                    .init();
                tracing::warn!("could not create the log directory {dir:?}: {e}");
                return;
            }
            // Daily rotation, 5 files kept (~generous: a GUI run is a few
            // hundred KB of `debug` lines, and rotation bounds it either way).
            let appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("corral")
                .filename_suffix("log")
                .max_log_files(5)
                .build(&dir);
            match appender {
                Ok(file) => {
                    let (file, guard) =
                        tracing_appender::non_blocking::NonBlockingBuilder::default()
                            .buffered_lines_limit(BUFFER_LINES)
                            .finish(file);
                    // Leak the guard deliberately: it flushes the writer on
                    // drop, and corral's process ends by way of the webview
                    // closing, where running destructors is not guaranteed.
                    // Leaking it keeps the writer alive for the whole
                    // process lifetime, which is what we want.
                    std::mem::forget(guard);
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(stderr_layer)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .with_writer(file)
                                .with_ansi(false)
                                .with_target(true),
                        )
                        .init();
                }
                Err(e) => {
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(stderr_layer)
                        .init();
                    tracing::warn!("could not open a log file in {dir:?}: {e}");
                }
            }
        }
        None => {
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .init();
            tracing::warn!("no corral data dir; logs go to stderr only");
        }
    }
}

/// The most recently modified log file in the logs dir.
pub fn current_log_file() -> Option<PathBuf> {
    let dir = logs_dir()?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, e.path()))
        })
        .filter(|(_, path)| path.extension().is_some_and(|ext| ext == "log"))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// The last `max_lines` lines of the current log file, oldest first. `None`
/// when no log file exists yet.
pub fn tail(max_lines: usize) -> Option<String> {
    let path = current_log_file()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let total = text.lines().count();
    Some(
        text.lines()
            .skip(total.saturating_sub(max_lines))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_dir_sits_under_the_data_root() {
        // The real data root depends on the environment, so only the shape is
        // asserted: if there is a data root, logs live in `<it>/logs`.
        if let Ok(root) = corral_core::paths::data_root() {
            assert_eq!(logs_dir(), Some(root.join("logs")));
        }
    }

    #[test]
    fn tail_reads_the_last_lines_of_the_current_log() {
        // Exercises `tail` against a synthetic dir by pointing the data root
        // env var at a temp dir — the same override `just run-dev` uses.
        // Serialized: the env var is process-global.
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("corral.2026-01-01.log"), "a\nb\nc\nd\n").unwrap();

        // SAFETY: test-only, serialized by LOCK, and this module's tests are
        // the only ones in the crate that read the data root concurrently.
        unsafe { std::env::set_var(corral_core::paths::DATA_DIR_ENV, dir.path()) };
        let tail = tail(2);
        unsafe { std::env::remove_var(corral_core::paths::DATA_DIR_ENV) };

        assert_eq!(tail.as_deref(), Some("c\nd"));
    }
}
