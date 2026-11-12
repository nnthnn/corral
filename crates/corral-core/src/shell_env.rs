//! The login shell's environment, for GUI launches.
//!
//! An app started from Finder/Dock/`open` on macOS inherits launchd's minimal
//! environment, not the user's shell environment: `PATH` is just
//! `/usr/bin:/bin:/usr/sbin:/sbin` (no Homebrew, `~/.local/bin`, pnpm, nvm,
//! volta…) and anything exported from `~/.zprofile`/`~/.zshrc`
//! (`ANTHROPIC_API_KEY`, `GITHUB_TOKEN`, `PNPM_HOME`, …) is absent. Every
//! child corral spawns — agent PTYs, Run/Setup commands, `git`, `gh` —
//! inherits corral's environment, so without this the app behaves like a
//! shell with an empty profile.
//!
//! The fix is the one terminal emulators and editors (VS Code, Zed) use: run
//! the user's shell once, dump its environment, and adopt the result into
//! corral's own process environment before anything is spawned. Interactive
//! (`-i`) matters: `~/.zshrc` is where PATH additions usually live, and a
//! login-only shell (`-l`) never sources it.
//!
//! Adoption is a merge, not a replace: variables already present in corral's
//! environment win (so `CORRAL_CONFIG_DIR` from `just run-dev`, or a
//! `RUST_LOG` set on a terminal launch, survive), except `PATH`, which takes
//! the shell's value with any app-only entries appended. That keeps a
//! terminal-launched corral working exactly as before while giving a
//! Finder-launched one the environment its children expect.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Markers around the dumped environment, so rc-file banners/prompts printed
/// by an interactive shell (oh-my-zsh, p10k, …) can be discarded. A fixed
/// marker pair is fine: the child shell is the user's own, not untrusted
/// input.
const START_MARKER: &str = "__CORRAL_ENV_START__";
const END_MARKER: &str = "__CORRAL_ENV_END__";

/// Cap on how long the login shell may take. Loading a heavy plugin manager
/// (oh-my-zsh, nvm, conda) can take a second or two; a broken rc that blocks
/// on input must not hang corral's startup forever.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// Variables the capture shell always sets that describe *that shell* (its
/// cwd, its tty, its nesting level) rather than the user's environment —
/// adopting them would mislead child processes.
const SKIPPED: &[&str] = &[
    "PWD",
    "OLDPWD",
    "SHLVL",
    "_",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "LINES",
    "COLUMNS",
    "SECURITYSESSIONID",
    // Set on the capture child only (see `capture`) to stop oh-my-zsh's
    // auto-updater from prompting.
    "DISABLE_AUTO_UPDATE",
];

#[cfg(target_os = "macos")]
const DEFAULT_SHELL: &str = "/bin/zsh";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SHELL: &str = "/bin/sh";

/// Run the user's login + interactive shell once and return its environment.
///
/// Best-effort by design: every failure (no shell, timeout, unparseable
/// output) is returned as a string for the caller to log, and callers treat a
/// missing environment as "keep what we already have".
pub fn capture() -> Result<Vec<(OsString, OsString)>, String> {
    let shell = login_shell().unwrap_or_else(|| OsString::from(DEFAULT_SHELL));

    let mut cmd = std::process::Command::new(&shell);
    cmd.args(["-l", "-i", "-c"])
        .arg(format!(
            "printf '%s' '{START_MARKER}'; /usr/bin/env -0 2>/dev/null || /usr/bin/env; \
             printf '%s' '{END_MARKER}'"
        ))
        // Oh My Zsh's update check can block waiting on a prompt.
        .env("DISABLE_AUTO_UPDATE", "true")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(home) = dirs::home_dir() {
        // rc files that assume a home-directory cwd (e.g. relative sources)
        // behave the same here as in a fresh terminal.
        cmd.current_dir(home);
    }

    let stdout = run_with_timeout(cmd, CAPTURE_TIMEOUT).ok_or_else(|| {
        format!(
            "{shell:?} produced no environment within {}s",
            CAPTURE_TIMEOUT.as_secs()
        )
    })?;
    let vars = parse(&stdout);
    if vars.is_empty() {
        return Err(format!("{shell:?} produced no parseable environment"));
    }
    Ok(vars)
}

/// The user's login shell, from the password database rather than `$SHELL`.
///
/// GUI-launched apps don't always get `SHELL`, and when they do it can be a
/// stale value from whatever launched the app. The passwd entry is what
/// Terminal.app itself uses, so it's the right shell to read rc files from.
#[cfg(unix)]
fn login_shell() -> Option<OsString> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    // `getpwuid` returns a pointer into a static buffer; use the `_r` variant
    // with our own, since the buffer's lifetime spans a child-process spawn.
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the duration of the call; `entry`
    // and `buf` outlive it.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || entry.pw_shell.is_null() {
        return None;
    }
    // SAFETY: `pw_shell` points at a NUL-terminated string owned by `buf`
    // (or by libc), valid until the next call.
    let shell = unsafe { CStr::from_ptr(entry.pw_shell) };
    let shell = shell.to_bytes();
    if shell.is_empty() {
        return None;
    }
    Some(OsString::from(std::ffi::OsStr::from_bytes(shell)))
}

#[cfg(not(unix))]
fn login_shell() -> Option<OsString> {
    std::env::var_os("SHELL").filter(|s| !s.is_empty())
}

/// Spawn `cmd`, read all of its stdout, and kill it if it outlives `timeout`.
///
/// `std::process::Command::output()` would be simpler but has no timeout,
/// and a hung prompt in a user's rc file would otherwise hang corral before
/// its window ever appears.
fn run_with_timeout(mut cmd: std::process::Command, timeout: Duration) -> Option<Vec<u8>> {
    cmd.stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;

    // Read concurrently with waiting: a shell that emits more than the pipe
    // buffer would block forever if we waited first.
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        // A killed shell closing the pipe mid-read is expected; keep what we
        // got either way.
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return reader.join().ok(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return None;
            }
        }
    }
}

/// Parse the shell's dump between the two markers.
///
/// `env -0` NUL-separates entries, but macOS's `/usr/bin/env` only gained
/// `-0` in macOS 13 (Zed hit the same thing): on older systems it either
/// fails (the `|| /usr/bin/env` fallback in `capture` covers that) or ignores
/// the flag and emits newlines. Both shapes are handled by picking the
/// separator that's actually present.
fn parse(stdout: &[u8]) -> Vec<(OsString, OsString)> {
    let Some(start) = find(stdout, START_MARKER.as_bytes()) else {
        return Vec::new();
    };
    let body_start = start + START_MARKER.len();
    let Some(end) = find(&stdout[body_start..], END_MARKER.as_bytes()) else {
        return Vec::new();
    };
    let body = &stdout[body_start..body_start + end];
    let separator = if body.contains(&0) { 0 } else { b'\n' };

    body.split(|b| *b == separator)
        .filter_map(parse_entry)
        .filter(|(key, _)| !should_skip(key))
        .collect()
}

fn parse_entry(entry: &[u8]) -> Option<(OsString, OsString)> {
    // Newline-separated output on old macOS can carry CRLF for CRLF-ified
    // shells; trim it so values don't end in `\r`.
    let entry = entry.strip_suffix(b"\r").unwrap_or(entry);
    let equals = entry.iter().position(|b| *b == b'=')?;
    if equals == 0 {
        return None;
    }
    Some((os_string(&entry[..equals]), os_string(&entry[equals + 1..])))
}

fn os_string(bytes: &[u8]) -> OsString {
    // Environment values are byte strings on Unix; lossy conversion keeps
    // odd bytes rather than dropping the whole variable.
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

fn should_skip(key: &OsStr) -> bool {
    SKIPPED.iter().any(|skipped| key == OsStr::new(skipped))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Adopt a captured shell environment into this process, returning how many
/// variables were actually set (existing ones and the skipped bookkeeping
/// vars are not counted).
///
/// # Safety
///
/// `std::env::set_var` is unsound if another thread reads or writes the
/// environment concurrently. Call this from `main`/`run()` before any threads
/// are started (corral does this before the Tauri builder spawns anything).
pub unsafe fn apply(captured: &[(OsString, OsString)]) -> usize {
    let current: HashMap<OsString, OsString> = std::env::vars_os().collect();
    let updates = new_values(&current, captured);
    for (key, value) in &updates {
        // SAFETY: the caller guarantees no concurrent environment access.
        unsafe { std::env::set_var(key, value) };
    }
    updates.len()
}

/// Which variables [`apply`] should set, given the current process
/// environment. Split out from `apply` so the merge rules are testable
/// without mutating the real (process-global) environment.
fn new_values(
    current: &HashMap<OsString, OsString>,
    captured: &[(OsString, OsString)],
) -> Vec<(OsString, OsString)> {
    let mut updates = Vec::new();
    for (key, value) in captured {
        // PATH is handled separately below: unlike other vars, the shell's
        // value must win even though the process already has one.
        if key == OsStr::new("PATH") || should_skip(key) || current.contains_key(key) {
            continue;
        }
        updates.push((key.clone(), value.clone()));
    }

    if let Some((_, shell_path)) = captured.iter().find(|(key, _)| key == OsStr::new("PATH")) {
        let mut dirs: Vec<PathBuf> = std::env::split_paths(shell_path).collect();
        if let Some(current_path) = current.get(OsStr::new("PATH")) {
            for dir in std::env::split_paths(current_path) {
                if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
        if let Ok(joined) = std::env::join_paths(&dirs) {
            updates.push((OsString::from("PATH"), joined));
        }
    }

    updates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(raw: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        raw.iter()
            .map(|(k, v)| (OsString::from(*k), OsString::from(*v)))
            .collect()
    }

    fn current(raw: &[(&str, &str)]) -> HashMap<OsString, OsString> {
        pairs(raw).into_iter().collect()
    }

    #[test]
    fn parses_nul_separated_pairs_between_markers() {
        let raw =
            b"junk before\n__CORRAL_ENV_START__FOO=bar\0PATH=/a:/b\0__CORRAL_ENV_END__trailing";
        assert_eq!(parse(raw), pairs(&[("FOO", "bar"), ("PATH", "/a:/b")]));
    }

    #[test]
    fn parses_newline_separated_output_as_the_old_macos_fallback() {
        let raw = b"__CORRAL_ENV_START__FOO=bar\nPATH=/a:/b\n__CORRAL_ENV_END__";
        assert_eq!(parse(raw), pairs(&[("FOO", "bar"), ("PATH", "/a:/b")]));
    }

    #[test]
    fn parses_crlf_entries_without_a_trailing_carriage_return() {
        let raw = b"__CORRAL_ENV_START__FOO=bar\r\n__CORRAL_ENV_END__";
        assert_eq!(parse(raw), pairs(&[("FOO", "bar")]));
    }

    #[test]
    fn keeps_equals_signs_in_values() {
        let raw = b"__CORRAL_ENV_START__CMD=a=b=c\0__CORRAL_ENV_END__";
        assert_eq!(parse(raw), pairs(&[("CMD", "a=b=c")]));
    }

    #[test]
    fn ignores_entries_without_a_key_or_separator() {
        let raw = b"__CORRAL_ENV_START__=empty-key\0no-equals\0OK=1\0\0__CORRAL_ENV_END__";
        assert_eq!(parse(raw), pairs(&[("OK", "1")]));
    }

    #[test]
    fn drops_the_capture_shells_own_bookkeeping() {
        let raw = b"__CORRAL_ENV_START__PWD=/Users/x\0SHLVL=2\0TERM=xterm\0\
                    TERM_PROGRAM=Apple_Terminal\0KEEP=1\0__CORRAL_ENV_END__";
        assert_eq!(parse(raw), pairs(&[("KEEP", "1")]));
    }

    #[test]
    fn requires_both_markers() {
        assert!(parse(b"FOO=bar").is_empty());
        assert!(parse(b"__CORRAL_ENV_START__FOO=bar").is_empty());
    }

    #[test]
    fn adopts_missing_variables_and_keeps_existing_ones() {
        let current = current(&[("KEEP", "app"), ("PATH", "/app/bin")]);
        let captured = pairs(&[
            ("KEEP", "shell"),
            ("NEW", "from-shell"),
            ("PATH", "/shell/bin"),
        ]);

        let updates = new_values(&current, &captured);

        assert_eq!(
            updates,
            pairs(&[("NEW", "from-shell"), ("PATH", "/shell/bin:/app/bin")])
        );
    }

    #[test]
    fn path_appends_app_only_entries_without_duplicating() {
        let current = current(&[("PATH", "/shell/bin:/app/bin")]);
        let captured = pairs(&[("PATH", "/shell/bin")]);

        assert_eq!(
            new_values(&current, &captured),
            pairs(&[("PATH", "/shell/bin:/app/bin")])
        );
    }

    #[test]
    fn skips_bookkeeping_even_when_missing_from_the_process_env() {
        let updates = new_values(&current(&[]), &pairs(&[("PWD", "/x"), ("KEEP", "1")]));
        assert_eq!(updates, pairs(&[("KEEP", "1")]));
    }

    #[test]
    fn a_capture_without_path_still_adopts_other_variables() {
        let updates = new_values(&current(&[("PATH", "/app/bin")]), &pairs(&[("FOO", "1")]));
        assert_eq!(updates, pairs(&[("FOO", "1")]));
    }
}
