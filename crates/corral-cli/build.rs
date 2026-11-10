//! Embeds the short git commit sha into the build as `CORRAL_GIT_SHA`, for
//! use in `--version` output. Falls back to `"unknown"` when not building
//! inside a git checkout (e.g. from a source tarball) or if `git` isn't on
//! PATH, so the build never fails over a missing sha.

use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=CORRAL_GIT_SHA={sha}");
    // Re-run when HEAD moves to a different commit.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
