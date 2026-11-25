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

/// The shell command Claude Code runs for one hook event.
///
/// On a normal install `cli` is an absolute path to the bundled CLI — on macOS
/// the app bundle's `Contents/MacOS` is never on `PATH`, and the hook may run
/// from a shell the app didn't spawn, so resolving by name isn't reliable.
/// `None` keeps the bare `corral` name (AppImage: its mount path changes on
/// every launch; see `bin::hook_cli_path`).
fn hook_command(arg: &str, cli: Option<&Path>) -> String {
    match cli {
        Some(cli) => format!("{} notify-hook {arg}", shell_quote(&cli.to_string_lossy())),
        None => format!("corral notify-hook {arg}"),
    }
}

/// Quote one shell word with single quotes, escaping embedded apostrophes
/// (`'` → `'\''`). App paths can contain spaces, and a user's Applications
/// folder can contain an apostrophe.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Split a hook command into shell words, understanding single quotes, double
/// quotes, and backslash escapes. Deliberately does not expand variables or
/// globs — it only needs to find the executable name.
fn shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = command.trim_start().chars();

    while let Some(ch) = chars.next() {
        match ch {
            '\'' | '"' => {
                in_word = true;
                for quoted in chars.by_ref() {
                    if quoted == ch {
                        break;
                    }
                    current.push(quoted);
                }
            }
            '\\' => {
                in_word = true;
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            ch if ch.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            ch => {
                in_word = true;
                current.push(ch);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// Whether a hook command is one corral installed (any cli path, `waiting` or
/// `working`), so stale entries can be replaced after an app move or when
/// switching between a bundled and a dev build.
fn is_corral_hook_command(command: &str) -> bool {
    let words = shell_words(command);
    words.len() >= 2
        && Path::new(&words[0])
            .file_stem()
            .is_some_and(|name| name == "corral")
        && words[1] == "notify-hook"
}

/// Whether a group has no reason to exist once corral's hooks are removed:
/// its `hooks` are empty, or every remaining hook is one corral installed
/// (from any slot). Groups still holding a user hook are kept.
fn group_is_only_managed(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().all(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(is_corral_hook_command)
            })
        })
}

fn matcher_matches(group: &Value, matcher: Option<&str>) -> bool {
    group.get("matcher").and_then(Value::as_str) == matcher
}

/// Whether a group belongs to `spec`'s slot within its event array.
///
/// The matcher is what separates specs that share an event and a status verb
/// — all three `Notification` specs report `waiting`. A group with *no*
/// matcher is also treated as belonging to any slot: an entry corral wrote
/// before matchers were compared this way (or one a user stripped) would
/// otherwise look foreign and get a duplicate appended next to it.
fn group_belongs_to(group: &Value, spec: &HookSpec) -> bool {
    matcher_matches(group, spec.matcher) || group.get("matcher").and_then(Value::as_str).is_none()
}

/// Every corral-managed command in `spec`'s slot, in any group.
fn managed_commands_for(entries: &[Value], spec: &HookSpec) -> Vec<String> {
    entries
        .iter()
        .filter(|group| group_belongs_to(group, spec))
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter_map(|hook| hook.get("command").and_then(Value::as_str))
        .filter(|command| is_corral_hook_command(command))
        .map(str::to_string)
        .collect()
}

/// Merge corral's status-tracking hooks into a settings JSON document,
/// without disturbing any hooks the user already configured.
///
/// Corral-managed entries are identified by their command (`<...>/corral
/// notify-hook …`) rather than by exact string, so an app that moved or
/// switched between dev and bundled builds replaces its stale entry instead
/// of accumulating another one. Re-running this is a no-op once the canonical
/// command is the only managed entry. Returns whether the document changed.
pub fn merge_hooks(doc: &mut Value, cli: Option<&Path>) -> bool {
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
        let command = hook_command(spec.arg, cli);
        let entries = hooks
            .entry(spec.event.to_string())
            .or_insert_with(|| json!([]));
        if !entries.is_array() {
            *entries = json!([]);
        }
        let entries = entries
            .as_array_mut()
            .expect("just ensured entries is an array");

        // Scoped to this spec's slot (see `group_belongs_to`), so specs that
        // share an event and verb — all three `Notification` entries report
        // `waiting` — never remove each other's hooks.
        let managed = managed_commands_for(entries, spec);

        // Already in the exact desired state (one managed entry, pointing at
        // the right CLI): leave the document alone.
        if matches!(managed.as_slice(), [only] if only == &command) {
            continue;
        }

        if !managed.is_empty() {
            // Every managed command in this slot is replaced by the canonical
            // one below; drop them from every group they appear in.
            for group in entries.iter_mut() {
                if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    hooks.retain(|hook| {
                        hook.get("command")
                            .and_then(Value::as_str)
                            .is_none_or(|command| !managed.iter().any(|m| m == command))
                    });
                }
            }
            // Drop groups left holding only corral hooks (from any slot).
            entries.retain(|group| !group_is_only_managed(group));
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
/// `cli` is the executable to bake into the hook commands (see
/// [`hook_command`]); `None` keeps the bare `corral` name.
///
/// Deliberately not run under `json_store::with_locked_path` (unlike the
/// other stores): `merge_hooks` is idempotent — concurrent corral windows
/// all compute and write the *same* document — so there's no
/// read-modify-write race to guard, and a sibling `.lock` file in the user's
/// `.claude` dir would surface as untracked in their repo. Writes are still
/// atomic via [`json_store::save_pretty`]'s temp-file rename.
pub fn ensure_hooks_installed(settings_path: &Path, cli: Option<&Path>) -> Result<()> {
    let mut doc: Value = match fs::read(settings_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!("hooks: failed to parse {settings_path:?}, treating as empty: {e}");
            json!({})
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.into()),
    };

    if merge_hooks(&mut doc, cli) {
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
        let changed = merge_hooks(&mut doc, None);
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
    fn merge_hooks_bakes_in_a_shell_quoted_absolute_cli_path() {
        let cli = Path::new("/Applications/My Corral.app/Contents/MacOS/corral");
        let mut doc = json!({});
        assert!(merge_hooks(&mut doc, Some(cli)));

        assert_eq!(
            doc["hooks"]["Stop"][0]["hooks"][0]["command"],
            "'/Applications/My Corral.app/Contents/MacOS/corral' notify-hook waiting"
        );
        assert_eq!(
            doc["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"],
            "'/Applications/My Corral.app/Contents/MacOS/corral' notify-hook working"
        );
    }

    #[test]
    fn shell_quote_escapes_embedded_apostrophes() {
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn is_corral_hook_command_recognises_managed_commands() {
        assert!(is_corral_hook_command("corral notify-hook waiting"));
        assert!(is_corral_hook_command(
            "'/Applications/My Corral.app/Contents/MacOS/corral' notify-hook working"
        ));
        assert!(is_corral_hook_command(
            r"'/Users/me/it'\''s/corral' notify-hook waiting"
        ));
        assert!(!is_corral_hook_command("echo corral notify-hook waiting"));
        assert!(!is_corral_hook_command(
            "/usr/bin/other notify-hook waiting"
        ));
        assert!(!is_corral_hook_command("corral list"));
    }

    #[test]
    fn merge_hooks_is_idempotent() {
        let mut doc = json!({});
        let cli = Path::new("/Applications/Corral.app/Contents/MacOS/corral");
        assert!(merge_hooks(&mut doc, Some(cli)));
        let after_first = doc.clone();
        let changed_again = merge_hooks(&mut doc, Some(cli));
        assert!(!changed_again);
        assert_eq!(doc, after_first);
    }

    #[test]
    fn merge_hooks_replaces_a_stale_cli_path_instead_of_accumulating() {
        let mut doc = json!({});
        assert!(merge_hooks(&mut doc, None));

        let moved = Path::new("/Applications/Corral.app/Contents/MacOS/corral");
        assert!(merge_hooks(&mut doc, Some(moved)));

        let stop = doc["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1, "stale group should be dropped");
        assert_eq!(stop[0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(
            stop[0]["hooks"][0]["command"],
            "'/Applications/Corral.app/Contents/MacOS/corral' notify-hook waiting"
        );
    }

    #[test]
    fn merge_hooks_replaces_a_managed_hook_under_an_unexpected_matcher() {
        // Neither of these groups matches the spec's matcher exactly, so an
        // identity check that looked at matchers would treat them as foreign
        // and append a third entry instead of consolidating.
        let mut doc = json!({
            "hooks": {
                "Notification": [
                    {
                        "matcher": ["permission_prompt"],
                        "hooks": [{ "type": "command", "command": "corral notify-hook waiting" }]
                    },
                    {
                        "hooks": [{ "type": "command", "command": "echo unrelated" }]
                    }
                ]
            }
        });

        assert!(merge_hooks(&mut doc, Some(Path::new("/app/corral"))));

        let notifications = doc["hooks"]["Notification"].as_array().unwrap();
        let commands: Vec<&str> = notifications
            .iter()
            .flat_map(|group| group["hooks"].as_array().unwrap())
            .map(|hook| hook["command"].as_str().unwrap())
            .collect();
        assert!(!commands.contains(&"corral notify-hook waiting"));
        assert!(commands.contains(&"echo unrelated"));
        let spawned: Vec<&str> = notifications
            .iter()
            .filter(|g| g.get("matcher").and_then(Value::as_str).is_some())
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .map(|hook| hook["command"].as_str().unwrap())
            .collect();
        assert!(
            spawned
                .iter()
                .all(|c| *c == "'/app/corral' notify-hook waiting"),
            "stale entries must be replaced by the canonical command: {spawned:?}"
        );
    }

    #[test]
    fn merge_hooks_replaces_each_shared_event_slot_independently() {
        // All three `Notification` specs share an event *and* report
        // "waiting", so a slot has to be identified by its matcher. Re-running
        // with a new CLI must update each one in place, leaving three groups.
        let mut doc = json!({});
        assert!(merge_hooks(&mut doc, None));

        let moved = Path::new("/Applications/Corral.app/Contents/MacOS/corral");
        assert!(merge_hooks(&mut doc, Some(moved)));

        let notifications = doc["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(notifications.len(), 3);
        for group in notifications {
            let hooks = group["hooks"].as_array().unwrap();
            assert_eq!(hooks.len(), 1, "each slot keeps exactly one hook");
            assert_eq!(
                hooks[0]["command"],
                "'/Applications/Corral.app/Contents/MacOS/corral' notify-hook waiting"
            );
        }
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
        merge_hooks(&mut doc, None);
        assert_eq!(
            doc["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "echo hi"
        );
        assert!(doc["hooks"]["Stop"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn merge_hooks_keeps_a_users_hook_in_a_group_it_also_manages() {
        let mut doc = json!({
            "hooks": {
                "Stop": [
                    {
                        "hooks": [
                            { "type": "command", "command": "echo done" },
                            { "type": "command", "command": "corral notify-hook waiting" }
                        ]
                    }
                ]
            }
        });

        merge_hooks(&mut doc, Some(Path::new("/app/corral")));

        let stop = doc["hooks"]["Stop"].as_array().unwrap();
        let commands: Vec<&str> = stop
            .iter()
            .flat_map(|group| group["hooks"].as_array().unwrap())
            .map(|hook| hook["command"].as_str().unwrap())
            .collect();
        assert!(commands.contains(&"echo done"));
        assert!(commands.contains(&"'/app/corral' notify-hook waiting"));
        assert!(!commands.contains(&"corral notify-hook waiting"));
    }

    #[test]
    fn ensure_hooks_installed_creates_missing_file_and_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.local.json");
        ensure_hooks_installed(&path, None).unwrap();

        let doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(doc["hooks"]["Stop"].is_array());
    }

    #[test]
    fn ensure_hooks_installed_merges_into_existing_file_without_rewriting_unrelated_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.local.json");
        fs::write(&path, r#"{"permissions": {"allow": ["Bash"]}}"#).unwrap();

        ensure_hooks_installed(&path, Some(Path::new("/app/corral"))).unwrap();

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
