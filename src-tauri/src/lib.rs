//! Tauri app bootstrap — the command surface itself lives in [`commands`],
//! split by domain (worktrees, projects, settings, system, session).

mod browser_host;
mod browser_window;
mod commands;
mod conflict_poll;
mod hook_listener;
mod idle_timeout;
mod opencode_events;
mod pr_poll;
mod watch;
mod worktree_watch;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Pure decision logic, independent of the real environment — see
/// `should_decorate()` below for the actual env-var-reading entry point.
/// Windows and macOS never call this (they always get real decorations;
/// see `should_decorate()`'s non-Linux branch) since Tauri uses native
/// win32/Cocoa windows there, not GTK.
///
/// On Linux, Tauri's windowing goes through GTK3, which draws its own full
/// client-side title bar the instant decorations are requested, regardless
/// of the actual compositor — GTK3 has no Wayland xdg-decoration
/// negotiation, so real WM/compositor behavior can't be detected at
/// runtime. Decide statically instead: known tiling WMs don't have
/// minimize/maximize semantics at all (no taskbar to restore a minimized
/// window to, no meaningful "maximize" when tiling already manages size),
/// so they get corral's own close-only fallback chrome (see
/// `src/lib/window-chrome.ts`/`src/components/WindowControls.tsx`);
/// anything else — including unknown/future desktop environments — defaults
/// to real decorations rather than guessing wrong toward "hide everything."
fn decorations_for_desktop(current_desktop: Option<&str>) -> bool {
    const TILING_WMS: &[&str] = &[
        "hyprland", "sway", "i3", "dwm", "bspwm", "awesome", "xmonad", "qtile", "river", "wayfire",
    ];
    match current_desktop {
        Some(value) => !value
            .split(':')
            .any(|part| TILING_WMS.contains(&part.to_lowercase().as_str())),
        None => true,
    }
}

#[cfg(target_os = "linux")]
fn should_decorate() -> bool {
    decorations_for_desktop(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref())
}

#[cfg(not(target_os = "linux"))]
fn should_decorate() -> bool {
    true
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("corral_app_lib=debug,corral_core=debug")
            }),
        )
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("corral starting");

    // On Linux, Tauri's default custom-protocol content serving
    // (tauri://localhost) never completes navigation on this system's
    // webkit2gtk — the window stays stuck on about:blank. Real HTTP
    // navigation (the Vite dev server, and this plugin) works fine, so
    // bundled Linux builds serve assets the same way dev builds do. This is
    // specific to webkit2gtk/Wry: macOS (WKWebView) and Windows (WebView2)
    // serve the custom protocol correctly, so they keep the normal
    // `WebviewUrl::App` path (see the `url` selection in `setup` below).
    // Separately, the ipc:// command-dispatch channel needed its own fix —
    // see the `tauri` patch in the workspace Cargo.toml.
    #[cfg(all(not(dev), target_os = "linux"))]
    let port = match portpicker::pick_unused_port() {
        Some(port) => port,
        None => {
            // A bundled build has no attached terminal, so a panic message
            // here would never be seen — log it properly and exit instead.
            tracing::error!("no unused localhost port available; corral cannot start");
            std::process::exit(1);
        }
    };

    // tauri_plugin_localhost defaults to binding host "localhost", which on
    // this system resolves/binds to [::1] (IPv6) only — but the webview's
    // own "localhost" resolution can try 127.0.0.1 first, hitting nothing
    // and failing with "Connection refused". Bind and navigate to the same
    // explicit IPv4 loopback address so they can't disagree.
    #[cfg(all(not(dev), target_os = "linux"))]
    let builder = tauri::Builder::default().plugin(
        tauri_plugin_localhost::Builder::new(port)
            .host("127.0.0.1")
            .build(),
    );
    #[cfg(not(all(not(dev), target_os = "linux")))]
    let builder = tauri::Builder::default();

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::session::SessionRegistry::default())
        .manage(commands::provision::Registry::default())
        .manage(commands::run::Registry::default())
        .manage(pr_poll::PrPollCache::default())
        .manage(corral_core::caffeinate::InhibitGuard::default())
        .invoke_handler(tauri::generate_handler![
            commands::system::notify_icon,
            commands::system::send_notification,
            commands::system::home_dir,
            commands::system::memory_usage,
            commands::projects::list_projects,
            commands::projects::project_icon,
            commands::projects::add_project,
            commands::projects::remove_project,
            commands::worktrees::list_worktrees,
            commands::worktrees::set_worktree_note,
            commands::worktrees::branch_changes,
            commands::worktrees::file_diff,
            commands::worktrees::revert_file,
            commands::worktrees::list_worktree_dir,
            commands::worktrees::read_worktree_file,
            commands::worktrees::create_worktree,
            commands::worktrees::remove_worktree,
            commands::worktrees::rename_worktree,
            commands::worktrees::merge_worktree,
            commands::worktrees::rebase_worktree,
            commands::worktrees::pr_status,
            commands::worktrees::pr_checks,
            commands::worktrees::pr_details,
            commands::worktrees::check_log,
            commands::worktrees::merge_pr,
            commands::worktrees::mark_pr_ready,
            commands::worktrees::watch_worktree,
            commands::worktrees::refresh_worktree_status,
            commands::system::save_pasted_image,
            commands::system::save_text_attachment,
            commands::system::open_in_zed,
            commands::system::open_terminal,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::project_settings::get_project_settings,
            commands::project_settings::save_project_settings,
            commands::session::list_sessions,
            commands::session::start_session,
            commands::session::attach_session,
            commands::session::write_session,
            commands::session::resize_session,
            commands::session::kill_session,
            commands::session::close_session,
            commands::provision::get_provision_output,
            commands::provision::rerun_provision,
            commands::provision::stop_provision,
            commands::provision::resize_provision,
            commands::run::start_run,
            commands::run::stop_run,
            commands::run::get_run_output,
            commands::run::resize_run,
            commands::run::resolve_run_command,
            commands::run::list_run_targets,
            browser_host::browser_ui_ready,
            browser_host::browser_ui_bounds,
            browser_host::browser_tab_new,
            browser_host::browser_tab_focus,
            browser_host::browser_tab_close,
            browser_host::browser_navigate,
            browser_host::browser_go_back,
            browser_host::browser_go_forward,
            browser_host::browser_reload,
        ])
        .setup(move |app| {
            app.manage(browser_host::BrowserHosts::default());

            // Bundled Linux builds load from the localhost plugin (see the
            // `port`/`builder` setup above — capabilities/default.json grants
            // that origin the same permissions as local content via its
            // `remote.urls` wildcard, so no runtime capability is needed).
            // Dev builds and non-Linux platforms use the normal
            // custom-protocol asset serving.
            #[cfg(all(not(dev), target_os = "linux"))]
            let url = tauri::Url::parse(&format!("http://127.0.0.1:{port}"))
                .map(WebviewUrl::External)
                .expect("just-formatted loopback URL");
            #[cfg(not(all(not(dev), target_os = "linux")))]
            let url = WebviewUrl::App(std::path::PathBuf::from("/"));

            WebviewWindowBuilder::new(app, "main", url)
                .title("Corral")
                .inner_size(1100.0, 720.0)
                // Must stay >= the app grid's hard floor (256px sidebar +
                // 480px min middle column + 280px changes pane = 1016px in
                // src/styles.css's `.app` rule), or the window can shrink
                // past what the layout can render and clip content.
                .min_inner_size(1040.0, 480.0)
                .decorations(should_decorate())
                .build()?;

            // Self-healing: a worktree removed out-of-band (not through
            // corral's own `worktree::remove`) leaves orphaned entries in the
            // session store and note store — sweep those out once per launch.
            match corral_core::worktree::prune_orphaned_state() {
                Ok((0, 0, 0)) => {}
                Ok((sessions, notes, titles)) => {
                    tracing::info!(
                        "pruned {sessions} orphaned session-store entries, \
                         {notes} orphaned worktree-note entries, \
                         {titles} orphaned worktree-title entries"
                    );
                }
                Err(e) => tracing::warn!("failed to prune orphaned state: {e}"),
            }

            watch::spawn(app.handle().clone());
            app.manage(worktree_watch::spawn(app.handle().clone()));
            hook_listener::spawn(app.handle().clone());
            idle_timeout::spawn(app.handle().clone());
            conflict_poll::spawn(app.handle().clone());
            pr_poll::spawn(app.handle().clone());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Releases the held `systemd-inhibit` process on a clean quit.
            // A hard crash/SIGKILL is covered separately by `PR_SET_PDEATHSIG`
            // in `caffeinate::spawn_inhibit` — the kernel kills the inhibitor
            // itself in that case, so this handler only needs the clean path.
            if let tauri::RunEvent::Exit = event {
                app_handle
                    .state::<corral_core::caffeinate::InhibitGuard>()
                    .sync(false);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprland_falls_back_to_custom_chrome() {
        assert!(!decorations_for_desktop(Some("Hyprland")));
    }

    #[test]
    fn sway_falls_back_to_custom_chrome() {
        assert!(!decorations_for_desktop(Some("sway")));
    }

    #[test]
    fn matches_case_insensitively_within_a_colon_separated_list() {
        assert!(!decorations_for_desktop(Some("ubuntu:SWAY")));
    }

    #[test]
    fn gnome_gets_real_decorations() {
        assert!(decorations_for_desktop(Some("GNOME")));
    }

    #[test]
    fn kde_gets_real_decorations() {
        assert!(decorations_for_desktop(Some("KDE")));
    }

    #[test]
    fn unset_defaults_to_real_decorations() {
        assert!(decorations_for_desktop(None));
    }

    #[test]
    fn unknown_future_desktop_defaults_to_real_decorations() {
        assert!(decorations_for_desktop(Some("SomeFutureDE")));
    }
}
