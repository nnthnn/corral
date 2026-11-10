//! `corral awake` — run a command while holding the platform's sleep/idle
//! inhibitor.
//!
//! Agents frequently launch long-running background work (scans, test suites,
//! dev servers, watchers) that outlives the turn that started it. Corral's own
//! `systemd-inhibit` guard only follows its in-memory "working" state, so
//! work detached from the harness (`setsid`, `nohup`, ...) would otherwise let
//! the machine suspend. Wrapping the worker in this command keeps the
//! platform's inhibitor held for exactly the worker's lifetime:
//!
//! - Linux: `systemd-inhibit --what=sleep:idle ...`
//! - macOS: `caffeinate -i -s ...`
//! - Windows: `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`
//!   held by the wrapper thread while the child runs.
//!
//! On Unix the wrapper `exec`s the platform inhibitor directly, so signals and
//! the child's exit status pass through unchanged. On Windows it spawns the
//! child and waits, then clears the execution state.

use std::process::Command;

use anyhow::{Context, Result};

/// Run `command` while holding a sleep/idle inhibitor. `why` is passed to
/// Linux's `systemd-inhibit` (the other platforms have no equivalent label).
pub fn run(why: &str, command: &[String]) -> Result<()> {
    let (program, args) = command
        .split_first()
        .context("no command given (use `corral awake -- <command>`)")?;
    platform_run(why, program, args)
}

#[cfg(target_os = "linux")]
fn linux_inhibitor(why: &str, program: &str, args: &[String]) -> Command {
    let mut cmd = Command::new("systemd-inhibit");
    cmd.args(["--what=sleep:idle", "--who=corral-agent", "--mode=block"])
        .arg(format!("--why={why}"))
        .arg("--")
        .arg(program)
        .args(args);
    cmd
}

#[cfg(target_os = "linux")]
fn platform_run(why: &str, program: &str, args: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut cmd = linux_inhibitor(why, program, args);
    let err = cmd.exec();
    Err(anyhow::Error::new(err).context("exec systemd-inhibit"))
}

#[cfg(target_os = "macos")]
fn macos_inhibitor(program: &str, args: &[String]) -> Command {
    let mut cmd = Command::new("caffeinate");
    // `-i` prevents idle sleep; `-s` additionally blocks system sleep while
    // on AC power. Together they mirror Linux's `sleep:idle` inhibitor.
    cmd.args(["-i", "-s"]).arg(program).args(args);
    cmd
}

#[cfg(target_os = "macos")]
fn platform_run(_why: &str, program: &str, args: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut cmd = macos_inhibitor(program, args);
    let err = cmd.exec();
    Err(anyhow::Error::new(err).context("exec caffeinate"))
}

#[cfg(windows)]
fn platform_run(_why: &str, program: &str, args: &[String]) -> Result<()> {
    use windows_sys::Win32::System::Power::{
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };

    // The execution state is per-thread, so it is set on this thread and
    // stays in effect while `status()` blocks waiting for the child. It is
    // cleared automatically if this process exits, including on Ctrl+C.
    // `SetThreadExecutionState` returns the *previous* state, which is also 0
    // on a fresh thread, so there is no reliable success/failure check here;
    // just run the command regardless and clear the state afterwards.
    unsafe {
        SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
    }

    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("running {program}"));

    // Always reset, even if spawning the child failed.
    unsafe {
        SetThreadExecutionState(ES_CONTINUOUS);
    }

    let status = status?;
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => std::process::exit(code),
        // A Windows process can be terminated without an exit code (e.g. by
        // a debugger or job object); surface that as failure.
        None => std::process::exit(1),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform_run(_why: &str, _program: &str, _args: &[String]) -> Result<()> {
    anyhow::bail!("`corral awake` is not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_empty_command() {
        let err = run("test", &[]).unwrap_err();
        assert!(err.to_string().contains("no command given"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_wraps_with_systemd_inhibit() {
        let why = "map 9 scan";
        let program = "bash";
        let args = ["scripts/long-running-job.sh".to_string()];
        let cmd = linux_inhibitor(why, program, &args);

        let rendered: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(cmd.get_program().to_string_lossy(), "systemd-inhibit");
        assert!(rendered.contains(&"--what=sleep:idle".to_string()));
        assert!(rendered.contains(&"--why=map 9 scan".to_string()));
        assert!(rendered.windows(3).any(|window| {
            window
                == [
                    "--".to_string(),
                    "bash".to_string(),
                    "scripts/long-running-job.sh".to_string(),
                ]
        }));
    }
}
