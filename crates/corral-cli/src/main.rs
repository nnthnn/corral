//! `corral` — command-line companion for managing Claude Code worktree sessions.

mod awake;

use std::process::Command;

use anyhow::{Context, Result};
use clap::builder::styling::{AnsiColor, Styles};
use clap::{Parser, Subcommand};
use corral_core::worktree::{self, Worktree};
use serde_json::Value;

/// `<crate version> (<short git sha>)`, e.g. `0.1.0 (a1b2c3d)`. The sha comes
/// from `build.rs`, which shells out to `git rev-parse --short HEAD` at
/// compile time and falls back to `"unknown"` outside a git checkout.
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("CORRAL_GIT_SHA"), ")");

/// Bold section headers + green usage/literals, applied to both the
/// top-level `--help` and every subcommand's `--help`/`-h`.
fn styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default().bold())
        .usage(AnsiColor::Yellow.on_default().bold())
        .literal(AnsiColor::Green.on_default().bold())
        .placeholder(AnsiColor::Cyan.on_default())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default())
        .error(AnsiColor::Red.on_default().bold())
}

#[derive(Parser)]
#[command(
    name = "corral",
    version = VERSION,
    about = "Manage parallel Claude Code sessions in isolated git worktrees",
    styles = styles()
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a worktree (branch feat/<feature>) and launch claude inside it.
    /// Omit `feature` to get an auto-picked placeholder name.
    New {
        project: String,
        feature: Option<String>,
        /// Create the worktree but don't launch claude.
        #[arg(long)]
        no_agent: bool,
    },
    /// List all active worktrees.
    List,
    /// Open a worktree in Zed.
    Open {
        feature: String,
        /// Disambiguate when the same feature exists in multiple projects.
        #[arg(long)]
        project: Option<String>,
    },
    /// Remove a worktree and delete its branch.
    Done {
        feature: String,
        /// Disambiguate when the same feature exists in multiple projects.
        #[arg(long)]
        project: Option<String>,
        /// Keep the feat/<feature> branch after removing the worktree.
        #[arg(long)]
        keep_branch: bool,
    },
    /// Rename a worktree: pass a human-readable name (e.g. "Fix Login
    /// Timeout") and corral derives the kebab-case branch/folder slug from
    /// it automatically. Its physical directory and any running session are
    /// unaffected. Defaults to the worktree containing the current
    /// directory, so an agent running in its own worktree can just run
    /// `corral rename "<name>"`.
    Rename {
        /// Human-readable name; corral slugifies it for the branch and
        /// folder alias.
        new_name: String,
        /// Worktree to rename; defaults to the one containing `cwd`.
        #[arg(long)]
        feature: Option<String>,
        /// Disambiguate when the same feature exists in multiple projects.
        #[arg(long)]
        project: Option<String>,
    },
    /// View or change where new worktrees are created. Note: switching this
    /// does not move worktrees already created under the old root — if you
    /// have existing worktrees under ~/source/worktrees, `set` this to that
    /// path to keep using them.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Register an existing local git repo as a project corral tracks
    /// (equivalent to the GUI's Add Project folder picker).
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Run a command while holding a system sleep/idle inhibitor, so
    /// long-running background work (scans, test suites, dev servers,
    /// watchers) can't let the machine suspend. Linux uses
    /// `systemd-inhibit`; macOS uses `caffeinate`; Windows uses
    /// `SetThreadExecutionState`. Wrap the long-running command itself, not
    /// a launcher that exits immediately.
    Awake {
        /// Short reason shown by the platform inhibitor (Linux only).
        #[arg(long, default_value = "agent background work")]
        why: String,
        /// Command and arguments to run under the inhibitor. Use `--` before
        /// a command whose own flags start with `-`.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Internal: invoked by Claude Code hooks (Stop / Notification /
    /// UserPromptSubmit) installed into a project's shared
    /// `.claude/settings.local.json` by the Tauri app. Reads the hook's
    /// stdin JSON to resolve which worktree fired it, then forwards a
    /// status update to that corral-app instance over its hook socket.
    /// Always exits 0 — a nonzero exit here must never block Claude Code.
    NotifyHook { event: HookEvent },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print the resolved worktrees root and whether it's the default.
    Get,
    /// Set the worktrees root (absolute path, or `~`-prefixed). Existing
    /// worktrees under the previous root are not moved.
    Set { path: String },
    /// Clear the override, reverting to the default (~/corral).
    Reset,
}

#[derive(Subcommand)]
enum ProjectAction {
    /// Register the git repo at `path`, deriving its name from the
    /// directory's basename. Errors if `path` isn't a git repo or a
    /// project with that name is already registered.
    Add { path: String },
}

#[derive(Clone, clap::ValueEnum)]
enum HookEvent {
    Waiting,
    Working,
}

impl HookEvent {
    fn as_str(&self) -> &'static str {
        match self {
            HookEvent::Waiting => "waiting",
            HookEvent::Working => "working",
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("corral_cli=info,corral_core=info")
            }),
        )
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("corral cli starting");

    let cli = Cli::parse();
    match cli.command {
        Commands::New {
            project,
            feature,
            no_agent,
        } => cmd_new(&project, feature.as_deref(), no_agent),
        Commands::List => cmd_list(),
        Commands::Open { feature, project } => cmd_open(&feature, project.as_deref()),
        Commands::Done {
            feature,
            project,
            keep_branch,
        } => cmd_done(&feature, project.as_deref(), keep_branch),
        Commands::Rename {
            new_name,
            feature,
            project,
        } => cmd_rename(&new_name, feature.as_deref(), project.as_deref()),
        Commands::Config { action } => cmd_config(action),
        Commands::Project { action } => cmd_project(action),
        Commands::Awake { why, command } => awake::run(&why, &command),
        Commands::NotifyHook { event } => {
            cmd_notify_hook(&event);
            Ok(())
        }
    }
}

fn cmd_new(project: &str, feature: Option<&str>, no_agent: bool) -> Result<()> {
    let generated;
    let feature = match feature {
        Some(f) => f,
        None => {
            generated = corral_core::names::generate(project).context("picking a feature name")?;
            println!("picked feature name: {generated}");
            &generated
        }
    };
    let wt = worktree::create(project, feature)
        .with_context(|| format!("creating worktree {project}/{feature}"))?;
    println!("→ {}", wt.path.display());

    if no_agent {
        return Ok(());
    }

    println!("launching claude…");
    let settings = corral_core::settings::load().unwrap_or_default();
    let project_settings = corral_core::project_settings::get(project).unwrap_or_default();
    let claude_bin = if settings.claude_executable_path.trim().is_empty() {
        "claude"
    } else {
        settings.claude_executable_path.trim()
    };
    if corral_core::bin::resolve_executable(claude_bin).is_none() {
        anyhow::bail!(
            "{}",
            corral_core::bin::missing_executable_error(
                corral_core::harness::Harness::Claude,
                claude_bin
            )
        );
    }
    let status = Command::new(claude_bin)
        .current_dir(&wt.path)
        .arg("--append-system-prompt")
        .arg(corral_core::onboarding::system_prompt(
            project,
            feature,
            // `corral new` just created this worktree, so it's still on its
            // placeholder name — `wt.title` is empty here.
            &wt.title,
            &settings.extra_instructions,
            settings.co_author_trailer,
            &project_settings.project_prompt,
        ))
        .status()
        .with_context(|| format!("failed to launch `{claude_bin}`"))?;
    if !status.success() {
        anyhow::bail!("claude exited with status {status}");
    }
    Ok(())
}

fn cmd_list() -> Result<()> {
    let worktrees = worktree::list().context("listing worktrees")?;
    if worktrees.is_empty() {
        println!(
            "no worktrees under {}",
            corral_core::config::worktrees_root()?.display()
        );
        return Ok(());
    }
    for wt in &worktrees {
        println!("{}  [{}]", label(wt), wt.branch);
        if let Some(s) = &wt.status {
            let dirty = if s.is_dirty() {
                let conflicts = if s.conflicts > 0 {
                    format!(" !{}", s.conflicts)
                } else {
                    String::new()
                };
                format!(
                    "  ~{} +{} ?{}{}",
                    s.modified + s.staged,
                    s.staged,
                    s.untracked,
                    conflicts
                )
            } else {
                "  clean".to_string()
            };
            println!("    ↑{} ↓{}{}", s.ahead, s.behind, dirty);
        }
    }
    Ok(())
}

fn cmd_open(feature: &str, project: Option<&str>) -> Result<()> {
    let wt = resolve(feature, project)?;
    let settings = corral_core::settings::load().unwrap_or_default();
    Command::new(&settings.editor_command)
        .arg("-n")
        .arg(&wt.path)
        .spawn()
        .with_context(|| {
            format!(
                "failed to launch `{}` (is it on your PATH?)",
                settings.editor_command
            )
        })?;
    println!("opened {} with {}", label(&wt), settings.editor_command);
    Ok(())
}

fn cmd_done(feature: &str, project: Option<&str>, keep_branch: bool) -> Result<()> {
    let wt = resolve(feature, project)?;
    worktree::remove(&wt.project, &wt.feature, !keep_branch)
        .with_context(|| format!("removing worktree {}", label(&wt)))?;
    println!("removed {}", label(&wt));
    Ok(())
}

fn cmd_rename(new_name: &str, feature: Option<&str>, project: Option<&str>) -> Result<()> {
    let wt = match feature {
        Some(f) => resolve(f, project)?,
        None => {
            let cwd = std::env::current_dir().context("getting current directory")?;
            let (project, feature) = corral_core::config::resolve_worktree_from_path(&cwd)
                .context("not inside a corral-managed worktree (pass --feature explicitly)")?;
            worktree::list()?
                .into_iter()
                .find(|w| w.project == project && w.feature == feature)
                .with_context(|| format!("no worktree {project}/{feature}"))?
        }
    };
    let renamed = worktree::rename(&wt.project, &wt.feature, new_name)
        .with_context(|| format!("renaming worktree {}", label(&wt)))?;
    println!("renamed to {} [{}]", label(&renamed), renamed.branch);
    Ok(())
}

fn cmd_config(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Get => {
            let settings = corral_core::settings::load().context("loading settings")?;
            let resolved =
                corral_core::config::worktrees_root().context("resolving worktrees root")?;
            let kind = if settings.worktrees_root.trim().is_empty() {
                "default"
            } else {
                "override"
            };
            println!("{} ({kind})", resolved.display());
            Ok(())
        }
        ConfigAction::Set { path } => {
            validate_worktrees_root_input(&path)?;
            let mut settings = corral_core::settings::load().context("loading settings")?;
            settings.worktrees_root = path.trim().to_string();
            corral_core::settings::save(&settings).context("saving settings")?;
            let resolved =
                corral_core::config::worktrees_root().context("resolving worktrees root")?;
            println!("worktrees root set to {}", resolved.display());
            Ok(())
        }
        ConfigAction::Reset => {
            let mut settings = corral_core::settings::load().context("loading settings")?;
            settings.worktrees_root = String::new();
            corral_core::settings::save(&settings).context("saving settings")?;
            let resolved =
                corral_core::config::worktrees_root().context("resolving worktrees root")?;
            println!("worktrees root reset to default: {}", resolved.display());
            Ok(())
        }
    }
}

fn cmd_project(action: ProjectAction) -> Result<()> {
    match action {
        ProjectAction::Add { path } => {
            let canonical =
                std::fs::canonicalize(&path).with_context(|| format!("resolving {path}"))?;
            if !canonical.join(".git").exists() {
                anyhow::bail!("{} is not a git repository", canonical.display());
            }
            let name = canonical
                .file_name()
                .with_context(|| format!("{} has no directory name", canonical.display()))?
                .to_string_lossy()
                .into_owned();
            corral_core::project_settings::register(&name, canonical.clone())
                .with_context(|| format!("registering project {name}"))?;
            println!("registered {name} -> {}", canonical.display());
            Ok(())
        }
    }
}

/// Rejects input that isn't usable as a worktrees root: empty/whitespace,
/// or a relative path that isn't `~`-prefixed. Mirrors the GUI form's
/// validation rule (see `SettingsForm.tsx`).
fn validate_worktrees_root_input(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() || !(trimmed.starts_with('/') || trimmed.starts_with('~')) {
        return Err(corral_core::Error::InvalidWorktreesRoot(path.to_string()).into());
    }
    Ok(())
}

/// Resolve a feature or slug (optionally scoped to a project) to a single
/// worktree.
fn resolve(feature: &str, project: Option<&str>) -> Result<Worktree> {
    match project {
        Some(p) => worktree::list()?
            .into_iter()
            .find(|w| w.project == p && (w.feature == feature || w.slug == feature))
            .with_context(|| format!("no worktree {p}/{feature}")),
        None => worktree::find_by_feature(feature)
            .or_else(|_| {
                let mut matches: Vec<Worktree> = worktree::list()?
                    .into_iter()
                    .filter(|w| w.slug == feature)
                    .collect();
                if matches.len() > 1 {
                    return Err(corral_core::Error::WorktreeNotFound(feature.to_string()));
                }
                matches
                    .pop()
                    .ok_or_else(|| corral_core::Error::WorktreeNotFound(feature.to_string()))
            })
            .map_err(Into::into),
    }
}

fn label(wt: &Worktree) -> String {
    format!("{}/{}", wt.project, wt.slug)
}

#[derive(serde::Deserialize)]
struct HookInput {
    cwd: String,
    hook_event_name: Option<String>,
    transcript_path: Option<String>,
    /// In-flight background work reported by Claude Code's `Stop` hook.
    /// A `Stop` with entries here means the turn paused while a shell,
    /// subagent, monitor, etc. keeps running — not that the session is done.
    #[serde(default)]
    background_tasks: Vec<Value>,
}

/// Extracted so it's unit-testable without stdin/env/socket I/O.
fn parse_hook_input(stdin_json: &str) -> Result<HookInput> {
    serde_json::from_str(stdin_json).context("parsing hook stdin JSON")
}

/// Claude Code fires `Stop` when a turn ends even if background work launched
/// during it is still in flight; its hook input lists those tasks in
/// `background_tasks`. Treat only those stops as "working" — the session is
/// paused, not done. When the task finishes, Claude wakes into a new turn
/// (`UserPromptSubmit` reports working), and that turn's `Stop` with an empty
/// list settles the status back to waiting.
///
/// Scheduled wakeups (`session_crons`) are deliberately not included: no work
/// is running in the meantime, and marking a far-off cron "working" would also
/// keep corral's sleep inhibitor on indefinitely.
fn reported_event(event: &HookEvent, input: &HookInput) -> HookEvent {
    if matches!(event, HookEvent::Waiting)
        && input.hook_event_name.as_deref() == Some("Stop")
        && !input.background_tasks.is_empty()
    {
        HookEvent::Working
    } else {
        event.clone()
    }
}

/// Longest raw summary text forwarded off this machine before the frontend
/// does its own markdown-stripping/truncation for display — just a safety
/// cap so one giant final message can't balloon the hook-socket payload.
const MAX_RAW_SUMMARY_LEN: usize = 2000;

/// Best-effort: pulls the text of the last assistant turn out of a Claude
/// Code transcript (JSONL, one message per line), for use as a one-line
/// "what did it just do" summary. Returns `None` on any I/O/parse failure or
/// if no assistant entry has text content — callers treat that as "leave the
/// previous summary alone" rather than an error worth surfacing.
fn extract_last_assistant_summary(transcript_path: &str) -> Option<String> {
    let contents = std::fs::read_to_string(transcript_path).ok()?;
    for line in contents.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(blocks) = entry.pointer("/message/content").and_then(Value::as_array) else {
            continue;
        };
        let text: String = blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        return Some(truncate_on_char_boundary(text, MAX_RAW_SUMMARY_LEN));
    }
    None
}

/// Truncates `s` to at most `max` bytes without splitting a UTF-8 codepoint.
/// `String::truncate` panics when the cut lands inside a multi-byte character,
/// and this input is arbitrary assistant text (emoji, curly quotes, CJK), so
/// the notify hook — which must never fail — can't use it directly.
fn truncate_on_char_boundary(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Best-effort: any failure (corral-app not running, socket gone, unknown
/// worktree) is logged and swallowed, never propagated — see `NotifyHook`'s
/// doc comment on why this must always exit 0.
fn cmd_notify_hook(event: &HookEvent) {
    if let Err(e) = try_notify_hook(event) {
        tracing::debug!("notify-hook: not delivered: {e:#}");
    }
}

fn try_notify_hook(event: &HookEvent) -> Result<()> {
    use std::io::{Read, Write};

    let mut stdin_json = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_json)
        .context("reading hook stdin")?;
    let input = parse_hook_input(&stdin_json)?;
    let event = reported_event(event, &input);
    let cwd = std::path::PathBuf::from(&input.cwd);

    let (project, feature) = corral_core::config::resolve_worktree_from_path(&cwd)
        .context("resolving worktree from cwd")?;
    let id = corral_core::agent_session::key(&project, &feature);

    // Only a genuine `Stop` (not the `Notification`/`UserPromptSubmit` events
    // that also map to `HookEvent::Waiting`/`Working`, see `hooks.rs`) marks
    // the end of a turn with real assistant text worth summarizing.
    let summary = if input.hook_event_name.as_deref() == Some("Stop") {
        input
            .transcript_path
            .as_deref()
            .and_then(extract_last_assistant_summary)
    } else {
        None
    };

    let socket =
        std::env::var(corral_core::hooks::HOOK_SOCKET_ENV).context("CORRAL_HOOK_SOCKET not set")?;
    let mut stream =
        std::os::unix::net::UnixStream::connect(&socket).context("connecting to hook socket")?;
    let payload = serde_json::json!({ "id": id, "event": event.as_str(), "summary": summary });
    writeln!(stream, "{payload}").context("writing to hook socket")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hook_input_extracts_the_cwd_field() {
        let stdin = r#"{"session_id":"abc","cwd":"/tmp/foo","hook_event_name":"Stop"}"#;
        assert_eq!(parse_hook_input(stdin).unwrap().cwd, "/tmp/foo");
    }

    #[test]
    fn parse_hook_input_rejects_missing_cwd() {
        assert!(parse_hook_input(r#"{"session_id":"abc"}"#).is_err());
    }

    #[test]
    fn parse_hook_input_extracts_event_name_and_transcript_path() {
        let stdin =
            r#"{"cwd":"/tmp/foo","hook_event_name":"Stop","transcript_path":"/tmp/t.jsonl"}"#;
        let input = parse_hook_input(stdin).unwrap();
        assert_eq!(input.hook_event_name.as_deref(), Some("Stop"));
        assert_eq!(input.transcript_path.as_deref(), Some("/tmp/t.jsonl"));
    }

    #[test]
    fn parse_hook_input_allows_missing_event_name_and_transcript_path() {
        let input = parse_hook_input(r#"{"cwd":"/tmp/foo"}"#).unwrap();
        assert!(input.hook_event_name.is_none());
        assert!(input.transcript_path.is_none());
        assert!(input.background_tasks.is_empty());
    }

    #[test]
    fn parse_hook_input_reads_background_tasks() {
        let input = parse_hook_input(
            r#"{"cwd":"/tmp/foo","hook_event_name":"Stop","background_tasks":[{"id":"t1","type":"shell"}]}"#,
        )
        .unwrap();
        assert_eq!(input.background_tasks.len(), 1);
    }

    fn stop_input(background_tasks: Vec<Value>) -> HookInput {
        HookInput {
            cwd: "/tmp/foo".to_string(),
            hook_event_name: Some("Stop".to_string()),
            transcript_path: None,
            background_tasks,
        }
    }

    #[test]
    fn stop_with_background_tasks_reports_working() {
        let input = stop_input(vec![serde_json::json!({ "id": "t1", "type": "shell" })]);
        assert_eq!(
            reported_event(&HookEvent::Waiting, &input).as_str(),
            "working"
        );
    }

    #[test]
    fn stop_without_background_tasks_reports_waiting() {
        let input = stop_input(Vec::new());
        assert_eq!(
            reported_event(&HookEvent::Waiting, &input).as_str(),
            "waiting"
        );
    }

    #[test]
    fn non_stop_waiting_event_ignores_background_tasks() {
        // Only Stop's own list describes end-of-turn background work.
        // A Notification still means the user is needed for that prompt.
        let input = HookInput {
            cwd: "/tmp/foo".to_string(),
            hook_event_name: Some("Notification".to_string()),
            transcript_path: None,
            background_tasks: vec![serde_json::json!({ "id": "t1" })],
        };
        assert_eq!(
            reported_event(&HookEvent::Waiting, &input).as_str(),
            "waiting"
        );
    }

    #[test]
    fn working_event_is_never_downgraded() {
        let input = stop_input(vec![serde_json::json!({ "id": "t1" })]);
        assert_eq!(
            reported_event(&HookEvent::Working, &input).as_str(),
            "working"
        );
    }

    fn write_transcript(dir: &std::path::Path, lines: &[&str]) -> std::path::PathBuf {
        let path = dir.join("transcript.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    #[test]
    fn extract_last_assistant_summary_takes_the_last_assistant_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_transcript(
            dir.path(),
            &[
                r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"do the thing"}]}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Working on it."}]}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Fixed the bug."}]}}"#,
            ],
        );
        assert_eq!(
            extract_last_assistant_summary(path.to_str().unwrap()),
            Some("Fixed the bug.".to_string())
        );
    }

    #[test]
    fn extract_last_assistant_summary_joins_multiple_text_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_transcript(
            dir.path(),
            &[
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Part one."},{"type":"tool_use","name":"Bash"},{"type":"text","text":"Part two."}]}}"#,
            ],
        );
        assert_eq!(
            extract_last_assistant_summary(path.to_str().unwrap()),
            Some("Part one. Part two.".to_string())
        );
    }

    #[test]
    fn extract_last_assistant_summary_skips_text_less_assistant_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_transcript(
            dir.path(),
            &[
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Real summary."}]}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash"}]}}"#,
            ],
        );
        assert_eq!(
            extract_last_assistant_summary(path.to_str().unwrap()),
            Some("Real summary.".to_string())
        );
    }

    #[test]
    fn extract_last_assistant_summary_returns_none_for_missing_file() {
        assert!(extract_last_assistant_summary("/nonexistent/path.jsonl").is_none());
    }

    #[test]
    fn extract_last_assistant_summary_returns_none_when_no_assistant_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_transcript(
            dir.path(),
            &[
                r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
            ],
        );
        assert!(extract_last_assistant_summary(path.to_str().unwrap()).is_none());
    }

    #[test]
    fn extract_last_assistant_summary_does_not_panic_on_a_multibyte_truncation_boundary() {
        // A string whose byte `MAX_RAW_SUMMARY_LEN` falls inside a multi-byte
        // character: `String::truncate` would panic here, aborting the hook.
        let prefix = "a".repeat(MAX_RAW_SUMMARY_LEN - 1);
        let text = format!("{prefix}é and more text");
        let dir = tempfile::tempdir().unwrap();
        let line = serde_json::json!({
            "type": "assistant",
            "message": { "role": "assistant", "content": [{ "type": "text", "text": text }] },
        })
        .to_string();
        let path = write_transcript(dir.path(), &[&line]);
        let summary = extract_last_assistant_summary(path.to_str().unwrap()).unwrap();
        assert!(summary.len() <= MAX_RAW_SUMMARY_LEN);
        assert!(summary.starts_with(&prefix));
    }

    #[test]
    fn truncate_on_char_boundary_keeps_short_strings_intact() {
        assert_eq!(truncate_on_char_boundary("hello", 2000), "hello");
    }

    #[test]
    fn truncate_on_char_boundary_backs_off_to_a_boundary() {
        // "é" is 2 bytes, so cutting at 1 must yield an empty string, and
        // cutting at 3 must yield just "é".
        assert_eq!(truncate_on_char_boundary("é", 1), "");
        assert_eq!(truncate_on_char_boundary("é", 3), "é");
    }

    #[test]
    fn config_get_parses() {
        let cli = Cli::try_parse_from(["corral", "config", "get"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Config {
                action: ConfigAction::Get
            }
        ));
    }

    #[test]
    fn awake_parses_a_command_after_double_dash() {
        let cli = Cli::try_parse_from(["corral", "awake", "--", "sleep", "1"]).unwrap();
        let Commands::Awake { why, command } = cli.command else {
            panic!("expected awake");
        };
        assert_eq!(why, "agent background work");
        assert_eq!(command, vec!["sleep".to_string(), "1".to_string()]);
    }

    #[test]
    fn awake_parses_a_custom_reason_and_hyphenated_arguments() {
        let cli = Cli::try_parse_from([
            "corral",
            "awake",
            "--why",
            "map 9 scan",
            "--",
            "bash",
            "-c",
            "echo hi",
        ])
        .unwrap();
        let Commands::Awake { why, command } = cli.command else {
            panic!("expected awake");
        };
        assert_eq!(why, "map 9 scan");
        assert_eq!(
            command,
            vec!["bash".to_string(), "-c".to_string(), "echo hi".to_string()]
        );
    }

    #[test]
    fn awake_requires_a_command() {
        assert!(Cli::try_parse_from(["corral", "awake"]).is_err());
        assert!(Cli::try_parse_from(["corral", "awake", "--why", "x"]).is_err());
    }

    #[test]
    fn config_set_parses_with_path() {
        let cli = Cli::try_parse_from(["corral", "config", "set", "~/corral"]).unwrap();
        match cli.command {
            Commands::Config {
                action: ConfigAction::Set { path },
            } => assert_eq!(path, "~/corral"),
            _ => panic!("expected Config(Set)"),
        }
    }

    #[test]
    fn config_reset_parses() {
        let cli = Cli::try_parse_from(["corral", "config", "reset"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Config {
                action: ConfigAction::Reset
            }
        ));
    }

    #[test]
    fn project_add_parses_with_path() {
        let cli = Cli::try_parse_from(["corral", "project", "add", "/tmp/some-repo"]).unwrap();
        match cli.command {
            Commands::Project {
                action: ProjectAction::Add { path },
            } => assert_eq!(path, "/tmp/some-repo"),
            _ => panic!("expected Project(Add)"),
        }
    }

    #[test]
    fn validate_worktrees_root_input_accepts_absolute_path() {
        assert!(validate_worktrees_root_input("/mnt/data/worktrees").is_ok());
    }

    #[test]
    fn validate_worktrees_root_input_accepts_tilde_path() {
        assert!(validate_worktrees_root_input("~/corral").is_ok());
    }

    #[test]
    fn validate_worktrees_root_input_rejects_relative_path() {
        assert!(validate_worktrees_root_input("relative/path").is_err());
    }

    #[test]
    fn validate_worktrees_root_input_rejects_empty() {
        assert!(validate_worktrees_root_input("").is_err());
        assert!(validate_worktrees_root_input("   ").is_err());
    }
}
