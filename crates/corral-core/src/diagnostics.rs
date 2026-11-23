//! `corral doctor` — a read-only snapshot of the machine as corral sees it,
//! meant to be pasted into a bug report.
//!
//! Every probe here is best-effort: a missing executable, an unreadable
//! settings file, or a `git` that hangs all degrade to a line saying so
//! rather than a failure. That's deliberate — the machine `doctor` is most
//! useful on is the one where everything else is broken. Nothing here writes
//! to disk or touches the network.
//!
//! Two audiences share [`collect`]: the CLI renders it as text (see
//! [`render_text`]) or JSON, and the GUI's "Copy diagnostics" button uses
//! [`render_markdown`] for a fenced block that drops straight into a GitHub
//! issue. The report therefore holds **no project names, worktree names,
//! agent session content, or absolute user paths** — counts, versions, and
//! `~`-abbreviated roots only. `--verbose` is the opt-out from that rule,
//! and it's the user's choice to share what it adds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::harness::Harness;

/// A single versioned probe result: what we looked for, where it resolved,
/// what it reported, and whether it looks usable.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Probe {
    pub name: String,
    /// `Some(path)` when found, `None` when it isn't on `PATH` / at the
    /// configured path.
    pub path: Option<String>,
    /// The tool's self-reported version, or the probe's own finding when the
    /// tool has no `--version` (e.g. `tool:gh` reports "not authenticated").
    pub version: Option<String>,
    pub ok: bool,
}

impl Probe {
    fn found(name: &str, path: PathBuf, version: Option<String>) -> Self {
        Probe {
            name: name.to_string(),
            path: Some(abbreviate_home(&path, home_dir().as_deref())),
            version,
            ok: true,
        }
    }

    fn missing(name: &str) -> Self {
        Probe {
            name: name.to_string(),
            path: None,
            version: None,
            ok: false,
        }
    }

    /// Found, but not in a usable state (e.g. `gh` present but logged out).
    fn unusable(name: &str, path: PathBuf, note: String) -> Self {
        Probe {
            name: name.to_string(),
            path: Some(abbreviate_home(&path, home_dir().as_deref())),
            version: Some(note),
            ok: false,
        }
    }
}

/// One agent harness's configured executable and whether corral can spawn it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessProbe {
    pub harness: String,
    /// The executable name/path corral would actually use, after the
    /// settings override (empty override means the harness default).
    pub executable: String,
    /// True when this is the harness sessions currently spawn with.
    pub active: bool,
    pub probe: Probe,
}

/// Platform facts that historically correlate with corral bug reports: the
/// desktop environment (window decoration + tiling WM behavior), the WebKit
/// build (the bundled-build blank-window workarounds), and the sleep
/// inhibitor.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Environment {
    pub os: String,
    pub arch: String,
    /// `Some("Arch Linux")`, `Some("macOS 15.1")`, …
    pub distro: Option<String>,
    pub kernel: Option<String>,
    /// `Some("Hyprland")` on Linux when `XDG_CURRENT_DESKTOP` is set.
    pub desktop: Option<String>,
    /// `Some("wayland")` / `Some("x11")` / `Some("tty")` from
    /// `XDG_SESSION_TYPE`.
    pub session_type: Option<String>,
    /// True when the desktop is a known tiling WM (corral draws its own
    /// close-only title bar there — see `corral-app`'s `decorations_for_desktop`).
    pub tiling_wm: bool,
    /// The `webkit2gtk` package version on Linux, where bundled builds carry
    /// workarounds keyed to it.
    pub webkit_version: Option<String>,
    /// The sleep-inhibitor binary corral would hold (`systemd-inhibit`,
    /// `caffeinate`) and whether it's on `PATH`.
    pub inhibitor: Option<String>,
}

/// Which section of corral's state could be read.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct State {
    /// `~`-abbreviated corral config dir.
    pub config_dir: Option<String>,
    /// `~`-abbreviated corral data dir.
    pub data_dir: Option<String>,
    /// `~`-abbreviated worktrees root.
    pub worktrees_root: Option<String>,
    /// True when `worktrees_root` came from Settings rather than the
    /// `~/corral` default.
    pub worktrees_root_custom: bool,
    pub projects: usize,
    pub worktrees: usize,
    /// Worktrees with a stored, resumable agent session id.
    pub resumable_sessions: usize,
    /// Whether `settings.json` parses as a `Settings` blob.
    pub settings_ok: Option<bool>,
    /// Whether `projects.json` parses.
    pub projects_ok: Option<bool>,
    /// Path of the most recent `*.bak.*` backup file in the config dir,
    /// `~`-abbreviated. A recent backup usually means a load failed (e.g. a
    /// newer build's file being read by an older one).
    pub latest_backup: Option<String>,
    pub logs_dir: Option<String>,
}

/// Everything `corral doctor` collects.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Report {
    /// `0.2.0 (a1b2c3d)` — the caller passes its own crate version, since the
    /// CLI and the GUI embed their own.
    pub version: String,
    /// `corral` or `corral-app`.
    pub binary: String,
    /// The harness new sessions spawn with.
    pub harness: Option<String>,
    /// `"Claude Code"` etc., human-facing.
    pub harness_display: Option<String>,
    pub harnesses: Vec<HarnessProbe>,
    pub git: Probe,
    /// `gh`'s presence and auth state — checked because PR/merge features
    /// fail confusingly without it.
    pub gh: Option<Probe>,
    /// `just`'s presence — Run-tab targets prefer justfile recipes.
    pub just: Option<Probe>,
    pub env: Environment,
    pub state: State,
    /// Human-readable warning lines, ready to render. A report with warnings
    /// is still a successful run.
    pub warnings: Vec<String>,
}

/// The tiling WMs corral knows about. This is the canonical list: the app's
/// window-decoration decision (`corral-app`'s `decorations_for_desktop`)
/// classifies desktops with the same function, so `doctor`'s "[tiling WM]"
/// marker and the app's custom-chrome behavior can't drift apart.
pub const TILING_WMS: &[&str] = &[
    "hyprland", "sway", "i3", "dwm", "bspwm", "awesome", "xmonad", "qtile", "river", "wayfire",
];

/// Whether an `XDG_CURRENT_DESKTOP`-style value names a known tiling WM.
/// Case-insensitive, and tolerant of the `:`-separated list the variable
/// sometimes carries (`"Hyprland:wlroots"`).
pub fn is_tiling_desktop(current_desktop: Option<&str>) -> bool {
    match current_desktop {
        Some(value) => value
            .split(':')
            .any(|part| TILING_WMS.contains(&part.to_lowercase().as_str())),
        // Unknown desktop: assume a normal WM with real decorations, matching
        // the app's own default.
        None => false,
    }
}

fn home_dir() -> Option<PathBuf> {
    crate::config::home_dir().ok()
}

/// `<home>` prefix → `~`, so a report can be pasted publicly without
/// leaking a username or directory layout.
pub fn abbreviate_home(path: &Path, home: Option<&Path>) -> String {
    let shown = match home {
        Some(home) => match path.strip_prefix(home) {
            Ok(rel) if rel.as_os_str().is_empty() => PathBuf::from("~"),
            Ok(rel) => PathBuf::from("~").join(rel),
            Err(_) => path.to_path_buf(),
        },
        None => path.to_path_buf(),
    };
    shown.display().to_string()
}

/// Run `bin` with `args`, returning trimmed stdout on exit 0. Bounded by
/// `timeout` so a wedged tool can't hang `doctor`. Synchronous by design:
/// `doctor` is a one-shot CLI command, and the app's Copy-diagnostics button
/// runs it on a Tauri worker thread.
fn output_with_timeout(bin: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;

    let mut child = std::process::Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut out = String::new();
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                let out = out.trim();
                return (!out.is_empty()).then(|| out.to_string());
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

/// Probe an executable by name-or-path, optionally asking it for a version.
/// `version_args` is empty for tools that have no useful `--version`.
fn probe_executable(name: &str, bin: &str, version_args: &[&str]) -> Probe {
    let Some(path) = crate::bin::resolve_executable(bin) else {
        return Probe::missing(name);
    };
    let version = if version_args.is_empty() {
        None
    } else {
        output_with_timeout(&path, version_args, Duration::from_secs(5)).map(|v| clean_version(&v))
    };
    Probe::found(name, path, version)
}

/// Reduce a `--version` line to just the version number. Tools disagree on
/// format — `git version 2.56.1`, `gh version 2.105.0 (2026-11-18)`,
/// `opencode v2.3.0`, `just 1.59.0`, `2.1.266 (Claude Code)` — so: take the
/// first line, drop a trailing parenthesized build string, and return the
/// first token that starts with a digit (stripping a leading `v`).
fn clean_version(raw: &str) -> String {
    let line = raw.lines().next().unwrap_or_default().trim();
    let without_build = match line.find(" (") {
        Some(i) if line.ends_with(')') => &line[..i],
        _ => line,
    };
    without_build
        .split_whitespace()
        .find(|tok| {
            tok.trim_start_matches('v')
                .starts_with(|c: char| c.is_ascii_digit())
        })
        .unwrap_or(without_build)
        .trim_start_matches('v')
        .to_string()
}

/// Every harness corral knows, in a stable order, with the executable each
/// would spawn given `settings`.
fn harness_probes(settings: &crate::settings::Settings) -> Vec<HarnessProbe> {
    let overrides = [
        (Harness::Claude, settings.claude_executable_path.as_str()),
        (
            Harness::Opencode,
            settings.opencode_executable_path.as_str(),
        ),
        (
            Harness::Opencode2,
            settings.opencode2_executable_path.as_str(),
        ),
        (Harness::Pi, settings.pi_executable_path.as_str()),
    ];
    overrides
        .into_iter()
        .map(|(harness, configured)| {
            let bin = if configured.trim().is_empty() {
                harness.default_executable().to_string()
            } else {
                configured.trim().to_string()
            };
            let probe = if bin.is_empty() {
                Probe::missing(&harness.display_name())
            } else {
                probe_executable(&harness.display_name(), &bin, &["--version"])
            };
            HarnessProbe {
                active: settings.harness == harness,
                harness: harness.display_name(),
                executable: bin,
                probe,
            }
        })
        .collect()
}

/// The sleep inhibitor corral would use on this platform.
pub fn inhibitor_binary() -> &'static str {
    if cfg!(target_os = "macos") {
        "caffeinate"
    } else if cfg!(target_os = "linux") {
        "systemd-inhibit"
    } else {
        ""
    }
}

fn read_os_release() -> Option<BTreeMap<String, String>> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        map.insert(k.trim().to_string(), v);
    }
    Some(map)
}

/// `/etc/os-release`'s `PRETTY_NAME`, or `"Arch Linux"`-style fallback.
fn linux_distro() -> Option<String> {
    let map = read_os_release()?;
    map.get("PRETTY_NAME").or_else(|| map.get("NAME")).cloned()
}

/// The `webkit2gtk*` package version on Linux, probed two ways: a
/// `webkit2gtk-4.1` pkg-config module, then `pacman -Q` (which covers a
/// system without the `-dev` package installed). Best-effort on any distro.
fn webkit_version() -> Option<String> {
    let pkg_config = crate::bin::resolve_executable("pkg-config")?;
    let out = output_with_timeout(
        &pkg_config,
        &["--modversion", "webkit2gtk-4.1"],
        Duration::from_secs(5),
    );
    if let Some(v) = out {
        return Some(v);
    }
    let pacman = crate::bin::resolve_executable("pacman")?;
    // `pacman -Q webkit2gtk-4.1` prints "webkit2gtk-4.1 2.52.4-1"; the suffix
    // is the distro package release, not part of WebKit's own version.
    output_with_timeout(&pacman, &["-Q", "webkit2gtk-4.1"], Duration::from_secs(5)).map(|s| {
        s.split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .rsplit_once('-')
            .map(|(v, _)| v.to_string())
            .unwrap_or_default()
    })
}

/// The most-recently modified `*.bak.*` in `dir`, if any.
fn latest_backup(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".bak"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let modified = meta.modified().ok()?;
            Some((modified, e.path()))
        })
        .max_by_key(|(m, _)| *m)
        .map(|(_, p)| p)
}

/// Does this JSON file parse into `T`? `None` when the file is absent.
fn parses_as<T: serde::de::DeserializeOwned>(path: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str::<T>(&text).is_ok())
}

/// Gather a full report. Never returns `Err`: anything that can't be read
/// becomes a `None` field plus a warning.
///
/// `version`/`binary` identify the caller (`"0.2.0 (a1b2c3d)"` / `"corral"`),
/// since the app and the CLI both ship a report and each knows its own
/// version.
pub fn collect(version: &str, binary: &str) -> Report {
    let mut report = Report {
        version: version.to_string(),
        binary: binary.to_string(),
        ..Report::default()
    };
    let home = crate::config::home_dir().ok();
    let settings = crate::settings::load().unwrap_or_default();
    if crate::settings::load().is_err() {
        report
            .warnings
            .push("settings.json could not be read; showing defaults".into());
    }

    report.harness = Some(match &settings.harness {
        Harness::Claude => "claude".into(),
        Harness::Opencode => "opencode".into(),
        Harness::Opencode2 => "opencode2".into(),
        Harness::Pi => "pi".into(),
        Harness::Unknown(name) => name.clone(),
    });
    report.harness_display = Some(settings.harness.display_name());
    report.harnesses = harness_probes(&settings);

    // The active harness missing is the single most common "nothing works"
    // cause, so it gets a warning rather than just an `ok: false` row.
    if let Some(active) = report.harnesses.iter().find(|h| h.active && !h.probe.ok) {
        report.warnings.push(format!(
            "the active harness ({}) executable \"{}\" was not found on PATH — \
             sessions will fail to start ({})",
            active.harness,
            active.executable,
            settings.harness.install_hint()
        ));
    }
    if matches!(settings.harness, Harness::Unknown(_)) {
        report.warnings.push(
            "settings.json names a harness this build doesn't know (written by a newer corral?)"
                .into(),
        );
    }

    report.git = probe_executable("git", "git", &["--version"]);
    if !report.git.ok {
        report
            .warnings
            .push("git was not found on PATH — every worktree operation will fail".into());
    }

    let gh = probe_executable("gh", "gh", &["--version"]);
    report.gh = Some(match (&gh.ok, gh.path.as_deref()) {
        (true, Some(path)) => {
            let authenticated =
                output_with_timeout(Path::new(path), &["auth", "status"], Duration::from_secs(8))
                    .is_some();
            if authenticated {
                gh
            } else {
                Probe::unusable("gh", PathBuf::from(path), "not authenticated".into())
            }
        }
        _ => gh,
    });

    let just = probe_executable("just", "just", &["--version"]);
    report.just = Some(just);

    report.env = environment();
    report.state = state(&settings, home.as_deref(), &mut report.warnings);
    report
}

fn environment() -> Environment {
    let mut env = Environment {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        ..Environment::default()
    };
    env.distro = match std::env::consts::OS {
        "linux" => linux_distro(),
        "macos" => output_with_timeout(
            Path::new("sw_vers"),
            &["-productVersion"],
            Duration::from_secs(3),
        )
        .map(|v| format!("macOS {v}")),
        _ => None,
    };
    if std::env::consts::OS == "linux" {
        env.kernel = output_with_timeout(Path::new("uname"), &["-r"], Duration::from_secs(3));
        env.webkit_version = webkit_version();
    }
    env.desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .filter(|v| !v.trim().is_empty());
    env.tiling_wm = is_tiling_desktop(env.desktop.as_deref());
    env.session_type = std::env::var("XDG_SESSION_TYPE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("WAYLAND_DISPLAY")
                .ok()
                .map(|_| "wayland".to_string())
        });
    let inhibitor = inhibitor_binary();
    if !inhibitor.is_empty() {
        env.inhibitor = Some(match crate::bin::resolve_executable(inhibitor) {
            Some(_) => inhibitor.to_string(),
            None => String::new(),
        });
    }
    env
}

fn state(
    settings: &crate::settings::Settings,
    home: Option<&Path>,
    warnings: &mut Vec<String>,
) -> State {
    let mut state = State {
        projects: crate::project::list().map(|p| p.len()).unwrap_or(0),
        worktrees: crate::worktree::list().map(|w| w.len()).unwrap_or(0),
        resumable_sessions: crate::agent_session::resumable_keys()
            .map(|k| k.len())
            .unwrap_or(0),
        worktrees_root_custom: !settings.worktrees_root.trim().is_empty(),
        ..State::default()
    };

    let config = crate::paths::config_root().ok();
    let data = crate::paths::data_root().ok();
    state.config_dir = config.as_deref().map(|p| abbreviate_home(p, home));
    state.data_dir = data.as_deref().map(|p| abbreviate_home(p, home));
    state.worktrees_root = crate::config::worktrees_root()
        .ok()
        .map(|p| abbreviate_home(&p, home));
    state.logs_dir = data
        .as_deref()
        .map(|d| abbreviate_home(&d.join("logs"), home));

    if let Some(config) = config {
        state.settings_ok = parses_as::<crate::settings::Settings>(&config.join("settings.json"));
        state.projects_ok = parses_as::<BTreeMap<String, crate::project_settings::ProjectSettings>>(
            &config.join("projects.json"),
        );
        state.latest_backup = latest_backup(&config).map(|p| abbreviate_home(&p, home));
        let parse_failed = state.settings_ok == Some(false) || state.projects_ok == Some(false);
        if state.settings_ok == Some(false) {
            warnings.push("settings.json does not parse — corral is running on defaults".into());
        }
        if state.projects_ok == Some(false) {
            warnings
                .push("projects.json does not parse — the project list may appear empty".into());
        }
        if parse_failed && state.latest_backup.is_some() {
            // A backup alone is routine (every save writes one); it only
            // matters next to a parse failure, where it holds the contents
            // this build couldn't read.
            warnings.push(
                "the unreadable config was backed up to *.bak — its previous contents are \
                 still there"
                    .into(),
            );
        }
    }

    if state.worktrees_root.is_none() {
        warnings.push("could not resolve the worktrees root".into());
    }
    state
}

/// Render the report as plain text for a terminal (or a plain issue paste).
pub fn render_text(report: &Report) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} {}   harness: {}\n",
        report.binary,
        report.version,
        report.harness_display.as_deref().unwrap_or("?")
    ));
    out.push_str(&format!(
        "os: {} {}   {}\n",
        report.env.os,
        report.env.arch,
        report.env.distro.as_deref().unwrap_or("(unknown distro)")
    ));
    if let Some(kernel) = &report.env.kernel {
        out.push_str(&format!("kernel: {kernel}\n"));
    }

    section(&mut out, "executables");
    out.push_str(&format_probe(&report.git));
    for h in &report.harnesses {
        let marker = if h.active { " *" } else { "  " };
        out.push_str(&format!("{marker}{}", format_probe(&h.probe)));
    }
    for probe in report.gh.iter().chain(report.just.iter()) {
        out.push_str(&format_probe(probe));
    }
    out.push_str("  (* = the harness new sessions spawn with)\n");

    section(&mut out, "environment");
    let mut desk = report
        .env
        .desktop
        .clone()
        .unwrap_or_else(|| "(unknown)".into());
    if report.env.tiling_wm {
        desk.push_str(" [tiling WM]");
    }
    if let Some(session) = &report.env.session_type {
        desk.push_str(&format!(" on {session}"));
    }
    out.push_str(&format!("desktop: {desk}\n"));
    if let Some(webkit) = &report.env.webkit_version {
        out.push_str(&format!("webkit2gtk: {webkit}\n"));
    }
    match report.env.inhibitor.as_deref() {
        Some("") => out.push_str("inhibitor: none found (sleep guard disabled)\n"),
        Some(bin) => out.push_str(&format!("inhibitor: {bin} ok\n")),
        None => {}
    }

    section(&mut out, "state");
    if let Some(dir) = &report.state.config_dir {
        out.push_str(&format!("config:  {dir}\n"));
    }
    if let Some(dir) = &report.state.data_dir {
        out.push_str(&format!("data:    {dir}\n"));
    }
    if let Some(root) = &report.state.worktrees_root {
        let note = if report.state.worktrees_root_custom {
            ""
        } else {
            " (default)"
        };
        out.push_str(&format!("worktrees: {root}{note}\n"));
    }
    out.push_str(&format!(
        "{} projects, {} worktrees, {} resumable sessions\n",
        report.state.projects, report.state.worktrees, report.state.resumable_sessions
    ));
    out.push_str(&format!(
        "settings.json: {}   projects.json: {}\n",
        yes_no(report.state.settings_ok),
        yes_no(report.state.projects_ok)
    ));
    if let Some(backup) = &report.state.latest_backup {
        out.push_str(&format!("latest backup: {backup}\n"));
    }
    if let Some(logs) = &report.state.logs_dir {
        out.push_str(&format!("logs: {logs}\n"));
    }

    if !report.warnings.is_empty() {
        section(&mut out, "warnings");
        for w in &report.warnings {
            out.push_str(&format!("  ! {w}\n"));
        }
    }
    out
}

/// Render as a Markdown fenced block, with a one-line preamble suitable for a
/// GitHub issue body.
pub fn render_markdown(report: &Report) -> String {
    format!(
        "### corral diagnostics\n\n```\n{}```\n",
        render_text(report)
    )
}

fn section(out: &mut String, title: &str) {
    out.push_str(&format!("── {title} ──\n"));
}

fn format_probe(probe: &Probe) -> String {
    match (&probe.path, &probe.version, probe.ok) {
        (Some(path), Some(version), true) => format!("{}: {path} ({version})\n", probe.name),
        (Some(path), None, true) => format!("{}: {path}\n", probe.name),
        (Some(path), Some(note), false) => format!("{}: {path} [{note}]\n", probe.name),
        (None, _, _) => format!("{}: MISSING\n", probe.name),
        (Some(path), None, false) => format!("{}: {path}\n", probe.name),
    }
}

fn yes_no(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "parses",
        Some(false) => "FAILED TO PARSE",
        None => "absent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiling_wm_list_covers_the_apps_known_desktops() {
        // The app treats exactly this set as tiling (see its
        // `decorations_for_desktop` tests); a desktop added there must be
        // added here too or `doctor` will mislabel the machine.
        for desktop in [
            "hyprland", "sway", "i3", "dwm", "bspwm", "awesome", "xmonad", "qtile", "river",
            "wayfire",
        ] {
            assert!(
                is_tiling_desktop(Some(desktop)),
                "{desktop} should be tiling"
            );
        }
        for desktop in ["GNOME", "KDE", "xfce", "SomeFutureDE"] {
            assert!(
                !is_tiling_desktop(Some(desktop)),
                "{desktop} should not be tiling"
            );
        }
    }

    #[test]
    fn recognizes_tiling_desktops_case_insensitively() {
        assert!(is_tiling_desktop(Some("Hyprland")));
        assert!(is_tiling_desktop(Some("sway")));
        assert!(is_tiling_desktop(Some("Hyprland:wlroots")));
        assert!(!is_tiling_desktop(Some("GNOME")));
        assert!(!is_tiling_desktop(None));
    }

    #[test]
    fn abbreviates_paths_under_home() {
        let home = Path::new("/home/alice");
        assert_eq!(
            abbreviate_home(Path::new("/home/alice/corral"), Some(home)),
            "~/corral"
        );
        assert_eq!(abbreviate_home(Path::new("/home/alice"), Some(home)), "~");
        assert_eq!(
            abbreviate_home(Path::new("/opt/builds/corral"), Some(home)),
            "/opt/builds/corral"
        );
        assert_eq!(
            abbreviate_home(Path::new("/home/alice/x"), None),
            "/home/alice/x"
        );
    }

    #[test]
    fn cleans_version_strings_from_different_tools() {
        assert_eq!(clean_version("git version 2.56.1"), "2.56.1");
        assert_eq!(clean_version("gh version 2.105.0 (2026-11-18)"), "2.105.0");
        assert_eq!(clean_version("opencode v2.3.0"), "2.3.0");
        assert_eq!(clean_version("just 1.59.0"), "1.59.0");
        assert_eq!(clean_version("2.1.266 (Claude Code)"), "2.1.266");
        assert_eq!(clean_version("1.5.2"), "1.5.2");
    }

    #[test]
    fn missing_probe_renders_as_missing() {
        let probe = Probe::missing("pi");
        assert_eq!(format_probe(&probe), "pi: MISSING\n");
    }

    #[test]
    fn unusable_probe_renders_its_note() {
        let probe = Probe::unusable(
            "gh",
            PathBuf::from("/usr/bin/gh"),
            "not authenticated".into(),
        );
        assert_eq!(
            format_probe(&probe),
            "gh: /usr/bin/gh [not authenticated]\n"
        );
    }

    #[test]
    fn text_render_includes_warnings() {
        let report = Report {
            binary: "corral".into(),
            version: "0.2.0 (abc)".into(),
            warnings: vec!["git was not found on PATH".into()],
            ..Report::default()
        };
        let text = render_text(&report);
        assert!(text.contains("corral 0.2.0 (abc)"));
        assert!(text.contains("! git was not found on PATH"));
    }

    #[test]
    fn markdown_wraps_the_text_in_a_fence() {
        let report = Report {
            binary: "corral".into(),
            version: "1".into(),
            ..Report::default()
        };
        let md = render_markdown(&report);
        assert!(md.starts_with("### corral diagnostics"));
        assert!(md.contains("```\ncorral 1 "));
    }

    #[test]
    fn collect_never_fails_and_names_its_caller() {
        // The environment-specific parts vary, so this only asserts the
        // invariants: the report identifies its caller and `collect` returns.
        let report = collect("9.9.9 (test)", "corral");
        assert_eq!(report.version, "9.9.9 (test)");
        assert_eq!(report.binary, "corral");
        assert_eq!(report.harnesses.len(), 4);
        assert!(report.harnesses.iter().filter(|h| h.active).count() <= 1);
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = collect("0.2.0 (x)", "corral");
        let json = serde_json::to_string(&report).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
    }
}
