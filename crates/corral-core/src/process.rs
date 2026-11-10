//! Total resident memory (RSS) of this process tree.
//!
//! "Is this eating my RAM?" is a real question when several parallel agent
//! sessions are running, so the sidebar header shows corral's own + all child
//! processes' RSS. Linux exposes per-process RSS as `VmRSS` (in kB) in
//! `/proc/<pid>/status`, and each process's direct children in
//! `/proc/<pid>/task/<pid>/children`, so the whole tree can be summed without
//! spawning any subprocess or linking against platform APIs.
//!
//! macOS has no `/proc`; there the whole system is sampled with a single
//! `ps -A -o pid=,ppid=,rss=` call and the tree is walked from that snapshot.

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

/// macOS: one `ps` snapshot for the whole system (`pid`, parent `pid`, RSS in
/// kB), then the same descendant walk as the Linux implementation. One
/// subprocess per poll rather than one per process — this runs once per poll
/// from the sidebar, not in a hot loop.
#[cfg(target_os = "macos")]
pub fn total_rss_bytes() -> Option<u64> {
    use std::collections::HashMap;

    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,rss="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    // pid -> (ppid, rss_kb). Any line that doesn't parse is skipped rather
    // than failing the whole read: the table is a snapshot, so a process can
    // exit between being listed and being walked, exactly as on Linux.
    let mut processes: HashMap<u32, (u32, u64)> = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(rss)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(pid), Ok(ppid), Ok(rss)) =
            (pid.parse::<u32>(), ppid.parse::<u32>(), rss.parse::<u64>())
        else {
            continue;
        };
        processes.insert(pid, (ppid, rss));
    }

    let this_pid = std::process::id();
    if !processes.contains_key(&this_pid) {
        // `ps` succeeded but we're somehow missing from its own snapshot —
        // report "unknown" rather than a misleadingly small number.
        return None;
    }

    let mut total_kb = 0u64;
    let mut stack = vec![this_pid];
    while let Some(pid) = stack.pop() {
        if let Some(&(_, rss)) = processes.get(&pid) {
            total_kb = total_kb.saturating_add(rss);
        }
        stack.extend(
            processes
                .iter()
                .filter(|(_, (ppid, _))| *ppid == pid)
                .map(|(child, _)| *child),
        );
    }
    Some(total_kb.saturating_mul(1024))
}

/// No cheap process-tree walk wired up on this platform, so the sidebar's
/// memory readout is simply hidden.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn total_rss_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn total_rss_bytes_is_nonzero_and_at_least_this_process() {
        // The current test process has a nonzero RSS, so the walk must at
        // least cover it (can't assert an exact value — RSS is inherently
        // racy and machine-dependent).
        let bytes = super::total_rss_bytes();
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            assert!(bytes.unwrap() > 0);
        } else {
            assert!(bytes.is_none());
        }
    }
}
