//! System sleep/idle inhibitor, held only while at least one agent session
//! is actively working (running but not waiting on the user). Shells out to
//! the platform's own tool rather than adding a DBus/driver dependency —
//! `systemd-inhibit` on Linux, `caffeinate` on macOS — mirroring `git.rs`'s
//! "shell out to the real tool" convention. Both are long-lived child
//! processes, so the guard can hold either by keeping (and killing) a
//! [`Child`]. Windows uses `SetThreadExecutionState`, which is per-thread
//! rather than a child process, so it isn't wired up here yet.

use std::process::{Child, Command};
use std::sync::Mutex;

#[derive(Default)]
pub struct InhibitGuard {
    child: Mutex<Option<Child>>,
}

impl InhibitGuard {
    /// Reconcile the held inhibitor against the desired state. Idempotent —
    /// calling repeatedly with the same `active` value only acts on the
    /// first call that actually changes state.
    pub fn sync(&self, active: bool) {
        self.sync_with(active, spawn_inhibit);
    }

    /// Same as [`Self::sync`], but takes the spawn function as a parameter
    /// so tests can substitute a lightweight process instead of the real
    /// `systemd-inhibit` binary.
    fn sync_with(&self, active: bool, spawn: impl Fn() -> Option<Child>) {
        let mut child = self.child.lock().unwrap();
        match (active, child.is_some()) {
            (true, false) => {
                // Spawn failure is already logged inside `spawn`; leaving
                // `child` as `None` means the next `sync(true)` call retries.
                if let Some(c) = spawn() {
                    *child = Some(c);
                }
            }
            (false, true) => {
                if let Some(mut c) = child.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
            _ => {}
        }
    }
}

fn spawn_inhibit() -> Option<Child> {
    spawn_platform_inhibitor()
}

#[cfg(target_os = "linux")]
fn spawn_platform_inhibitor() -> Option<Child> {
    let mut cmd = Command::new("systemd-inhibit");
    cmd.args([
        "--what=sleep:idle",
        "--who=corral",
        "--why=Agent session working",
        "--mode=block",
        "sleep",
        "infinity",
    ]);
    // Clean exit already kills this via `InhibitGuard::sync(false)` (see
    // `RunEvent::Exit` in `lib.rs`), but a hard crash/SIGKILL/`pkill
    // corral-app` skips that entirely and orphans `sleep infinity` — it has
    // no other reason to ever exit, so it leaks forever and blocks sleep
    // until someone notices and kills it by hand. `PR_SET_PDEATHSIG` makes
    // the kernel deliver SIGKILL to this child the instant *this* process
    // (parent at prctl-time) dies, by any means, closing that gap. Linux-only
    // syscall, but `systemd-inhibit` itself is Linux-only too.
    use std::os::unix::process::CommandExt;
    let parent_pid = std::process::id() as libc::pid_t;
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Close the race where the parent exits between fork and
            // this prctl call ever registering — in that window no
            // death signal would ever be sent, so check explicitly
            // against the parent pid captured before forking and
            // self-kill if we're already orphaned.
            if libc::getppid() != parent_pid {
                libc::raise(libc::SIGKILL);
            }
            Ok(())
        });
    }
    spawn_logged(cmd)
}

#[cfg(target_os = "macos")]
fn spawn_platform_inhibitor() -> Option<Child> {
    // Holding the process is the inhibitor: `-i` prevents idle sleep and `-s`
    // also blocks system sleep while on AC power, together mirroring Linux's
    // `sleep:idle`. `InhibitGuard::sync(false)` kills it on a normal stop, and
    // `-w <our pid>` covers the hard-crash case (macOS has no
    // `PR_SET_PDEATHSIG`): caffeinate exits on its own the instant corral-app
    // dies, instead of orphaning itself and blocking sleep forever.
    let mut cmd = Command::new("caffeinate");
    cmd.args(["-i", "-s"])
        .arg("-w")
        .arg(std::process::id().to_string());
    spawn_logged(cmd)
}

/// Windows would need `SetThreadExecutionState(ES_CONTINUOUS |
/// ES_SYSTEM_REQUIRED)` held by a dedicated thread rather than a child
/// process, so it can't share this `Child`-based guard. Not wired up yet.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn spawn_platform_inhibitor() -> Option<Child> {
    tracing::debug!("caffeinate: no sleep inhibitor implemented on this platform");
    None
}

#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
fn spawn_logged(mut cmd: Command) -> Option<Child> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    match cmd.spawn() {
        Ok(child) => Some(child),
        Err(e) => {
            tracing::warn!("caffeinate: failed to spawn {program}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn fake_spawn(count: &Cell<u32>) -> Option<Child> {
        count.set(count.get() + 1);
        Command::new("sleep").arg("30").spawn().ok()
    }

    #[test]
    fn spawns_only_on_inactive_to_active_transition() {
        let guard = InhibitGuard::default();
        let count = Cell::new(0);
        guard.sync_with(true, || fake_spawn(&count));
        guard.sync_with(true, || fake_spawn(&count));
        guard.sync_with(true, || fake_spawn(&count));
        assert_eq!(count.get(), 1);
        guard.sync_with(false, || fake_spawn(&count));
        assert_eq!(count.get(), 1);
        assert!(guard.child.lock().unwrap().is_none());
    }

    #[test]
    fn kills_only_on_active_to_inactive_transition() {
        let guard = InhibitGuard::default();
        let count = Cell::new(0);
        guard.sync_with(true, || fake_spawn(&count));
        assert!(guard.child.lock().unwrap().is_some());
        guard.sync_with(false, || fake_spawn(&count));
        assert!(guard.child.lock().unwrap().is_none());
        guard.sync_with(false, || fake_spawn(&count));
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn failed_spawn_leaves_slot_empty_for_retry() {
        let guard = InhibitGuard::default();
        let attempts = Cell::new(0);
        let never_spawns = || {
            attempts.set(attempts.get() + 1);
            None
        };
        guard.sync_with(true, never_spawns);
        guard.sync_with(true, never_spawns);
        assert_eq!(attempts.get(), 2);
        assert!(guard.child.lock().unwrap().is_none());
    }
}
