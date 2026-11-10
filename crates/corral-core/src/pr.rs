//! Query a worktree's PR status from GitHub via the `gh` CLI.
//!
//! Reads the current state (`open`, `merged`, or `closed`) and metadata (number, URL,
//! draft status) of the PR associated with the worktree's branch. Shells out to `gh pr view`
//! the same way `git.rs` shells out to `git` — `gh` owns auth (`gh auth login`),
//! corral just drives it.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A PR's lifecycle state as GitHub reports it. Authoritative for "was this
/// merged" — unlike local git-ancestry checks, it's correct regardless of
/// merge strategy: a GitHub squash-merge creates a brand-new commit that
/// would never satisfy `git merge-base --is-ancestor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrInfo {
    pub number: u32,
    pub url: String,
    pub state: PrState,
    pub is_draft: bool,
}

/// Shape of `gh pr view --json number,url,state,isDraft,headRefOid`'s stdout.
#[derive(Deserialize)]
struct GhPrView {
    number: u32,
    url: String,
    state: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    /// The head branch's tip as of the PR's last sync — used to reject a PR
    /// that no longer corresponds to the branch currently checked out (a
    /// reused branch name that `gh pr view` still resolves to a long-merged
    /// PR). `None` when GitHub didn't report one.
    #[serde(rename = "headRefOid", default)]
    head_ref_oid: Option<String>,
}

/// Parse `gh pr view` output into [`PrInfo`] plus the head ref oid, so
/// [`pr_status`] can check whether the PR actually belongs to the current
/// branch's history.
fn pr_info_from_json(json: &str) -> Result<(PrInfo, Option<String>)> {
    let raw: GhPrView = serde_json::from_str(json)?;
    let state = match raw.state.as_str() {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ => PrState::Open,
    };
    Ok((
        PrInfo {
            number: raw.number,
            url: raw.url,
            state,
            is_draft: raw.is_draft,
        },
        raw.head_ref_oid,
    ))
}

/// Whether `head` and the current `HEAD` sit on the same commit lineage (one
/// is an ancestor of the other) — the criterion for "this PR is about the
/// branch checked out in `dir`". `git merge-base --is-ancestor` exits 0 for
/// ancestor and 1 for not-ancestor; [`crate::git::git`] surfaces both the
/// non-ancestor and error cases as `Err`, so `.is_ok()` means "is ancestor".
fn same_lineage(dir: &Path, head: &str) -> bool {
    let is_ancestor =
        |a: &str, b: &str| crate::git::git(dir, &["merge-base", "--is-ancestor", a, b]).is_ok();
    is_ancestor(head, "HEAD") || is_ancestor("HEAD", head)
}

/// The current branch's PR in `dir`, if one has ever been opened for it —
/// `gh pr view --json ...`. `Ok(None)` means either "no PR exists yet" or
/// "the PR GitHub has for this branch name belongs to a *different* branch"
/// — a reused branch name would otherwise inherit a long-merged PR's state
/// (see [`same_lineage`]).
pub fn pr_status(dir: &Path) -> Result<Option<PrInfo>> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            "--json",
            "number,url,state,isDraft,headRefOid",
        ])
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no pull requests found") {
            return Ok(None);
        }
        return Err(Error::Gh {
            command: "gh pr view".to_string(),
            stderr: stderr.trim().to_string(),
        });
    }
    let stdout = String::from_utf8(output.stdout)?;
    let (info, head_ref_oid) = pr_info_from_json(&stdout)?;
    if let Some(head) = head_ref_oid.as_deref()
        && !same_lineage(dir, head)
    {
        tracing::debug!(
            "pr_status: PR #{} head {head} is not on the current branch's lineage; treating as no PR",
            info.number
        );
        return Ok(None);
    }
    Ok(Some(info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_info_from_json_maps_open_state() {
        let json = r#"{"number":42,"url":"https://github.com/x/y/pull/42","state":"OPEN","isDraft":false}"#;
        let (info, head) = pr_info_from_json(json).unwrap();
        assert_eq!(info.number, 42);
        assert_eq!(info.url, "https://github.com/x/y/pull/42");
        assert_eq!(info.state, PrState::Open);
        assert!(!info.is_draft);
        assert_eq!(head, None);
    }

    #[test]
    fn pr_info_from_json_maps_merged_state() {
        let json = r#"{"number":1,"url":"https://x","state":"MERGED","isDraft":false}"#;
        assert_eq!(pr_info_from_json(json).unwrap().0.state, PrState::Merged);
    }

    #[test]
    fn pr_info_from_json_maps_closed_state() {
        let json = r#"{"number":1,"url":"https://x","state":"CLOSED","isDraft":false}"#;
        assert_eq!(pr_info_from_json(json).unwrap().0.state, PrState::Closed);
    }

    #[test]
    fn pr_info_from_json_reads_draft_flag() {
        let json = r#"{"number":1,"url":"https://x","state":"OPEN","isDraft":true}"#;
        assert!(pr_info_from_json(json).unwrap().0.is_draft);
    }

    #[test]
    fn pr_info_from_json_reads_head_ref_oid() {
        let json = r#"{"number":1,"url":"https://x","state":"MERGED","isDraft":false,"headRefOid":"799cfd6903dfadd3aaef1effb3e6327db8379692"}"#;
        let (info, head) = pr_info_from_json(json).unwrap();
        assert_eq!(info.state, PrState::Merged);
        assert_eq!(
            head.as_deref(),
            Some("799cfd6903dfadd3aaef1effb3e6327db8379692")
        );
    }

    #[test]
    fn pr_info_from_json_errors_on_garbage() {
        assert!(pr_info_from_json("not json").is_err());
    }
}
