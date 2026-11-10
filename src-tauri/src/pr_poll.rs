//! Periodically fetches every remote-having worktree's GitHub PR state via
//! `corral_core::pr::pr_status`, caching it in memory so `list_worktrees`
//! can attach it to each `Worktree` for free rather than every sidebar row
//! triggering its own `gh` subprocess. Mirrors `conflict_poll.rs`'s shape:
//! one thread spawned once at app startup, running for the process's
//! lifetime.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use corral_core::checks::{self, ChecksSummary};
use corral_core::pr::{PrInfo, PrState};
use tauri::{AppHandle, Emitter, Manager};

/// One worktree's polled GitHub state — its PR plus a coarse CI summary.
/// Kept together so `list_worktrees` attaches both from a single lookup, and
/// so a PR that disappears (or closes) takes its stale check summary with it
/// instead of leaving the sidebar showing "CI running" next to a merged PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPr {
    pub info: PrInfo,
    pub checks: Option<ChecksSummary>,
}

/// `id` is the `"<project>/<feature>"` key from
/// `corral_core::agent_session::key` — same format the session store and
/// the frontend's `wtId` both already use.
#[derive(Default)]
pub struct PrPollCache(Mutex<HashMap<String, CachedPr>>);

impl PrPollCache {
    pub fn get(&self, id: &str) -> Option<CachedPr> {
        self.0.lock().unwrap().get(id).cloned()
    }

    fn insert(&self, id: &str, value: CachedPr) {
        self.0.lock().unwrap().insert(id.to_string(), value);
    }

    /// Returns whether an entry was actually present (and now removed).
    fn remove(&self, id: &str) -> bool {
        self.0.lock().unwrap().remove(id).is_some()
    }

    /// Drop every cached entry whose id isn't in `live`, returning how many
    /// were dropped. Called once per poll so a removed worktree's PR state
    /// can't be reattached to a later worktree that reuses the same
    /// `project/feature` — which would otherwise show a stale PR badge until
    /// the next successful `gh pr view` (and indefinitely if `gh` keeps
    /// erroring, since `apply_result` deliberately preserves the last value).
    fn retain_live(&self, live: &HashSet<String>) -> usize {
        let mut map = self.0.lock().unwrap();
        let before = map.len();
        map.retain(|id, _| live.contains(id));
        before - map.len()
    }
}

/// How often to re-poll every worktree's PR state. Coarse on purpose, same
/// reasoning as `conflict_poll.rs::POLL_INTERVAL`: this only needs to catch
/// drift on the order of minutes, and each tick costs up to two `gh`
/// subprocesses (real GitHub API calls) per worktree that has a remote —
/// `gh pr view`, plus `gh pr checks` when the PR is open.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Fetch a worktree's PR state and, when the PR is open, a coarse CI summary
/// for it. `checks` is only fetched for an open PR — a merged/closed PR has
/// no live checks to show. A transient `gh pr checks` failure keeps the last
/// known-good summary rather than blanking it (same best-effort philosophy as
/// `apply_result`'s error arm).
fn poll_worktree(cache: &PrPollCache, id: &str, path: &Path) -> bool {
    let result = corral_core::pr::pr_status(path);
    let checks = match &result {
        Ok(Some(info)) if info.state == PrState::Open => match checks::pr_checks(path) {
            Ok(runs) => Some(checks::summarize_checks(&runs)),
            Err(e) => {
                tracing::warn!("pr_poll: gh pr checks failed for {id}: {e}");
                cache.get(id).and_then(|c| c.checks)
            }
        },
        _ => None,
    };
    apply_result(cache, id, result, checks)
}

/// Merges one worktree's freshly-fetched PR state (plus its CI summary, if
/// any) into `cache`, returning whether the cache actually changed. A
/// transient error (network hiccup, `gh` not authed, rate limit) leaves the
/// last known-good cached value alone rather than evicting it — same
/// best-effort philosophy as `conflict_poll.rs::spawn`'s `git fetch`
/// failures, which are silently skipped rather than treated as "no longer
/// has a remote."
fn apply_result(
    cache: &PrPollCache,
    id: &str,
    result: corral_core::error::Result<Option<PrInfo>>,
    checks: Option<ChecksSummary>,
) -> bool {
    match result {
        Ok(Some(info)) => {
            // A non-open PR carries no live checks; drop any incoming summary
            // so a stale "CI running" can't linger next to a merged badge.
            let checks = if info.state == PrState::Open {
                checks
            } else {
                None
            };
            let next = CachedPr { info, checks };
            let changed = cache.get(id).as_ref() != Some(&next);
            cache.insert(id, next);
            changed
        }
        Ok(None) => cache.remove(id),
        Err(e) => {
            tracing::warn!("pr_poll: gh pr view failed for {id}: {e}");
            false
        }
    }
}

fn poll_once(cache: &PrPollCache) -> usize {
    let worktrees = match corral_core::worktree::list() {
        Ok(worktrees) => worktrees,
        Err(e) => {
            tracing::warn!("pr_poll: failed to list worktrees: {e}");
            return 0;
        }
    };

    // Evict entries for worktrees that no longer exist (removed via corral,
    // the CLI, or out-of-band) before re-polling the live ones.
    let live: HashSet<String> = worktrees
        .iter()
        .map(|wt| corral_core::agent_session::key(&wt.project, &wt.feature))
        .collect();
    let evicted = cache.retain_live(&live);

    let updated = worktrees
        .iter()
        .filter(|wt| wt.status.as_ref().is_some_and(|s| s.has_remote))
        .filter(|wt| {
            let id = corral_core::agent_session::key(&wt.project, &wt.feature);
            poll_worktree(cache, &id, &wt.path)
        })
        .count();

    evicted + updated
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let cache = app.state::<PrPollCache>();
        loop {
            let changed = poll_once(&cache);
            if changed > 0 {
                tracing::info!("pr_poll: updated PR state for {changed} worktree(s)");
                // Reuses the same event `conflict_poll.rs` emits — the
                // frontend's existing `fs-changed` listener already does a
                // full `loadAll()` refresh, which picks up the now-fresh
                // `pr_state` on every worktree.
                let _ = app.emit("fs-changed", ());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PrInfo {
        PrInfo {
            number: 42,
            url: "https://github.com/x/y/pull/42".to_string(),
            state: PrState::Open,
            is_draft: false,
        }
    }

    /// A cached entry with no CI summary — the shape most tests start from.
    fn cached(info: PrInfo) -> CachedPr {
        CachedPr { info, checks: None }
    }

    fn summary(pending: u32, failed: u32) -> ChecksSummary {
        ChecksSummary {
            total: pending + failed,
            pending,
            failed,
        }
    }

    #[test]
    fn get_returns_none_for_an_unknown_id() {
        let cache = PrPollCache::default();
        assert_eq!(cache.get("corral/hulling"), None);
    }

    #[test]
    fn insert_then_get_round_trips() {
        let cache = PrPollCache::default();
        cache.insert("corral/hulling", cached(sample()));
        assert_eq!(cache.get("corral/hulling"), Some(cached(sample())));
    }

    #[test]
    fn remove_clears_a_cached_entry() {
        let cache = PrPollCache::default();
        cache.insert("corral/hulling", cached(sample()));
        assert!(cache.remove("corral/hulling"));
        assert_eq!(cache.get("corral/hulling"), None);
    }

    #[test]
    fn apply_result_inserts_on_a_found_pr() {
        let cache = PrPollCache::default();
        let changed = apply_result(&cache, "corral/hulling", Ok(Some(sample())), None);
        assert!(changed);
        assert_eq!(cache.get("corral/hulling"), Some(cached(sample())));
    }

    #[test]
    fn apply_result_stores_the_checks_summary_for_an_open_pr() {
        let cache = PrPollCache::default();
        apply_result(
            &cache,
            "corral/hulling",
            Ok(Some(sample())),
            Some(summary(2, 1)),
        );
        assert_eq!(
            cache.get("corral/hulling").unwrap().checks,
            Some(summary(2, 1))
        );
    }

    #[test]
    fn apply_result_drops_the_checks_summary_for_a_merged_pr() {
        let cache = PrPollCache::default();
        let merged = PrInfo {
            state: PrState::Merged,
            ..sample()
        };
        apply_result(
            &cache,
            "corral/hulling",
            Ok(Some(merged)),
            Some(summary(2, 0)),
        );
        // A merged PR has no live checks, so a stale "CI running" must not
        // survive next to the merged badge.
        assert_eq!(cache.get("corral/hulling").unwrap().checks, None);
    }

    #[test]
    fn apply_result_reports_a_change_when_only_the_checks_summary_moved() {
        let cache = PrPollCache::default();
        cache.insert(
            "corral/hulling",
            CachedPr {
                info: sample(),
                checks: Some(summary(2, 0)),
            },
        );
        let changed = apply_result(
            &cache,
            "corral/hulling",
            Ok(Some(sample())),
            Some(summary(0, 0)),
        );
        assert!(
            changed,
            "a checks-only transition must still refresh the UI"
        );
    }

    #[test]
    fn apply_result_removes_on_no_pr_found() {
        let cache = PrPollCache::default();
        cache.insert("corral/hulling", cached(sample()));
        let changed = apply_result(&cache, "corral/hulling", Ok(None), None);
        assert!(changed);
        assert_eq!(cache.get("corral/hulling"), None);
    }

    #[test]
    fn apply_result_leaves_the_cache_untouched_on_a_transient_error() {
        let cache = PrPollCache::default();
        cache.insert("corral/hulling", cached(sample()));
        let changed = apply_result(
            &cache,
            "corral/hulling",
            Err(corral_core::error::Error::Gh {
                command: "gh pr view".to_string(),
                stderr: "rate limited".to_string(),
            }),
            None,
        );
        assert!(!changed);
        // Best-effort: a transient `gh` failure shouldn't clobber the last
        // known-good state with nothing.
        assert_eq!(cache.get("corral/hulling"), Some(cached(sample())));
    }

    #[test]
    fn retain_live_drops_entries_for_worktrees_that_are_gone() {
        let cache = PrPollCache::default();
        cache.insert("corral/alive", cached(sample()));
        cache.insert("corral/gone", cached(sample()));
        let live: HashSet<String> = ["corral/alive".to_string()].into_iter().collect();

        let dropped = cache.retain_live(&live);

        assert_eq!(dropped, 1);
        assert_eq!(cache.get("corral/alive"), Some(cached(sample())));
        assert_eq!(cache.get("corral/gone"), None);
    }

    #[test]
    fn apply_result_is_a_noop_when_nothing_was_cached_and_none_is_found() {
        let cache = PrPollCache::default();
        let changed = apply_result(&cache, "corral/hulling", Ok(None), None);
        assert!(!changed);
    }
}
