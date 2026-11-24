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
            path: Some(redact_path(&path, home_dir().as_deref())),
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
            path: Some(redact_path(&path, home_dir().as_deref())),
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

/// Like [`abbreviate_home`], but never emits a path outside the home
/// directory verbatim — the report is meant to be pasted into a public issue,
/// and an install outside `$HOME` (a repo under `/srv` or `/opt`, a worktrees
/// root on an external drive) would otherwise name the user's directory
/// layout. Such paths keep their last component (enough to recognize *which*
/// tool/dir it is) with the parent elided: `/opt/builds/corral` →
/// `<outside home>/corral`. Paths with nothing identifying left degrade to
/// `<outside home>`.
///
/// When `home` is unknown every path is outside it, so the whole thing
/// degrades to the last-component form rather than leaking.
pub fn redact_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && path.strip_prefix(home).is_ok()
    {
        return abbreviate_home(path, Some(home));
    }
    match path.file_name().map(|n| n.to_string_lossy()) {
        // A bare root (`/`, or a Windows drive root) has no file name; naming
        // it tells the reader nothing anyway.
        Some(name) if !name.is_empty() => {
            format!("<outside home>/{}", sanitize_component(&name))
        }
        _ => "<outside home>".to_string(),
    }
}

/// A single path component made safe to render in a report. Components could
/// themselves carry unexpected content (a directory literally named
/// `token=abc`, a name with a newline), so non-path-ish characters are
/// replaced rather than passed through — this is a bug-report line, not a
/// filesystem round-trip.
fn sanitize_component(name: &str) -> String {
    const MAX: usize = 64;
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | ' ' | '@') {
                c
            } else {
                '?'
            }
        })
        .collect();
    if cleaned.chars().count() <= MAX {
        cleaned
    } else {
        let mut truncated: String = cleaned.chars().take(MAX).collect();
        truncated.push('…');
        truncated
    }
}

/// Run `bin` with `args`, returning trimmed stdout on exit 0. Bounded by
/// `timeout` so a wedged tool can't hang `doctor` or the app's
/// Copy-diagnostics button. Blocking, but never on the caller's UI thread —
/// the Tauri command wraps it in `spawn_blocking`.
///
/// stdout is drained on a dedicated thread *while* the child runs, not after
/// it exits. A tool that writes more than the pipe buffer (~64 KB) blocks on
/// its own `write()` until someone reads; a wait-then-read loop would let
/// `try_wait()` spin until the deadline and report a healthy tool as broken.
/// Same shape as `shell_env::run_with_timeout`, which solved this already.
fn output_with_timeout(bin: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::sync::mpsc;

    let mut child = std::process::Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = std::io::BufReader::new(stdout).read_to_end(&mut out);
        let _ = tx.send(out);
    });

    // Park on the exit status instead of polling, so a fast probe costs its
    // own runtime rather than a 25 ms poll tick. Every tool probed here is a
    // short-lived CLI, so `wait_timeout`'s helper thread per child is cheap
    // insurance against a wedged one.
    let Some(status) = wait_timeout(child, timeout) else {
        // Killed by `wait_timeout` (or un-waitable): the reader thread sees
        // EOF when the pipe closes and exits on its own.
        return None;
    };
    if !status.success() {
        return None;
    }
    let raw = rx.recv_timeout(timeout).ok()?;
    let out = String::from_utf8_lossy(&raw);
    let out = out.trim();
    (!out.is_empty()).then(|| out.to_string())
}

/// `child.wait()` bounded by `timeout`, killing and reaping on expiry so no
/// zombie is left behind. Consumes the child: waiting may only happen on the
/// thread that owns it, so the caller can't be left holding a half-waited
/// handle. `None` means "timed out or un-waitable".
///
/// Avoids a `wait-timeout` dependency for one call site: `std` has no timed
/// wait, so the wait itself runs on a thread the timeout abandons. The
/// timeout path can only kill by pid (the `Child` handle now lives on that
/// abandoned thread), which still closes the probe's pipe; the orphaned
/// waiter reaps it a moment later when `wait()` returns and sends to a
/// receiver nobody is reading. Nothing is left as a zombie.
fn wait_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    use std::sync::mpsc;

    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(status)) => Some(status),
        // Exited but un-waitable (`wait` errored) — nothing to report.
        Ok(Err(_)) => None,
        Err(_) => {
            // `kill -9` by pid rather than `Child::kill()`: the handle now
            // lives on the waiter thread. Only used to stop a wedged *probe*
            // we spawned ourselves, and a failure (already exited, pid
            // reused) is harmless at worst.
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            None
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
    state.config_dir = config.as_deref().map(|p| redact_path(p, home));
    state.data_dir = data.as_deref().map(|p| redact_path(p, home));
    state.worktrees_root = crate::config::worktrees_root()
        .ok()
        .map(|p| redact_path(&p, home));
    state.logs_dir = data.as_deref().map(|d| redact_path(&d.join("logs"), home));

    if let Some(config) = config {
        state.settings_ok = parses_as::<crate::settings::Settings>(&config.join("settings.json"));
        state.projects_ok = parses_as::<BTreeMap<String, crate::project_settings::ProjectSettings>>(
            &config.join("projects.json"),
        );
        state.latest_backup = latest_backup(&config).map(|p| redact_path(&p, home));
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
    fn redact_path_keeps_the_home_prefix_form_for_under_home() {
        let home = Path::new("/home/alice");
        assert_eq!(
            redact_path(Path::new("/home/alice/.local/bin/corral"), Some(home)),
            "~/.local/bin/corral"
        );
        assert_eq!(redact_path(Path::new("/home/alice"), Some(home)), "~");
    }

    /// The report is pasted into public issues, so a path outside `$HOME`
    /// must not be emitted verbatim — it would name the user's layout (an
    /// install under `/opt`, a worktrees root on a mounted drive).
    #[test]
    fn redact_path_elides_outside_home_parents() {
        let home = Path::new("/home/alice");
        assert_eq!(
            redact_path(Path::new("/opt/builds/corral"), Some(home)),
            "<outside home>/corral"
        );
        assert_eq!(
            redact_path(Path::new("/srv/projects/myapp"), Some(home)),
            "<outside home>/myapp"
        );
        // A bare root names nobody and tells the reader nothing.
        assert_eq!(redact_path(Path::new("/"), Some(home)), "<outside home>");
    }

    /// Unknown home ⇒ *everything* is outside it, so the fallback form has to
    /// be used rather than falling through to the raw path.
    #[test]
    fn redact_path_without_home_never_leaks_the_full_path() {
        let out = redact_path(Path::new("/home/alice/.local/bin/corral"), None);
        assert_eq!(out, "<outside home>/corral");
        assert!(!out.contains("alice"));
    }

    #[test]
    fn redact_path_sanitizes_and_bounds_the_component() {
        // A directory name can itself carry punctuation or control characters;
        // this is a report line, not a filesystem round-trip.
        assert_eq!(
            redact_path(Path::new("/opt/a b\tc/name\nwith:colon"), None),
            "<outside home>/name?with?colon"
        );
        let long = format!("/opt/{}", "x".repeat(200));
        let out = redact_path(Path::new(&long), None);
        // `<outside home>/` + 64 chars + the ellipsis.
        assert_eq!(out.chars().count(), "<outside home>/".len() + 65);
        assert!(out.ends_with('…'));
    }

    /// The redaction is only worth anything if prose fields can't smuggle the
    /// raw path back in. This pins the contract the module doc states.
    #[test]
    fn a_collected_report_contains_no_absolute_paths_outside_home() {
        let report = collect("0.0.0 (test)", "corral");
        let mut rendered = render_text(&report);
        rendered.push_str(&render_markdown(&report));
        // Every absolute-path token must have been redacted to a relative or
        // `~`/`<outside home>` form. This catches a new field that formats a
        // `Path` with `{}` instead of routing it through `redact_path`.
        for token in rendered.split_whitespace() {
            assert!(
                !token.starts_with('/'),
                "absolute path {token:?} leaked into the diagnostics report"
            );
        }
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

    /// Regression test for the wait-then-read bug: a tool that writes more
    /// than the ~64 KB pipe buffer must still be read successfully. The old
    /// implementation only started reading after `try_wait()` reported exit,
    /// so the child blocked on `write()` forever and the probe timed out.
    #[cfg(unix)]
    #[test]
    fn reads_output_larger_than_the_pipe_buffer() {
        // 200 KB is comfortably past Linux's default 64 KB pipe capacity and
        // macOS's 16 KB one.
        let out = output_with_timeout(
            Path::new("sh"),
            &["-c", "i=0; while [ $i -lt 2000 ]; do echo 0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789; i=$((i+1)); done"],
            Duration::from_secs(10),
        )
        .expect("a 200 KB write must not deadlock against the pipe buffer");
        assert_eq!(out.lines().count(), 2000);
    }

    /// A probe that outlives its timeout is killed and reported as absent,
    /// rather than hanging `doctor`.
    #[cfg(unix)]
    #[test]
    fn a_hung_probe_times_out_instead_of_blocking() {
        let started = std::time::Instant::now();
        let out = output_with_timeout(
            Path::new("sh"),
            &["-c", "sleep 30 & wait"],
            Duration::from_millis(150),
        );
        assert!(out.is_none(), "a hung probe must report no output");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must return near the timeout, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_nonzero_exit_reports_no_output() {
        // Exit status, not content, decides: `echo hi; exit 1` prints output
        // but the tool still failed, so there's no version to report.
        assert!(
            output_with_timeout(Path::new("sh"), &["-c", "exit 3"], Duration::from_secs(5))
                .is_none()
        );
        assert!(
            output_with_timeout(
                Path::new("sh"),
                &["-c", "echo hi; exit 1"],
                Duration::from_secs(5)
            )
            .is_none()
        );
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
        // `/usr/bin` isn't under `$HOME`, so the path is redacted to its
        // identifying component (see `redact_path`) — the report is pasted
        // into public issues.
        assert_eq!(
            format_probe(&probe),
            "gh: <outside home>/gh [not authenticated]\n"
        );
    }

    #[test]
    fn probe_under_home_keeps_the_tilde_form() {
        let home = crate::config::home_dir().ok();
        let Some(home) = home else { return };
        let probe = Probe::found("claude", home.join(".local/bin/claude"), None);
        assert_eq!(
            probe.path.as_deref(),
            Some("~/.local/bin/claude"),
            "a tool installed under home should stay readable in the report"
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
