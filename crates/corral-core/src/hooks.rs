//! Claude Code hooks that let corral learn a session's real "waiting for
//! input" vs "working" state from Claude Code itself (`Stop`, `Notification`,
//! `UserPromptSubmit`) instead of guessing from PTY output timing.
//!
//! Hook commands report back over a per-instance Unix socket (see
//! [`socket_path`]) rather than talking to the Tauri backend's in-memory
//! state directly, since the hook runs as a detached subprocess of the
//! spawned `claude` process, not inside corral-app itself.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::Result;
use crate::json_store;

/// Env var, set on every spawned `claude` process, pointing at this
/// corral-app instance's hook callback socket.
pub const HOOK_SOCKET_ENV: &str = "CORRAL_HOOK_SOCKET";

struct HookSpec {
    event: &'static str,
    matcher: Option<&'static str>,
    arg: &'static str,
}

/// `Stop` fires the instant Claude finishes a turn — the earliest true
/// "waiting for input" signal, no timer needed. `corral notify-hook` reports
/// it as working instead when the hook input's `background_tasks` list is
/// non-empty (the turn paused with a shell/subagent still running). The
/// `Notification` hooks cover the cases where Claude is blocked mid-turn
/// (permission prompt) or has gone idle without the user noticing.
/// `UserPromptSubmit` fires the moment a new turn starts, flipping status
/// back to "working".
const HOOK_SPECS: &[HookSpec] = &[
    HookSpec {
        event: "Stop",
        matcher: None,
        arg: "waiting",
    },
    HookSpec {
        event: "Notification",
        matcher: Some("permission_prompt"),
        arg: "waiting",
    },
    HookSpec {
        event: "Notification",
        matcher: Some("idle_prompt"),
        arg: "waiting",
    },
    HookSpec {
        event: "Notification",
        matcher: Some("agent_needs_input"),
        arg: "waiting",
    },
    HookSpec {
        event: "UserPromptSubmit",
        matcher: None,
        arg: "working",
    },
];

fn hook_command(arg: &str) -> String {
    format!("corral notify-hook {arg}")
}

/// Merge corral's status-tracking hooks into a settings JSON document,
/// without disturbing any hooks the user already configured (checked by
/// exact `(event, matcher, command)` match, so re-running this is a no-op).
/// Returns whether the document was actually changed.
pub fn merge_hooks(doc: &mut Value) -> bool {
    if !doc.is_object() {
        *doc = json!({});
    }
    let mut changed = false;
    let hooks = doc
        .as_object_mut()
        .expect("just ensured doc is an object")
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let hooks = hooks
        .as_object_mut()
        .expect("just ensured hooks is an object");

    for spec in HOOK_SPECS {
        let command = hook_command(spec.arg);
        let entries = hooks
            .entry(spec.event.to_string())
            .or_insert_with(|| json!([]));
        if !entries.is_array() {
            *entries = json!([]);
        }
        let entries = entries
            .as_array_mut()
            .expect("just ensured entries is an array");

        let already_present = entries.iter().any(|group| {
            group.get("matcher").and_then(Value::as_str) == spec.matcher
                && group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|hs| {
                        hs.iter().any(|h| {
                            h.get("command").and_then(Value::as_str) == Some(command.as_str())
                        })
                    })
        });
        if already_present {
            continue;
        }

        let mut group = json!({
            "hooks": [{ "type": "command", "command": command }]
        });
        if let Some(m) = spec.matcher {
            group["matcher"] = json!(m);
        }
        entries.push(group);
        changed = true;
    }

    changed
}

/// Merge corral's hooks into the settings file at `settings_path`, creating
/// it (and its parent directory) if missing. A no-op write is skipped.
///
/// Deliberately not run under `json_store::with_locked_path` (unlike the
/// other stores): `merge_hooks` is idempotent — concurrent corral windows
/// all compute and write the *same* document — so there's no
/// read-modify-write race to guard, and a sibling `.lock` file in the user's
/// `.claude` dir would surface as untracked in their repo. Writes are still
/// atomic via [`json_store::save_pretty`]'s temp-file rename.
pub fn ensure_hooks_installed(settings_path: &Path) -> Result<()> {
    let mut doc: Value = match fs::read(settings_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!("hooks: failed to parse {settings_path:?}, treating as empty: {e}");
            json!({})
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.into()),
    };

    if merge_hooks(&mut doc) {
        if let Some(dir) = settings_path.parent() {
            fs::create_dir_all(dir)?;
        }
        json_store::save_pretty(settings_path, &doc)?;
    }
    Ok(())
}

/// Per-instance Unix socket path for hook callbacks, keyed by this
/// process's PID so multiple concurrently-running corral-app windows (per
/// CLAUDE.md: "Multiple corral windows/instances can be running at once")
/// don't collide.
pub fn socket_path() -> PathBuf {
    let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    dir.join(format!("corral-hooks-{}.sock", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_hooks_adds_all_specs_to_empty_doc() {
        let mut doc = json!({});
        let changed = merge_hooks(&mut doc);
        assert!(changed);

        let stop = doc["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], "corral notify-hook waiting");
        assert!(stop[0].get("matcher").is_none());

        let notif = doc["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(notif.len(), 3);
        let matchers: Vec<&str> = notif
            .iter()
            .map(|g| g["matcher"].as_str().unwrap())
            .collect();
        assert!(matchers.contains(&"permission_prompt"));
        assert!(matchers.contains(&"idle_prompt"));
        assert!(matchers.contains(&"agent_needs_input"));

        let submit = doc["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(
            submit[0]["hooks"][0]["command"],
            "corral notify-hook working"
        );
    }

    #[test]
    fn merge_hooks_is_idempotent() {
        let mut doc = json!({});
        assert!(merge_hooks(&mut doc));
        let after_first = doc.clone();
        let changed_again = merge_hooks(&mut doc);
        assert!(!changed_again);
        assert_eq!(doc, after_first);
    }

    #[test]
    fn merge_hooks_preserves_existing_user_hooks() {
        let mut doc = json!({
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo hi" }] }
                ]
            }
        });
        merge_hooks(&mut doc);
        assert_eq!(
            doc["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "echo hi"
        );
        assert!(doc["hooks"]["Stop"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn ensure_hooks_installed_creates_missing_file_and_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.local.json");
        ensure_hooks_installed(&path).unwrap();

        let doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(doc["hooks"]["Stop"].is_array());
    }

    #[test]
    fn ensure_hooks_installed_merges_into_existing_file_without_rewriting_unrelated_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.local.json");
        fs::write(&path, r#"{"permissions": {"allow": ["Bash"]}}"#).unwrap();

        ensure_hooks_installed(&path).unwrap();

        let doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["permissions"]["allow"][0], "Bash");
        assert!(doc["hooks"]["Stop"].is_array());
    }

    #[test]
    fn socket_path_is_stable_within_a_process() {
        assert_eq!(socket_path(), socket_path());
        assert!(
            socket_path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(&std::process::id().to_string())
        );
    }
}
