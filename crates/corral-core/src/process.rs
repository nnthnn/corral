//! Total resident memory (RSS) of this process tree, summed from `/proc`.
//!
//! "Is this eating my RAM?" is a real question when several parallel agent
//! sessions are running, so the sidebar header shows corral's own + all child
//! processes' RSS. Linux exposes per-process RSS as `VmRSS` (in kB) in
//! `/proc/<pid>/status`, and each process's direct children in
//! `/proc/<pid>/task/<pid>/children`, so the whole tree can be summed without
//! spawning any subprocess or linking against platform APIs.

/// Resident set size of `pid` in kilobytes, from the `VmRSS:` line of
/// `/proc/<pid>/status` — or `None` if the process has exited or the file
/// can't be read.
#[cfg(target_os = "linux")]
fn rss_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // Format: "VmRSS:\t  12345 kB" — parse the number, tolerating the
            // trailing unit and any surrounding whitespace.
            return rest.trim().trim_end_matches("kB").trim().parse().ok();
        }
    }
    None
}

/// Direct children of `pid`, parsed from `/proc/<pid>/task/<pid>/children`.
/// Returns an empty list on any failure (e.g. the process exited mid-walk).
#[cfg(target_os = "linux")]
fn child_pids(pid: u32) -> Vec<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .map(|s| {
            s.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Sum of this process's and every descendant's RSS, in bytes. Iterative
/// (explicit stack) so a deep process tree can't overflow the call stack.
/// Individual processes that vanish mid-walk (a child exits between being
/// listed and read) are skipped rather than failing the whole sum.
#[cfg(target_os = "linux")]
pub fn total_rss_bytes() -> Option<u64> {
    let mut total_kb = 0u64;
    let mut stack = vec![std::process::id()];
    while let Some(pid) = stack.pop() {
        if let Some(kb) = rss_kb(pid) {
            total_kb = total_kb.saturating_add(kb);
        }
        stack.extend(child_pids(pid));
    }
    Some(total_kb.saturating_mul(1024))
}

/// `/proc` is Linux-specific; other platforms have no equivalent cheap walk.
#[cfg(not(target_os = "linux"))]
pub fn total_rss_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn total_rss_bytes_is_nonzero_and_at_least_this_process() {
        // The current test process exists in /proc and has a nonzero RSS, so
        // the walk must at least cover it (can't assert an exact value — RSS
        // is inherently racy and machine-dependent).
        let bytes = super::total_rss_bytes();
        if cfg!(target_os = "linux") {
            assert!(bytes.unwrap() > 0);
        } else {
            assert!(bytes.is_none());
        }
    }
}
