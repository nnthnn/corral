//! Background thread that auto-stops sessions which have been continuously
//! "waiting" longer than the configured idle timeout
//! (`Settings.idle_timeout_enabled`/`idle_timeout_minutes`) — a cost/resource
//! guard for unattended parallel agents. Mirrors `hook_listener::spawn`'s
//! shape: one thread spawned once at app startup, running for the process's
//! lifetime.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::commands::session::SessionRegistry;

/// How often to check for idle sessions. Coarse on purpose — this only
/// needs to catch a session that's been idle for minutes, not seconds.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(SWEEP_INTERVAL);

            let settings = corral_core::settings::load().unwrap_or_default();
            if !settings.idle_timeout_enabled {
                continue;
            }
            // Clamp to at least one minute: the UI enforces a minimum, but a
            // hand-edited/older settings.json can carry `idle_timeout_minutes:
            // 0`, which would make the timeout zero and stop every waiting
            // session on the very next sweep.
            let timeout = Duration::from_secs(u64::from(settings.idle_timeout_minutes.max(1)) * 60);

            let registry = app.state::<SessionRegistry>();
            let stopped = registry.sweep_idle(timeout);
            if !stopped.is_empty() {
                tracing::info!(
                    "idle_timeout: auto-stopped {} idle session(s): {stopped:?}",
                    stopped.len()
                );
            }
        }
    });
}
