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

/// Which executable to bake into Claude Code's hook commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookCli<'a> {
    /// The absolute path of the CLI bundled with this app install. Hooks may
    /// run from a shell the app didn't spawn, and macOS never puts the app
    /// bundle's `Contents/MacOS` on `PATH`, so a name lookup isn't reliable.
    Absolute(&'a Path),
    /// The bare `corral` name, resolved from the session PTY's `PATH` (see
    /// `bin::hook_cli_path` for when this is the only option — an AppImage,
    /// whose mount path changes on every launch).
    Name,
    /// Don't install corral's hooks at all — remove any it installed, leaving
    /// the user's own untouched, and drop the groups that leaves empty.
    Remove,
}

/// The shell command Claude Code runs for one hook event, or `None` when the
/// hook should be removed ([`HookCli::Remove`]).
fn hook_command(arg: &str, cli: HookCli<'_>) -> Option<String> {
    match cli {
        HookCli::Absolute(cli) => Some(format!(
            "{} notify-hook {arg}",
            shell_quote(&cli.to_string_lossy())
        )),
        HookCli::Name => Some(format!("corral notify-hook {arg}")),
        HookCli::Remove => None,
    }
}

/// Quote one shell word with single quotes, escaping embedded apostrophes
/// (`'` → `'\''`). App paths can contain spaces, and a user's Applications
/// folder can contain an apostrophe.
///
/// Only apostrophes need escaping: inside single quotes every other shell
/// metacharacter is literal. Two characters can't be repaired this way and are
/// the caller's problem — a newline makes Claude Code treat the command as a
/// *path* rather than a shell command, and a Windows `\` separator is a shell
/// escape on Windows' own shell. `bin::hook_cli_path` is the only caller, and
/// a `current_exe` would have to be equally broken for any hook to work at all.
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

/// Whether a hook command is corral's status reporter, whatever CLI path it
/// was written with — so a stale entry is replaced after an app move or a
/// switch between a bundled and a dev build, instead of accumulating another.
///
/// The executable name must be exactly `corral` (or `corral.exe`), not merely
/// *start* with it: a stem comparison would also claim a user's own
/// `corral-rc` or `corral.dev`, and then append a duplicate next to it.
fn is_corral_hook_command(command: &str) -> bool {
    let words = shell_words(command);
    words.len() >= 2
        && Path::new(&words[0])
            .file_name()
            .is_some_and(|name| name == CLI_FILE_NAME)
        && words[1] == "notify-hook"
}

/// Whether `command` is the corral hook for `spec` specifically — same
/// executable, same `notify-hook` verb, and the same status (`waiting` vs
/// `working`, which is what stops `Stop` and `UserPromptSubmit` from being
/// mistaken for one another).
fn is_corral_hook_for(command: &str, spec: &HookSpec) -> bool {
    is_corral_hook_command(command)
        && shell_words(command).last().map(String::as_str) == Some(spec.arg)
}

/// The CLI's file name as it appears in a hook command.
const CLI_FILE_NAME: &str = if cfg!(windows) {
    "corral.exe"
} else {
    "corral"
};

/// Whether `group` is this spec's exact slot in its event array.
///
/// The matcher is what separates specs that share an event and a status verb
/// — all three `Notification` specs report `waiting` — so it is compared
/// exactly, missing matcher included.
fn group_is_exact_slot(group: &Value, spec: &HookSpec) -> bool {
    group.get("matcher").and_then(Value::as_str) == spec.matcher
}

/// Whether more than one spec shares `spec`'s event (the `Notification`s do),
/// which is why a matcher-less group there can't be attributed to a slot.
fn event_has_several_slots(spec: &HookSpec) -> bool {
    HOOK_SPECS
        .iter()
        .filter(|other| other.event == spec.event)
        .count()
        > 1
}

/// Where corral's hook for `spec` could be: this slot's exact group, plus —
/// only when the event has a single slot, so nothing can be confused with it —
/// a matcher-less group corral may have written it into (an entry from before
/// matchers were compared this way, or one whose matcher a user stripped).
///
/// Used to decide whether a slot is already settled. Rewrites never go through
/// this: see [`group_is_exact_slot`] and [`remove_managed_from_slot`], so two
/// specs sharing an event can't each delete what the other just wrote.
fn group_belongs_to_slot(group: &Value, spec: &HookSpec) -> bool {
    group_is_exact_slot(group, spec)
        || (!event_has_several_slots(spec)
            && group.get("matcher").and_then(Value::as_str).is_none())
}

/// Remove corral's status hook from `spec`'s own group and drop the groups that
/// leaves empty. Returns whether anything changed.
///
/// Matcher-less groups are deliberately skipped: see `group_is_exact_slot`.
fn remove_managed_from_slot(entries: &mut Vec<Value>, spec: &HookSpec) -> bool {
    let mut changed = false;
    let mut emptied = Vec::new();
    for (index, group) in entries.iter_mut().enumerate() {
        if !group_is_exact_slot(group, spec) {
            continue;
        }
        let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        let before = hooks.len();
        hooks.retain(|hook| {
            !hook
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(is_corral_hook_command)
        });
        if hooks.len() != before {
            changed = true;
            if hooks.is_empty() {
                // Corral's own group, now empty: drop it. A group the user
                // built that merely also held one of corral's hooks keeps
                // whatever it still holds.
                emptied.push(index);
            }
        }
    }
    for index in emptied.into_iter().rev() {
        entries.remove(index);
    }
    changed
}

/// Merge corral's status-tracking hooks into a settings JSON document,
/// without disturbing any hooks the user already configured.
///
/// Corral-managed entries are identified by their command (`<...>/corral
/// notify-hook …`) rather than by exact string, so an app that moved or
/// switched between dev and bundled builds replaces its stale entry instead of
/// accumulating another one. Re-running this is a no-op once every slot holds
/// its canonical command and nothing else. Returns whether the document
/// changed.
///
/// Must stay idempotent: it is not run under a lock (see
/// [`ensure_hooks_installed`]) and every Claude session start calls it, so a
/// pass that "fixed" an already-canonical document would rewrite the user's
/// `settings.local.json` on every start and race other corral windows.
///
/// [`HookCli::Remove`] takes a separate, shorter path: it strips corral's
/// groups (and any event array that leaves empty), leaving the user's own
/// untouched. `start_session` uses it when a project stops using Claude, so the
/// repo's settings don't keep a stale absolute path for a harness that never
/// reads it.
pub fn merge_hooks(doc: &mut Value, cli: HookCli<'_>) -> bool {
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
    // Entries that aren't arrays (corral never writes one) are replaced below
    // rather than tripping the `expect`s in the loop.
    for entries in hooks.values_mut() {
        if !entries.is_array() {
            *entries = json!([]);
        }
    }

    // Removal is not a merge: strip corral's hooks using each spec's own slot,
    // drop the events that leaves empty, and stop. Going through the install
    // loop instead would create the very slots it then has to delete, and
    // report a change on every later call.
    if cli == HookCli::Remove {
        let mut changed = false;
        for spec in HOOK_SPECS {
            if let Some(entries) = hooks.get_mut(spec.event).and_then(Value::as_array_mut) {
                changed |= remove_managed_from_slot(entries, spec);
            }
        }
        let mut emptied = false;
        hooks.retain(|_, entries| {
            let empty = entries.as_array().is_some_and(|entries| entries.is_empty());
            emptied |= empty;
            !empty
        });
        return changed || emptied;
    }

    for spec in HOOK_SPECS {
        // Infallible here: the `HookCli::Remove` case returned above.
        let Some(desired) = hook_command(spec.arg, cli) else {
            continue;
        };
        let entries = hooks
            .entry(spec.event.to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("just ensured every entry is an array");

        // Already canonical: corral's hook for this slot (or, for an event with
        // a single slot, a matcher-less group corral may have written) is the
        // current command and nothing else of corral's is left in it. The
        // common case on every session start.
        let managed: Vec<String> = entries
            .iter()
            .filter(|group| group_belongs_to_slot(group, spec))
            .filter_map(|group| group.get("hooks").and_then(Value::as_array))
            .flatten()
            .filter_map(|hook| hook.get("command").and_then(Value::as_str))
            .filter(|command| is_corral_hook_for(command, spec))
            .map(str::to_string)
            .collect();
        if matches!(managed.as_slice(), [only] if only == &desired) {
            continue;
        }

        // Rewrite only this slot's own group, so specs sharing an event can't
        // each wipe what the other just wrote.
        remove_managed_from_slot(entries, spec);

        let mut group = json!({
            "hooks": [{ "type": "command", "command": desired }]
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
/// `cli` says what to bake into the hook commands — an absolute path, the bare
/// name, or nothing to install and everything of corral's removed (see
/// [`HookCli`] and [`hook_command`]).
///
/// Deliberately not run under `json_store::with_locked_path` (unlike the
/// other stores): `merge_hooks` is idempotent — concurrent corral windows
/// all compute and write the *same* document — so there's no
/// read-modify-write race to guard, and a sibling `.lock` file in the user's
/// `.claude` dir would surface as untracked in their repo. Writes are still
/// atomic via [`json_store::save_pretty`]'s temp-file rename.
pub fn ensure_hooks_installed(settings_path: &Path, cli: HookCli<'_>) -> Result<()> {
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
        let changed = merge_hooks(&mut doc, HookCli::Name);
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

    /// Every command in an event array with the corral ones normalized, so two
    /// documents can be compared regardless of ordering.
    fn corral_commands(doc: &Value, event: &str) -> Vec<String> {
        doc["hooks"][event]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["hooks"].as_array().unwrap())
            .filter_map(|hook| hook["command"].as_str())
            .filter(|command| is_corral_hook_command(command))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn merge_hooks_replaces_a_stale_matcherless_group_without_claiming_its_siblings() {
        // A `Notification` group whose matcher was stripped belongs to
        // whichever `Notification` spec runs first (`permission_prompt`). The
        // replacement must not take the *other* slots' hooks down with it —
        // that was a real bug: the scrub ran over every group in the array.
        let mut doc = json!({});
        merge_hooks(&mut doc, HookCli::Name);
        let mut stale = doc["hooks"]["Notification"][2].clone();
        stale.as_object_mut().unwrap().remove("matcher");
        stale["hooks"][0]["command"] =
            json!("'/old/Corral.app/Contents/MacOS/corral' notify-hook waiting");
        doc["hooks"]["Notification"]
            .as_array_mut()
            .unwrap()
            .push(stale);

        assert!(merge_hooks(
            &mut doc,
            HookCli::Absolute(Path::new("/app/corral"))
        ));

        let notifications = doc["hooks"]["Notification"].as_array().unwrap();
        let matchers: Vec<&str> = notifications
            .iter()
            .filter_map(|g| g["matcher"].as_str())
            .collect();
        assert_eq!(
            matchers.len(),
            3,
            "each Notification slot must keep its hook: {notifications:?}"
        );
        assert!(matchers.contains(&"permission_prompt"));
        assert!(matchers.contains(&"idle_prompt"));
        assert!(matchers.contains(&"agent_needs_input"));
        assert!(
            notifications
                .iter()
                .all(|g| !g["hooks"].as_array().unwrap().is_empty()),
            "no group may be left empty: {notifications:?}"
        );
    }

    #[test]
    fn merge_hooks_never_deletes_another_slots_hook_when_rewriting_an_event() {
        // Rewriting `Notification` must not delete the `permission_prompt` and
        // `agent_needs_input` hooks written earlier in the same pass just
        // because they also hold nothing but corral commands.
        let mut doc = json!({});
        merge_hooks(&mut doc, HookCli::Name);
        assert!(merge_hooks(
            &mut doc,
            HookCli::Absolute(Path::new("/app/corral"))
        ));
        assert_eq!(
            corral_commands(&doc, "Notification"),
            vec!["'/app/corral' notify-hook waiting"; 3]
        );

        // Same guarantee when the array started out empty for that event.
        let mut fresh = json!({ "hooks": { "Notification": [] } });
        merge_hooks(&mut fresh, HookCli::Name);
        assert_eq!(corral_commands(&fresh, "Notification").len(), 3);
    }

    #[test]
    fn is_corral_hook_command_requires_the_exact_cli_name() {
        // A user's own `corral-rc` / `corral.dev` must not be adopted, or the
        // merge appends a duplicate next to it.
        assert!(!is_corral_hook_command("corral-rc notify-hook waiting"));
        assert!(!is_corral_hook_command(
            "/usr/local/bin/corral.dev notify-hook waiting"
        ));
        assert!(!is_corral_hook_command("mycorral notify-hook waiting"));
        assert!(is_corral_hook_command("corral notify-hook waiting"));
        assert!(is_corral_hook_command("/opt/corral notify-hook working"));
        assert!(is_corral_hook_command("'/a b/corral' notify-hook waiting"));

        let mut doc = json!({
            "hooks": { "Stop": [{ "hooks": [
                { "type": "command", "command": "corral-rc notify-hook waiting" }
            ] }] }
        });
        merge_hooks(&mut doc, HookCli::Absolute(Path::new("/app/corral")));
        let commands: Vec<&str> = doc["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert!(
            commands.contains(&"corral-rc notify-hook waiting"),
            "a user's lookalike command must survive: {commands:?}"
        );
    }

    #[test]
    fn merge_hooks_bakes_in_a_shell_quoted_absolute_cli_path() {
        let cli = Path::new("/Applications/My Corral.app/Contents/MacOS/corral");
        let mut doc = json!({});
        assert!(merge_hooks(&mut doc, HookCli::Absolute(cli)));

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
        assert!(merge_hooks(&mut doc, HookCli::Absolute(cli)));
        let after_first = doc.clone();
        let changed_again = merge_hooks(&mut doc, HookCli::Absolute(cli));
        assert!(!changed_again);
        assert_eq!(doc, after_first);
    }

    #[test]
    fn merge_hooks_replaces_a_stale_cli_path_instead_of_accumulating() {
        let mut doc = json!({});
        assert!(merge_hooks(&mut doc, HookCli::Name));

        let moved = Path::new("/Applications/Corral.app/Contents/MacOS/corral");
        assert!(merge_hooks(&mut doc, HookCli::Absolute(moved)));

        let stop = doc["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1, "stale group should be dropped");
        assert_eq!(stop[0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(
            stop[0]["hooks"][0]["command"],
            "'/Applications/Corral.app/Contents/MacOS/corral' notify-hook waiting"
        );
    }

    #[test]
    fn merge_hooks_keeps_a_user_hook_under_an_unexpected_matcher() {
        // A `Notification` group whose matcher is a list (not a string) isn't
        // one corral wrote and isn't this slot's, so its user hook survives and
        // the stale corral entry beside it is still replaced.
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

        assert!(merge_hooks(
            &mut doc,
            HookCli::Absolute(Path::new("/app/corral"))
        ));

        let notifications = doc["hooks"]["Notification"].as_array().unwrap();
        let commands: Vec<&str> = notifications
            .iter()
            .flat_map(|group| group["hooks"].as_array().unwrap())
            .map(|hook| hook["command"].as_str().unwrap())
            .collect();
        assert!(commands.contains(&"echo unrelated"));
        let matchers: Vec<&str> = notifications
            .iter()
            .filter_map(|g| g["matcher"].as_str())
            .collect();
        assert_eq!(
            matchers,
            vec!["permission_prompt", "idle_prompt", "agent_needs_input"]
        );
        assert!(
            notifications
                .iter()
                .filter(|g| g.get("matcher").and_then(Value::as_str).is_some())
                .flat_map(|g| g["hooks"].as_array().unwrap())
                .all(|hook| hook["command"] == "'/app/corral' notify-hook waiting"),
            "each slot must hold the canonical command: {notifications:?}"
        );
    }

    #[test]
    fn merge_hooks_replaces_each_shared_event_slot_independently() {
        // All three `Notification` specs share an event *and* report
        // "waiting", so a slot has to be identified by its matcher. Re-running
        // with a new CLI must update each one in place, leaving three groups.
        let mut doc = json!({});
        assert!(merge_hooks(&mut doc, HookCli::Name));

        let moved = Path::new("/Applications/Corral.app/Contents/MacOS/corral");
        assert!(merge_hooks(&mut doc, HookCli::Absolute(moved)));

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
        merge_hooks(&mut doc, HookCli::Name);
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

        merge_hooks(&mut doc, HookCli::Absolute(Path::new("/app/corral")));

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
    fn merge_hooks_tolerates_events_that_are_not_arrays() {
        // A hand-edited (or older) settings file can hold `"Stop": {}` where
        // corral expects an array; that must be replaced, not panic on.
        let mut doc = json!({
            "hooks": {
                "Stop": { "hooks": [] },
                "Notification": "nope",
                "UserPromptSubmit": null
            }
        });
        assert!(merge_hooks(&mut doc, HookCli::Name));
        assert_eq!(doc["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(doc["hooks"]["Notification"].as_array().unwrap().len(), 3);
        assert_eq!(
            doc["hooks"]["UserPromptSubmit"].as_array().unwrap().len(),
            1
        );
        assert!(!merge_hooks(&mut doc, HookCli::Name));
    }

    #[test]
    fn hook_cli_remove_strips_corral_hooks_and_leaves_the_users() {
        // What the merge into a `.claude/settings.local.json` that a project no
        // longer uses as a Claude project should leave behind: no corral
        // command, no empty groups, and the user's hooks untouched.
        let mut doc = json!({
            "permissions": { "allow": ["Bash"] },
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{ "type": "command", "command": "/tmp/guard.sh" }]
                }],
                "Stop": [{
                    "hooks": [
                        { "type": "command", "command": "echo done" },
                        { "type": "command", "command": "corral notify-hook waiting" }
                    ]
                }]
            }
        });
        merge_hooks(&mut doc, HookCli::Absolute(Path::new("/app/corral")));
        assert!(merge_hooks(&mut doc, HookCli::Remove));

        assert_eq!(doc["permissions"]["allow"][0], "Bash");
        assert_eq!(
            doc["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "/tmp/guard.sh"
        );
        let stop = doc["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1, "corral's own group is gone: {stop:?}");
        assert_eq!(stop[0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], "echo done");
        assert!(
            doc["hooks"]["Notification"].is_null(),
            "no empty event arrays left behind: {doc}"
        );

        // Removing again is a no-op, and re-enabling puts the hooks back.
        assert!(!merge_hooks(&mut doc, HookCli::Remove));
        assert!(merge_hooks(&mut doc, HookCli::Name));
        assert_eq!(doc["hooks"]["Notification"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn ensure_hooks_installed_creates_missing_file_and_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.local.json");
        ensure_hooks_installed(&path, HookCli::Name).unwrap();

        let doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(doc["hooks"]["Stop"].is_array());
    }

    #[test]
    fn ensure_hooks_installed_merges_into_existing_file_without_rewriting_unrelated_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.local.json");
        fs::write(&path, r#"{"permissions": {"allow": ["Bash"]}}"#).unwrap();

        ensure_hooks_installed(&path, HookCli::Absolute(Path::new("/app/corral"))).unwrap();

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
