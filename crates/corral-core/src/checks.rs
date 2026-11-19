//! GitHub check-run status, PR comments, and merge — shells out to `gh` the
//! same way `pr.rs` does; `gh` owns auth, corral just drives it.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub workflow: String,
    pub bucket: CheckBucket,
    pub url: String,
}

/// Coarse aggregate of a PR's check runs — just enough for the sidebar to
/// show "CI running"/"CI failed" for every worktree without shipping the full
/// per-check list (`checksByWtId`, which the PR tab fetches on demand).
/// `total == 0` means "PR has no checks configured"; the whole summary is
/// held as `Option` by callers so "unknown / no PR" stays distinct from
/// "known to have no checks".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksSummary {
    pub total: u32,
    pub pending: u32,
    /// Terminal non-pass runs — `fail` plus `cancel`, since a cancelled run
    /// is equally "something didn't pass and needs a look".
    pub failed: u32,
}

impl ChecksSummary {
    /// Whether nothing is still running and nothing failed — i.e. every
    /// check either passed or was skipped.
    pub fn is_green(&self) -> bool {
        self.pending == 0 && self.failed == 0
    }
}

/// Fold a full check-run list into the coarse [`ChecksSummary`].
pub fn summarize_checks(checks: &[CheckRun]) -> ChecksSummary {
    let mut summary = ChecksSummary::default();
    for check in checks {
        summary.total += 1;
        match check.bucket {
            CheckBucket::Pending => summary.pending += 1,
            CheckBucket::Fail | CheckBucket::Cancel => summary.failed += 1,
            CheckBucket::Pass | CheckBucket::Skipping => {}
        }
    }
    summary
}

#[derive(Deserialize)]
struct GhCheck {
    name: String,
    workflow: String,
    bucket: String,
    link: String,
}

/// Maps `gh pr checks --json name,workflow,bucket,link`'s stdout. An
/// unrecognized bucket string falls back to `Pending` rather than erroring —
/// `gh` may add new bucket values corral doesn't know about yet, and
/// "pending" is the safe (non-mergeable) default.
pub fn checks_from_json(json: &str) -> Result<Vec<CheckRun>> {
    let raw: Vec<GhCheck> = serde_json::from_str(json)?;
    Ok(raw
        .into_iter()
        .map(|c| CheckRun {
            name: c.name,
            workflow: c.workflow,
            bucket: match c.bucket.as_str() {
                "pass" => CheckBucket::Pass,
                "fail" => CheckBucket::Fail,
                "skipping" => CheckBucket::Skipping,
                "cancel" => CheckBucket::Cancel,
                _ => CheckBucket::Pending,
            },
            url: c.link,
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrComment {
    /// Stable identity for the frontend's list key: the issue comment's URL,
    /// or `review:<id>` for a review (`gh` exposes no per-review URL).
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    /// Link to the comment on GitHub. Empty for reviews, which `gh` gives no
    /// URL for.
    pub url: String,
    /// Empty for an ordinary issue comment; otherwise GitHub's review state
    /// (`APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`) — reviews
    /// with a body show up in the same timeline on github.com.
    #[serde(default)]
    pub review_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrReview {
    pub author: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, or `DISMISSED`.
    pub state: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetails {
    pub title: String,
    pub body: String,
    /// GitHub login of the PR's author.
    pub author: String,
    /// GitHub's review decision: `APPROVED`, `CHANGES_REQUESTED`,
    /// `REVIEW_REQUIRED`, or an empty string when no review is required.
    #[serde(default)]
    pub review_decision: String,
    pub comments: Vec<PrComment>,
    /// Every submitted review, in submission order — the Approvals panel
    /// folds them down to each reviewer's latest state. Kept separate from
    /// `comments` because a review may have no body.
    #[serde(default)]
    pub reviews: Vec<PrReview>,
}

#[derive(Deserialize)]
struct GhAuthor {
    login: String,
}

#[derive(Deserialize)]
struct GhComment {
    author: GhAuthor,
    body: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    url: String,
}

#[derive(Deserialize)]
struct GhReview {
    id: String,
    author: GhAuthor,
    body: String,
    #[serde(rename = "submittedAt")]
    submitted_at: String,
    state: String,
}

#[derive(Deserialize)]
struct GhPrView {
    title: String,
    body: String,
    author: GhAuthor,
    #[serde(rename = "reviewDecision", default)]
    review_decision: String,
    comments: Vec<GhComment>,
    #[serde(default)]
    reviews: Vec<GhReview>,
}

/// Maps `gh pr view --json
/// title,body,author,reviewDecision,comments,reviews`'s stdout.
///
/// Issue comments and review bodies are folded into one chronological list,
/// mirroring github.com's timeline: a review such as "nice, much better"
/// (submitted with an approve) is a comment the user expects to see, but
/// `gh`'s `comments` field only carries issue comments. Reviews with an empty
/// body are skipped — they'd render as blank "No content." cards, and the
/// summary card's Approvals column already reports the review decision.
pub fn pr_details_from_json(json: &str) -> Result<PrDetails> {
    let raw: GhPrView = serde_json::from_str(json)?;
    let mut reviews: Vec<PrReview> = raw
        .reviews
        .iter()
        .map(|r| PrReview {
            author: r.author.login.clone(),
            state: r.state.clone(),
            submitted_at: r.submitted_at.clone(),
        })
        .collect();
    let mut comments: Vec<PrComment> = raw
        .comments
        .into_iter()
        .map(|c| PrComment {
            id: c.url.clone(),
            author: c.author.login,
            body: c.body,
            created_at: c.created_at,
            url: c.url,
            review_state: String::new(),
        })
        .collect();
    comments.extend(
        raw.reviews
            .into_iter()
            .filter(|r| !r.body.trim().is_empty())
            .map(|r| PrComment {
                id: format!("review:{}", r.id),
                author: r.author.login,
                body: r.body,
                created_at: r.submitted_at,
                url: String::new(),
                review_state: r.state,
            }),
    );
    // Both timestamp fields are ISO 8601 UTC (`...Z`), so lexicographic order
    // is chronological — GitHub's timeline shows the two kinds interleaved.
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    // Likewise oldest-first, so the frontend can fold each reviewer down to
    // their latest review by taking the last entry.
    reviews.sort_by(|a, b| a.submitted_at.cmp(&b.submitted_at));
    Ok(PrDetails {
        title: raw.title,
        body: raw.body,
        author: raw.author.login,
        review_decision: raw.review_decision,
        comments,
        reviews,
    })
}

/// Parses the run ID out of a `gh pr checks` check's `link`, e.g.
/// `https://github.com/owner/repo/actions/runs/123456/job/789` -> `"123456"`.
/// Returns `None` if the URL doesn't contain a numeric `/runs/<id>` segment.
pub fn run_id_from_check_url(url: &str) -> Option<&str> {
    let after = url.split("/runs/").nth(1)?;
    let id = after.split('/').next()?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MergePrResult {
    Merged,
    /// GitHub rejected the merge — `reason` is its stderr (e.g. "required
    /// status check ... is expected"). Empty stderr falls back to a generic
    /// `"no reason given"` so the UI never shows a blank reason.
    /// Client-side check gating in the
    /// frontend is a UX nicety; this is the authoritative outcome.
    Rejected {
        reason: String,
    },
}

pub fn merge_pr_result_from_output(success: bool, stderr: &str) -> MergePrResult {
    if success {
        MergePrResult::Merged
    } else {
        let reason = stderr.trim();
        MergePrResult::Rejected {
            reason: if reason.is_empty() {
                "no reason given".to_string()
            } else {
                reason.to_string()
            },
        }
    }
}

/// Whether `stderr` from a `gh pr ...` command means "the PR (or its checks)
/// aren't available yet" rather than a real failure — GitHub's
/// eventual-consistency window right after a PR is created, or a worktree
/// with no PR at all. Callers treat this as empty data instead of an error,
/// so a freshly-pushed PR doesn't flash a "couldn't refresh" banner while
/// GitHub catches up.
fn is_no_pr_yet(stderr: &str) -> bool {
    stderr.contains("no pull requests found")
        || stderr.contains("no checks reported")
        || stderr.contains("no commit found on the pull request")
}

/// The open PR's check runs for the current branch in `dir` —
/// `gh pr checks --json name,workflow,bucket,link`. Returns an empty `Vec`
/// if there's no PR yet or it has no checks configured, rather than erroring
/// (mirrors `pr_status`'s "no PR yet" -> `Ok(None)` convention in `pr.rs`).
pub fn pr_checks(dir: &Path) -> Result<Vec<CheckRun>> {
    let output = Command::new("gh")
        .args(["pr", "checks", "--json", "name,workflow,bucket,link"])
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_no_pr_yet(&stderr) {
            return Ok(Vec::new());
        }
        return Err(Error::Gh {
            command: "gh pr checks".to_string(),
            stderr: stderr.trim().to_string(),
        });
    }
    checks_from_json(&String::from_utf8(output.stdout)?)
}

/// The open PR's title, body, author, review decision, and timeline comments
/// (issue comments plus review bodies) — `gh pr view --json
/// title,body,author,reviewDecision,comments,reviews`. `Ok(None)` means "no
/// PR yet" (mirrors `pr_status`'s convention), so the frontend can
/// distinguish it from a real `gh` failure.
pub fn pr_details(dir: &Path) -> Result<Option<PrDetails>> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            "--json",
            "title,body,author,reviewDecision,comments,reviews",
        ])
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_no_pr_yet(&stderr) {
            return Ok(None);
        }
        return Err(Error::Gh {
            command: "gh pr view".to_string(),
            stderr: stderr.trim().to_string(),
        });
    }
    Ok(Some(pr_details_from_json(&String::from_utf8(
        output.stdout,
    )?)?))
}

/// How many trailing lines of a failed check's log to keep — enough context
/// to diagnose without flooding the PTY/agent context on a huge log.
const LOG_TAIL_LINES: usize = 200;

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// The failing job's log for one check run, truncated to its last
/// [`LOG_TAIL_LINES`] lines — `gh run view <run_id> --log-failed`, where
/// `run_id` is parsed out of the check's `link` URL.
pub fn check_log(dir: &Path, check_url: &str) -> Result<String> {
    let run_id = run_id_from_check_url(check_url)
        .ok_or_else(|| Error::Other(format!("couldn't parse a run id out of {check_url:?}")))?;
    let output = Command::new("gh")
        .args(["run", "view", run_id, "--log-failed"])
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        return Err(Error::Gh {
            command: "gh run view".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(tail_lines(
        &String::from_utf8(output.stdout)?,
        LOG_TAIL_LINES,
    ))
}

/// Squash-merges the current branch's open PR — `gh pr merge --squash`.
/// GitHub enforces branch-protection required checks server-side regardless
/// of what corral's UI shows, so a rejection here is always authoritative
/// (see `MergePrResult::Rejected`).
pub fn merge_pr(dir: &Path) -> Result<MergePrResult> {
    let output = Command::new("gh")
        .args(["pr", "merge", "--squash"])
        .current_dir(dir)
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(merge_pr_result_from_output(
        output.status.success(),
        &stderr,
    ))
}

/// Marks the current branch's draft PR as ready for review —
/// `gh pr ready`.
pub fn mark_pr_ready(dir: &Path) -> Result<()> {
    let output = Command::new("gh")
        .args(["pr", "ready"])
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        return Err(Error::Gh {
            command: "gh pr ready".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_from_json_maps_all_known_buckets() {
        let json = r#"[
            {"name":"build","workflow":"ci.yml","bucket":"pass","link":"https://x/1"},
            {"name":"test","workflow":"ci.yml","bucket":"fail","link":"https://x/2"},
            {"name":"lint","workflow":"ci.yml","bucket":"pending","link":"https://x/3"},
            {"name":"docs","workflow":"ci.yml","bucket":"skipping","link":"https://x/4"},
            {"name":"deploy","workflow":"ci.yml","bucket":"cancel","link":"https://x/5"}
        ]"#;
        let checks = checks_from_json(json).unwrap();
        assert_eq!(checks.len(), 5);
        assert_eq!(checks[0].bucket, CheckBucket::Pass);
        assert_eq!(checks[1].bucket, CheckBucket::Fail);
        assert_eq!(checks[2].bucket, CheckBucket::Pending);
        assert_eq!(checks[3].bucket, CheckBucket::Skipping);
        assert_eq!(checks[4].bucket, CheckBucket::Cancel);
        assert_eq!(checks[0].name, "build");
        assert_eq!(checks[0].url, "https://x/1");
    }

    #[test]
    fn checks_from_json_defaults_unknown_bucket_to_pending() {
        let json =
            r#"[{"name":"x","workflow":"w","bucket":"weird_new_value","link":"https://x/1"}]"#;
        assert_eq!(
            checks_from_json(json).unwrap()[0].bucket,
            CheckBucket::Pending
        );
    }

    fn run(bucket: CheckBucket) -> CheckRun {
        CheckRun {
            name: "c".into(),
            workflow: "w".into(),
            bucket,
            url: "https://x".into(),
        }
    }

    #[test]
    fn summarize_checks_counts_total_pending_and_failed() {
        let summary = summarize_checks(&[
            run(CheckBucket::Pass),
            run(CheckBucket::Pending),
            run(CheckBucket::Pending),
            run(CheckBucket::Fail),
            run(CheckBucket::Cancel),
            run(CheckBucket::Skipping),
        ]);
        assert_eq!(summary.total, 6);
        assert_eq!(summary.pending, 2);
        // Both fail and cancel count as "failed" for the sidebar's purposes.
        assert_eq!(summary.failed, 2);
        assert!(!summary.is_green());
    }

    #[test]
    fn summarize_checks_is_green_when_only_pass_and_skip() {
        let summary = summarize_checks(&[run(CheckBucket::Pass), run(CheckBucket::Skipping)]);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.pending, 0);
        assert_eq!(summary.failed, 0);
        assert!(summary.is_green());
    }

    #[test]
    fn summarize_checks_of_empty_list_is_green_with_zero_total() {
        let summary = summarize_checks(&[]);
        assert_eq!(summary.total, 0);
        assert!(summary.is_green());
    }

    #[test]
    fn pr_details_from_json_maps_title_body_and_comments() {
        let json = r###"{"title":"Add feature","body":"## Summary\n- did a thing","author":{"login":"octocat"},"reviewDecision":"REVIEW_REQUIRED","comments":[{"author":{"login":"github-actions"},"body":"CI note","createdAt":"2026-07-15T00:00:00Z","url":"https://x/c1"}]}"###;
        let details = pr_details_from_json(json).unwrap();
        assert_eq!(details.title, "Add feature");
        assert_eq!(details.body, "## Summary\n- did a thing");
        assert_eq!(details.author, "octocat");
        assert_eq!(details.review_decision, "REVIEW_REQUIRED");
        assert_eq!(details.comments.len(), 1);
        assert_eq!(details.comments[0].id, "https://x/c1");
        assert_eq!(details.comments[0].author, "github-actions");
        assert_eq!(details.comments[0].body, "CI note");
        assert_eq!(details.comments[0].url, "https://x/c1");
        assert_eq!(details.comments[0].review_state, "");
    }

    #[test]
    fn pr_details_from_json_includes_review_bodies_in_chronological_order() {
        // `gh pr view --json comments` only returns issue comments — a review
        // submitted with a body (what github.com shows as a timeline comment)
        // lives under `reviews` instead. Regression: the PR tab said "No
        // comments yet" for a PR whose only comment was an approving review.
        let json = r#"{
            "title":"t",
            "body":"b",
            "author":{"login":"octocat"},
            "comments":[
                {"author":{"login":"alice"},"body":"later issue comment","createdAt":"2026-11-18T20:00:00Z","url":"https://x/c2"},
                {"author":{"login":"alice"},"body":"earlier issue comment","createdAt":"2026-11-18T18:00:00Z","url":"https://x/c1"}
            ],
            "reviews":[
                {"id":"PRR_1","author":{"login":"jhsu"},"body":"nice, much better","submittedAt":"2026-11-18T19:24:44Z","state":"APPROVED"},
                {"id":"PRR_2","author":{"login":"bob"},"body":"","submittedAt":"2026-11-18T21:00:00Z","state":"COMMENTED"}
            ]
        }"#;
        let details = pr_details_from_json(json).unwrap();
        // The bodyless review is dropped; the rest interleave by time.
        assert_eq!(details.comments.len(), 3);
        assert_eq!(details.comments[0].body, "earlier issue comment");
        assert_eq!(details.comments[1].body, "nice, much better");
        assert_eq!(details.comments[2].body, "later issue comment");
        // Review entries get a synthetic key (no URL from `gh`) and keep their
        // review state so the UI can say "approved these changes".
        assert_eq!(details.comments[1].id, "review:PRR_1");
        assert_eq!(details.comments[1].url, "");
        assert_eq!(details.comments[1].review_state, "APPROVED");
        assert_eq!(details.comments[1].author, "jhsu");
        assert_eq!(details.comments[1].created_at, "2026-11-18T19:24:44Z");
        // The Approvals panel keeps every review (even a bodyless one) so it
        // can show who reviewed and in what state, oldest first.
        assert_eq!(details.reviews.len(), 2);
        assert_eq!(details.reviews[0].author, "jhsu");
        assert_eq!(details.reviews[0].state, "APPROVED");
        assert_eq!(details.reviews[1].author, "bob");
        assert_eq!(details.reviews[1].state, "COMMENTED");
    }

    #[test]
    fn pr_details_from_json_handles_no_comments() {
        let json = r#"{"title":"t","body":"b","author":{"login":"a"},"comments":[]}"#;
        let details = pr_details_from_json(json).unwrap();
        assert!(details.comments.is_empty());
        assert!(details.reviews.is_empty());
        // `reviewDecision` is absent when no review is required — defaults to
        // empty rather than failing the whole parse.
        assert_eq!(details.review_decision, "");
    }

    #[test]
    fn run_id_from_check_url_extracts_numeric_run_id() {
        assert_eq!(
            run_id_from_check_url("https://github.com/o/r/actions/runs/123456/job/789"),
            Some("123456")
        );
    }

    #[test]
    fn run_id_from_check_url_none_without_runs_segment() {
        assert_eq!(run_id_from_check_url("https://github.com/o/r/pull/1"), None);
    }

    #[test]
    fn run_id_from_check_url_none_when_segment_not_numeric() {
        assert_eq!(
            run_id_from_check_url("https://github.com/o/r/actions/runs/abc/job/789"),
            None
        );
    }

    #[test]
    fn merge_pr_result_from_output_maps_success() {
        assert!(matches!(
            merge_pr_result_from_output(true, ""),
            MergePrResult::Merged
        ));
    }

    #[test]
    fn merge_pr_result_from_output_maps_failure_with_trimmed_reason() {
        let result = merge_pr_result_from_output(false, "  required check missing  \n");
        match result {
            MergePrResult::Rejected { reason } => assert_eq!(reason, "required check missing"),
            _ => panic!("expected Rejected"),
        }
    }

    #[test]
    fn merge_pr_result_from_output_falls_back_when_stderr_is_empty() {
        let result = merge_pr_result_from_output(false, "   \n");
        match result {
            MergePrResult::Rejected { reason } => assert_eq!(reason, "no reason given"),
            _ => panic!("expected Rejected"),
        }
    }

    #[test]
    fn is_no_pr_yet_recognizes_github_eventual_consistency_errors() {
        assert!(is_no_pr_yet("no pull requests found"));
        assert!(is_no_pr_yet("no pull requests found for branch \"feat/x\""));
        assert!(is_no_pr_yet("no checks reported on the 'feat/x' branch"));
        assert!(is_no_pr_yet("no commit found on the pull request"));
        assert!(!is_no_pr_yet("some other error"));
    }
}
