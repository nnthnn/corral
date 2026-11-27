# AGENTS.md — corral

> Project context for coding agents, committed to the repo. opencode reads
> this natively; Claude Code picks it up via the committed `CLAUDE.md` shim
> (`@AGENTS.md`). Backlog items live in Linear (Dev team → "Corral"
> project), not in a repo file. Since this file is tracked, keep it free of
> secrets and anything personal.

## What corral is

A **Tauri desktop app for Linux and macOS** for running multiple parallel
coding-agent sessions, each isolated in its own git worktree. Each agent is a
real CLI process (Claude Code, OpenCode, or Pi) running in a PTY, shown in an
embedded terminal. There's also a `corral` CLI.

Windows is not supported yet: the code still assumes a Unix process model
(Unix-socket hooks, `symlink`'d `.claude` dirs, `/proc`-based process
tracking, `systemd-inhibit`/`caffeinate` sleep guards).

## Architecture

Cargo **workspace**:

```
corral/
├── Cargo.toml                    # workspace root
├── crates/
│   ├── corral-core/               # shared lib — the real logic
│   │   ├── config.rs               # worktrees root (default ~/corral) + feat/<x> conventions
│   │   ├── git.rs                  # shells out to git: status, changed_files,
│   │   │                           #   branch_changes, file_diff (no libgit2)
│   │   ├── files.rs                # gitignore-aware dir listing + file reads for previews
│   │   ├── project.rs              # registered projects + per-project icon candidates
│   │   ├── project_settings.rs     # per-project registry: provisioning rules, run command
│   │   ├── worktree.rs             # list / create / rename / remove + find_by_feature
│   │   ├── worktree_notes.rs       # per-worktree free-text sidebar note (untracked)
│   │   ├── worktree_settings.rs    # per-worktree settings (human-readable title)
│   │   ├── agent_session.rs        # worktree -> last agent session id, per harness, `--resume`
│   │   ├── harness.rs              # which agent CLI a session runs (Claude / OpenCode v1/v2 / Pi)
│   │   ├── opencode.rs             # spawns/talks to a headless `opencode serve` per session
│   │   ├── pi.rs                   # installs the bundled pi status extension (--extension)
│   │   ├── hooks.rs                # Claude Code hooks -> real waiting/working status
│   │   ├── merge.rs                # rebase onto base + fast-forward + push
│   │   ├── pr.rs                   # push branch + open/update PR via `gh`
│   │   ├── checks.rs               # GitHub check-run status, PR comments/reviews, merge via `gh`
│   │   ├── caffeinate.rs           # systemd-inhibit while any session is running
│   │   ├── onboarding.rs           # system-prompt briefing for a freshly started agent
│   │   │                           #   (+ BROWSER_INSTRUCTIONS for OpenCode browser tools)
│   │   ├── names.rs                # auto-picked placeholder feature names
│   │   ├── settings.rs             # user app preferences (disk-persisted)
│   │   ├── bin.rs                  # resolve agent CLIs on PATH + the bundled `corral` CLI
│   │   ├── process.rs              # sum RSS across the process tree (from /proc)
│   │   ├── natural.rs              # numeric-aware string ordering
│   │   ├── json_store.rs           # shared JSON-blob-on-disk primitives
│   │   ├── keyed_store.rs          # generic keyed JSON store (~/.local/share/corral)
│   │   ├── watch.rs                # which fs events matter for a watched worktree
│   │   ├── log_err.rs              # "log at warn, continue" ext trait
│   │   └── error.rs
│   └── corral-cli/                # `corral` binary (clap): new / list / open / done /
│                                   #   rename / config / project add / awake / notify-hook
│                                   #   (shipped as a Tauri externalBin sidecar)
├── scripts/                       # build/test helpers, e.g. stage-cli-sidecar.sh
├── src-tauri/                     # Tauri v2 app
│   ├── src/lib.rs                  # bootstrap: command surface + window/localhost-plugin setup
│   ├── src/commands/                # command surface, split by domain
│   │   ├── worktrees.rs            # lifecycle + git-diff commands
│   │   ├── projects.rs             # project list/registry commands
│   │   ├── project_settings.rs     # per-project provisioning-rule commands
│   │   ├── session.rs              # agent sessions in a PTY per worktree (Claude/OpenCode)
│   │   ├── provision.rs            # one-shot post-create setup (e.g. `pnpm install`)
│   │   ├── run.rs                  # long-lived "Run" command per worktree (Run tab)
│   │   ├── run_detect.rs           # best-effort dev-command auto-detection
│   │   ├── run_targets.rs          # Run-tab one-click targets: justfile recipes
│   │   │                           #   (else package.json scripts)
│   │   ├── pty_process.rs          # shared one-shot-PTY core (used by provision + run)
│   │   ├── settings.rs             # app-settings commands
│   │   ├── fonts.rs                # system monospaced families for the terminal font picker
│   │   └── system.rs               # notify icon, clipboard paste, open editor/terminal
│   ├── src/hook_listener.rs        # Unix-socket listener for Claude Code hook callbacks
│   ├── src/opencode_events.rs      # opencode SSE stream -> session status (same sink as hooks)
│   ├── src/browser_host.rs         # opencode's browser.* RPC -> embedded browser backend
│   ├── src/browser_window.rs       # corral-owned WebKit browser window + child page webviews
│   ├── src/browser_agent.js        # injected in-page snapshot/input/console/network bridge
│   ├── src/idle_timeout.rs         # auto-stops sessions idle past the configured timeout
│   ├── src/pr_poll.rs              # periodic PR-state fetch per remote worktree
│   ├── src/conflict_poll.rs        # periodic `origin` fetch to keep conflict state fresh
│   ├── src/watch.rs                # watches project worktree roots for external changes
│   ├── src/worktree_watch.rs       # watches the currently-selected worktree for Changes pane
│   ├── build.rs                   # tauri_build::try_build + AppManifest (see below)
│   ├── capabilities/default.json  # ACL: core/opener/notification defaults (dev/local window)
│   └── tauri.conf.json            # productName "corral", id dev.corral.desktop
└── src/ + index.html              # React 19 + Zustand + Tailwind v4 frontend (Vite)
    ├── App.tsx                     # 3-pane shell: Sidebar, MainPane, ChangesPane
    ├── store.ts                    # Zustand store — single source of frontend state
    ├── components/                 # Sidebar, MainPane, ChangesPane, RunPanel,
    │                                #   ProvisionPanel, SettingsPanel, TerminalHost
    │                                #   (xterm.js), DiffViewer, dialogs, etc.
    ├── hooks/                      # useGlobalShortcuts, useHoverIntent, useDialogRef
    └── lib/                        # tauri.ts invoke wrappers, actions, event listeners
                                     #   (session/run/provisioning/worktree-watch/browser-preview/browser-state)
```

- **src-tauri package** is `corral-app`; its lib is `corral_app_lib`
  (`main.rs` calls `corral_app_lib::run()`).
- **Frontend ↔ backend**: `invoke()` calls Tauri commands; the backend streams
  agent/run/provision output and worktree-watch/status changes via
  `session-output`, `session-exit`, `session-status`, `run-output`,
  `provision-output`, `worktree-watch`, `browser-preview`, and `browser-state`
  events. The browser window is a separate local `browser.html` entry whose
  chrome page (tab strip/address bar) is the only webview with browser
  permissions; page webviews are remote and get no capability.

### Conventions

- Main repos live wherever they're registered; worktrees go under a
  configurable root (default `~/corral`, `Settings.worktrees_root`) at
  `<worktrees_root>/<project>/<feature>`, branch `feat/<feature>`.
- `AGENTS.md` and the `CLAUDE.md` shim are tracked, so git ships them into
  every worktree. Worktrees also symlink `.claude` back to the main repo's
  `.claude` so settings/permissions are shared instead of drifting per-worktree.
- Removing a worktree = `git worktree remove` then `git branch -d` (safe delete).
- Editor is Zed via `zeditor -n <path>` (this system's `zed` symlink at
  `/usr/local/bin/zed` is stale/unreliable — `zeditor` on `/usr/bin` is the
  real CLI entry point).
- Sessions are keyed `<project>/<feature>` — one agent per worktree.

## Build & run

There's a `justfile` — `just --list` for the full set. Highlights:

```fish
pnpm install     # frontend deps + Tauri CLI (uses pnpm, not npm), one-time
just run         # build the frontend once, then run the desktop app
just check       # cargo check --workspace
just build       # fast standalone binary, real cfg(not(dev)) path — see note below
just bundle      # full pnpm tauri build (release + .deb/.rpm/.AppImage), slow
just cli list    # exercise the CLI
```

> **Prefer `just run` over `pnpm tauri dev`.** A child process of the Vite dev
> server leaves the terminal in the DEC line-drawing charset on exit, so typing
> afterward comes out as gibberish (`stty sane` alone doesn't fix it — the
> charset needs `\e(B`). Manual recovery:
> `printf '\e(B\e[m\e[?25h\e[?1049l\e[?2004l'; stty sane` (or just `reset`).

> **Testing a new config setting? Use `just run-dev`.** It builds and runs the
> app against an isolated profile — its own config dir
> (`~/.config/corral/profiles/dev`) and data dir
> (`~/.local/share/corral/profiles/dev`) — seeded on first run from your real
> `settings.json` and `projects.json`, so the dev instance starts with your
> projects/worktrees and current settings, but anything you change can't
> clobber the live app's files (the running app keeps using the unscoped
> dirs). Pick a different profile with `just run-dev <name>`; delete its dir
> to start over. Under the hood this sets `CORRAL_CONFIG_DIR`/`CORRAL_DATA_DIR`
> (see `corral-core::paths`), which the `corral` CLI honors too — e.g.
> `CORRAL_CONFIG_DIR=~/.config/corral/profiles/dev corral list`. The override
> is corral-only, so the agent CLIs corral spawns still read their own real
> config (unlike redirecting `XDG_CONFIG_HOME`/`XDG_DATA_HOME`, which would
> also hide opencode's config from the dev instance).

> **`cargo build`/`cargo build --release` alone do NOT produce a real bundled
> build — use `just build`/`just build-release`/`just bundle` instead.**
> Tauri's `cfg(dev)` vs `cfg(not(dev))` split is driven by the `tauri` crate's
> own `custom-protocol` Cargo feature, which only `tauri-cli` (`pnpm tauri
> build` / `pnpm tauri dev`) sets automatically. A plain `cargo build`
> compiles the `cfg(dev)` branch regardless of `--release`, which tries to
> load the Vite dev server URL — if that's not running, you get a blank white
> window with "Operation was cancelled" that looks like a webview bug but is
> actually just a dead `localhost:1420`. `just build`/`just build-release` add
> `--features tauri/custom-protocol` for fast iteration on the real
> production code path without the full bundler; `just bundle` runs the real
> `pnpm tauri build` release artifact.

## Releasing

The version lives in **three files that must stay in sync** (all currently
`0.2.0`), and nothing ties them together automatically — bump them via the
script, not by hand:

- `Cargo.toml` → `[workspace.package].version` (feeds all three crates via
  `version.workspace = true`)
- `package.json` → `"version"`
- `src-tauri/tauri.conf.json` → `"version"` (what the bundled app + the
  footer's `vX.Y.Z` read via `@tauri-apps/api/app` `getVersion()`)

`Cargo.lock` refreshes itself (the script runs `cargo metadata` for that, so
`cargo build --locked` on the tag stays reproducible). The `"0.1.0"` strings
in `App.test.tsx` / `Sidebar.test.tsx` are just `getVersion` mocks and don't
need bumping.

Release flow:

```fish
just bump patch     # or minor / major — edits the 3 files, refreshes
                    # Cargo.lock, commits, and tags vX.Y.Z locally
git push origin HEAD
git push origin vX.Y.Z   # pushes the tag, which triggers the Release workflow
```

The `.github/workflows/release.yml` workflow runs on `v*` tag pushes, builds
the Linux bundles, and publishes them in one GitHub Release:

- **Linux** (`ubuntu-latest`): `.deb` / `.rpm` / `.AppImage`.

The tag is re-verified against the Cargo.toml version before bundling. Use
`--no-commit` / `--no-tag` on the script to stop short (e.g.
`just bump patch --no-tag`).

**macOS is built by hand, not in the release workflow.** macOS runners are the
one standard runner GitHub does *not* make free on a public repo (a 10×
multiplier against an included allowance a GitHub Free user account doesn't
have), so a `macos-14` job dies at scheduling with zero steps run — which
takes the whole run down to `cancelled` and stops Linux publishing, since
`publish` needs it. There is no free tier to wait for. To ship a `.dmg`:

```fish
# On a Mac, from the tagged checkout:
pnpm install
just bundle                      # host arch — arm64 on Apple Silicon
gh release upload vX.Y.Z target/release/bundle/dmg/*.dmg
```

The released `.dmg` is **arm64**, not universal: Apple stopped selling Intel
Macs in 2023. An arm64-only app fails loudly on an Intel Mac (it won't launch),
which is preferable to a universal app with a thin CLI — that launches under
Rosetta and then silently loses every hook-driven feature (`bad CPU type in
executable`). An Intel user can build a fully fat dmg with `just universal`,
which lipos both the app and the bundled CLI and prints `lipo -archs` for every
nested executable.

There is no macOS job in the release workflow at all — the `.dmg` is always a
manual build on a Mac. macOS signing + notarization is opt-in via repository
secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`); for a
local `just bundle` they're read from the environment, and with none set the
dmg is unsigned and needs Gatekeeper's right-click → Open on first launch.
Because the dmg is only built locally, nothing in CI packages the macOS
sidecar — if `stage-cli-sidecar.sh` or `build.rs` breaks only on macOS, the
manual build is where it shows up.

There is deliberately **no macOS job in `ci.yml`**: it would never run, and a
green check that silently means "macOS was never compiled" is worse than none.
`cfg(target_os = "macos")` code is therefore only compiled when someone builds
on a Mac — keep that in mind when touching `process.rs`, `caffeinate.rs`, or
`shell_env.rs`.

## Gotchas / hard-won lessons

- **Long-running background work must use `corral awake`.** Corral's own
  sleep inhibitor follows the in-memory session status, so a worker detached
  from the harness (`setsid`, `nohup`, a background shell that outlives a
  restart) is invisible to it. The onboarding prompt tells agents to wrap the
  long-running command itself in `corral awake -- <command>`, which holds
  `systemd-inhibit` on Linux, `caffeinate` on macOS, or
  `SetThreadExecutionState` on Windows for the worker's lifetime. Wrap the
  worker, not a launcher that exits immediately.
- **Credential storage is `corral-core::secret`, not `Settings`.** Anything
  secret goes through `secret::set/get/clear` with the config dir, never into
  `settings.json` (which is re-serialized on every toggle and snapshotted to
  `.bak`). Every store file and backup is written `0600`.
- **The Linux keyring backend must never run on a thread with a Tokio runtime
  already active.** `zbus`'s blocking API blocks the calling thread on its own
  process-wide runtime, so calling it from a Tauri command panicked on the very
  first `invoke()` — "Cannot start a runtime from within a runtime" when the
  call happens on a Tokio worker, "Cannot drop a runtime in a context where
  blocking is not allowed" (`tokio/src/runtime/blocking/shutdown.rs`) when a
  runtime built on that thread is dropped. `Runtime::new()` also *inherits*
  the ambient handle, so wrapping the call in `new_current_thread` didn't fix
  it, and neither did `spawn_blocking`. `secret.rs::on_keyring_thread` now
  runs every OS-store call on a fresh `std::thread` (no ambient runtime, and
  nowhere near a runtime worker that a keychain prompt would stall).
  `secret::tests::os_store_calls_survive_a_tokio_worker_thread` pins this —
  it fails with the exact panic above if the guard is removed. Any *new* call
  into `zbus` (or `secret_service`) must go through a thread like that, not
  straight from a command, an `async` block, or a spawned task.
- **`create_project` (and anything else doing real work) must be `async` +
  `spawn_blocking`.** A non-`async` `#[tauri::command]` body runs inline on
  the GTK main thread (`create_ipc_handler` → `on_message` →
  `run_invoke_handler`), so disk/git/keychain work freezes the whole UI.
  `create_project` was one of these: `git init` + commit + a settings reload
  (which reads the keychain) ran inline, which is what exposed the panic above.
  The sweep since then covers the other settings/credential commands
  (`get_settings`, `save_settings`, `linear_key_storage`), project lifecycle
  (`add_project`, and `remove_project` — whose `worktree::list()` is the same
  full `git status` scan `list_worktrees` was converted for), and
  `rename_worktree`.
  - **An `async` command's body is a Tokio task, so `spawn_blocking` is not
    optional there either** — anything blocking left in the body stalls a
    pool worker for its duration, and on Linux that includes every
    `secret::*` call (a Secret Service round trip that can wait on an unlock
    prompt). `save_settings` therefore puts its `sync_caffeinate` reload
    *inside* the `spawn_blocking` closure rather than after it, and
    `remove_project`'s per-worktree teardown (killing children, then the
    flock-guarded session/note stores) is a `spawn_blocking` body of its own.
    That's why the three registries (`SessionRegistry`, `provision::Registry`,
    `run::Registry`) are cheaply cloneable via `handle()`: a `State` borrow
    can't cross into the closure, a clone can.
  - **`kill_session_best_effort` is reachable from a blocking thread** and its
    browser-host teardown is a window operation, so it hops to the main thread
    with `run_on_main_thread`. `remove_worktree` was already calling it from
    an `async` command before that fix.
  Synchronous **by design**: the per-keystroke session hot
  path (`write_session`/`resize_session`/`attach_session`, where a thread hop
  costs more than the work), and the `browser_*` window commands (window ops
  must be on the main thread). Synchronous **and still wrong** — next
  candidates, all of which do real I/O inline: `branch_changes`, `file_diff`,
  `revert_file`, `list_worktree_dir`, `read_worktree_file`, `send_notification`
  (blocking DBus), `memory_usage` (polled `/proc` walk), `save_pasted_image`,
  `save_text_attachment`, `resolve_run_command`, `list_run_targets`.
- **Clearing per-worktree state goes through
  `worktree::clear_worktree_state(keys)`.** All three stores (session ids,
  notes, titles) have to be cleared together — `remove` and project removal
  both learned that the hard way, once for `--resume` and once for titles
  surviving a project re-add. It clears each store in one locked write via
  `KeyedStore::remove_keys`, so don't reintroduce a per-worktree `set(key, "")`
  loop: that's one flock + full re-read/re-serialize per key.
- **Rust changes need a full app restart.** Vite HMR only reloads the
  frontend; new/changed Tauri commands or Rust logic won't take effect until
  the binary rebuilds (e.g. `just run`). Frontend-only changes hot-reload
  live.
- **pnpm build scripts:** esbuild's postinstall is allow-listed in
  `pnpm-workspace.yaml` (`allowBuilds: { esbuild: true }`). pnpm 11 moved this
  out of `package.json`.
- **Every Tauri command must be registered** in `generate_handler![...]` in
  `lib.rs` *and* usually needs an ACL permission in `capabilities/default.json`.
- **Notifications on Linux:** do NOT gate `sendNotification` on the JS
  permission API (`isPermissionGranted`/`requestPermission`) — on Linux desktop
  it reports "not granted" even though the DBus send works. Request permission
  best-effort, then always attempt the send. The app logo is embedded and
  written to the cache dir via the `notify_icon` command.
- **git is shelled out**, not libgit2 — keeps deps small and matches the rest
  of corral's argv-vector `Command` style. `git.rs::git()` errors on nonzero
  exit; `git_diff()` tolerates exit 1.
- **PTY sessions:** drop the pty slave after spawn so the reader sees EOF on
  exit. Sessions keep a 256KB scrollback for terminal re-attach.
- **Every xterm `FitAddon` parent (the element passed to `term.open()`) must be
  `box-sizing: content-box`** — corral's `src/styles.css` provides
  `.xterm-fit` for exactly this, and all three terminals (the agent pane in
  `session.ts`, `RunPanel`, `ProvisionPanel`) carry it. `FitAddon` sizes the
  terminal from `getComputedStyle(parent).height`/`.width` (xterm.js#1283); for
  a `border-box` element those computed values *include* the element's padding,
  which FitAddon never subtracts. Tailwind preflight makes every element
  `border-box`, so a padded terminal container (the agent pane's `py-1.5
  px-2`, the logs' `p-1`) made FitAddon think it had more room than the content
  box really did — rounding the row/column count up by one whenever the size
  landed near a cell boundary, spilling the last row past the bottom edge (the
  tell was pi's bottom status line getting clipped). The rendered layout is
  unchanged for the `inset-0` panes. If a new terminal container uses padding
  and looks a row short/tall, add `.xterm-fit` — don't "fix" it by trimming
  padding.
- **Gate platform-specific workarounds on `target_os`, not just `dev`.**
  Now that macOS builds ship too, a bare `cfg(not(dev))` means "every release
  build on every OS." The localhost-plugin content-serving workaround is
  webkit2gtk-only, so it's `#[cfg(all(not(dev), target_os = "linux"))]` (and
  the Linux webview gets `WebviewUrl::External` while dev/macOS use the normal
  `WebviewUrl::App`). The sleep inhibitor (`systemd-inhibit` vs `caffeinate`)
  and the process-tree RSS walk (`/proc` vs a `ps` snapshot) are likewise
  per-platform. The `[patch.crates-io]` tauri/wry forks can't be target-gated
  (Cargo has no per-target `[patch]`), so they apply everywhere, but their
  fixes are Linux-only at runtime.
- **A GUI-launched corral doesn't inherit the user's shell environment.**
  macOS apps started from Finder/Dock/`open` get launchd's minimal env, not
  `~/.zprofile`/`~/.zshrc` — so `PATH` has no Homebrew/pnpm/nvm entries and
  exported vars (`ANTHROPIC_API_KEY`, …) are missing, which is what made Run
  commands fail on a fresh Mac install. `corral_core::shell_env` runs the
  login+interactive shell once at startup (`login_shell()` reads the passwd
  entry like Terminal.app, `-l -i -c`, `env -0` with a newline fallback for
  macOS < 13) and merges the result into corral's own process environment
  *before* the Tauri builder starts any threads — `std::env::set_var` is
  unsafe in edition 2024, so that ordering is load-bearing. Existing vars win
  (so `CORRAL_CONFIG_DIR` from `just run-dev` survives); `PATH` is the one
  exception, taking the shell's value with app-only entries appended. Don't
  swap this for `fix-path-env`'s `fix_all_vars()`, which overwrites
  `CORRAL_CONFIG_DIR` and friends. Only wired up on macOS; Linux `.desktop`
  launches have the same gap and could enable the same call.
- **The `corral` CLI ships as a Tauri sidecar, and hooks reference it by
  absolute path.** `bundle.externalBin` plus
  `scripts/stage-cli-sidecar.sh` (run by `build.beforeBundleCommand`) stage
  `src-tauri/binaries/corral-<target-triple>`; the bundler copies it to
  `Contents/MacOS/corral` in the `.app` (and signs it with the app) and to
  `/usr/bin/corral` in the Linux packages. `tauri-build` requires the sidecar
  to exist at **compile** time and errors out when it is missing, so plain
  `cargo check` in a fresh checkout fails before the bundler runs — the
  `justfile` therefore exports `TAURI_CONFIG` with `bundle.externalBin` emptied
  for `just check` / `just build` (which also build the CLI into the same
  profile dir, so `corral_cli_path` finds it as a sibling). Never work around
  this by writing a stub into `src-tauri/binaries/`: `tauri-build` would copy it
  next to a dev `corral-app`, where the sibling rule prefers it over a real CLI
  and every `corral` call exits 127. Do **not** switch
  this to `bundle.macos.files`: Tauri copies those under `Contents/` but does not add
  them to its signing list, so a nested Mach-O stays unsigned and breaks
  notarization/`codesign --verify`. `bin::corral_cli_path()` resolves the
  executable as a sibling of `current_exe` (dev `target/debug/corral`, bundled
  `Contents/MacOS/corral`, `/usr/bin/corral`), falling back to `$PATH`;
  `start_session` bakes that absolute path into Claude hook commands (the hook
  shell on macOS never has the bundle dir on `PATH`) — but only while Claude is
  the harness; switching to OpenCode/Pi removes them again
  (`hooks::HookCli::Remove`) — and prepends its directory
  to the agent PTY's `PATH` so `corral rename/list/done/awake` work.
  `bin::hook_cli_path()` returns `None` inside an AppImage, whose mount path
  changes every launch, and `merge_hooks` replaces a stale managed entry rather
  than accumulating another.
  **The staging script must stay bash-3.2-safe**, because on a Mac it runs
  under `/bin/bash` 3.2 (Apple's last GPLv2 build) even though the shebang says
  `env bash` and `just`/`fish` may hand it 5.x: under `set -u`, 3.2 aborts on
  `"${empty_array[@]}"` with `<name>[@]: unbound variable` where bash 4.4+
  expands it to nothing, so `locked=(); locked=(--locked)` made every macOS
  `just bundle` die at the sidecar step (`line 39: locked[@]: unbound
  variable`) — after a full release compile, so it was slow to hit and slow to
  diagnose. Keep optional args in plain strings and word-split deliberately (see
  `run_cargo`), never in possibly-empty arrays; same for any script a macOS
  build hook runs. Nothing in CI catches this — there's no macOS job — so it
  only shows up in the manual `.dmg` build. Don't use `resource_dir()`/
  `Contents/Resources` for this — sidecars live in `Contents/MacOS`.
- **`just bundle` builds the CLI itself now.** The freedesktop packages used to
  need a separate `cargo build -p corral-cli` before `pnpm tauri build`
  (`bundle.linux.*.files` pointed at `target/release/corral`). The
  before-bundle hook stages the sidecar for the actual target triple, so a
  universal build gets `corral-universal-apple-darwin` automatically. Keep
  `just build`'s CLI build: a dev `just run` needs `target/debug/corral`.
- **Bundled builds need webkit2gtk workarounds on this system (CachyOS
  rolling release, `webkit2gtk-4.1 2.52.x`).** Two separate upstream bugs,
  both worked around, both required to get past a blank white
  "Operation was cancelled" window in non-dev builds:
  1. **IPC `Origin` header bug:** webkit2gtk sends a missing/unparseable
     `Origin` header (literal `"null"`) on Tauri's internal `ipc://` fetch,
     which Tauri's `crates/tauri/src/ipc/protocol.rs` hard-rejects — so
     *every* `invoke()` 500s. Fixed via a one-line patch to a vendored fork
     of `tauri` in a local fork (`nnthnn/tauri-patched`, tag `tauri-v2.11.4`,
     branch `corral-origin-patch`), wired in via `[patch.crates-io]` in the
     workspace `Cargo.toml`. The patch falls back to the app's local protocol
     URL when Origin is missing/unparseable, instead of rejecting the
     request — safe here because corral never loads remote content.
  2. **Custom-protocol navigation bug:** separately, top-level page loads
     through Tauri's default `tauri://localhost` custom-protocol content
     serving never complete on this webkit2gtk (window stays stuck on
     `about:blank`) — real HTTP navigation works fine though (confirmed via
     the Vite dev server). Worked around by serving the bundled
     frontend over `tauri-plugin-localhost` (a real `http://localhost:<port>`
     origin) instead of the custom protocol, for non-dev builds only — see
     the `#[cfg(not(dev))]` branch in `lib.rs::run()`.
  - Serving over `http://localhost` makes the window a "remote" origin for
    Tauri's ACL. `capabilities/default.json` covers both the local
    (custom-protocol/dev) and remote (localhost-plugin) cases with one file,
    via a `"remote": {"urls": ["http://localhost:*"]}` wildcard — no runtime
    capability code needed in `lib.rs`.
  - Adding an `AppManifest` in `build.rs` (needed to auto-generate
    `allow-<command>` permissions for corral's own `#[tauri::command]`s, since
    remote origins never get the implicit local-window command allowlist)
    has a global side effect: it makes Tauri enforce ACL strictly for *all*
    windows, including local/dev ones, which previously bypassed ACL entirely
    when the app had no manifest at all. So `capabilities/default.json` needs
    the same `allow-*` permissions too, even though dev windows are local.
    **When adding a new `#[tauri::command]`,  update three places:**
    `generate_handler![...]` in `lib.rs`, the `commands` list in `build.rs`,
    and `allow-<command>` in `capabilities/default.json` — otherwise it fails
    with "not allowed. Plugin not found" in *both* dev and bundled builds.
  - `devtools` Cargo feature is kept on permanently (not just for this
    debugging session) since bundled-build issues need Inspect Element to
    diagnose, and `right-click → Inspect` requires it compiled in ahead of time.
  - If this system's webkit2gtk ever ships a fix and things break again from
    the workarounds themselves, the first thing to try is reverting to the
    plain custom-protocol setup and seeing if the original bugs still
    reproduce.
- **Every keystroke in the terminal felt slow — Tauri's Linux IPC transport,
  not rendering.** Tauri always tries the `ipc://` custom-protocol fetch first
  on Linux and only falls back to the faster `postMessage` transport if that
  fetch throws — which it never does on modern webkit2gtk, so Linux never got
  the fast path. The custom-protocol fetch simulates a full HTTP request
  through WebKit's network stack on *every* `invoke()` call, adding 10-150ms
  of latency even for a few bytes of payload (measured on xterm.js keystroke
  writes to `write_session`; the same Tauri+xterm.js stack has no such lag on
  macOS). Before finding this, ruled out: xterm.js's default DOM renderer
  (added the GPU-accelerated `@xterm/addon-webgl` — no improvement, so
  rendering wasn't the bottleneck) and `app.emit()`'s output-side latency
  (instrumented, confirmed consistently <2ms). Fixed with another one-line
  patch to the same vendored `tauri-patched` fork —
  `crates/tauri/scripts/ipc-protocol.js`, forcing
  `canUseCustomProtocol = false` unconditionally on Linux instead of only on
  fetch failure. Cut `write_session` round-trip from 10-150ms to ~0-16ms in
  testing. If diagnosing similar lag again: instrument with
  `performance.now()` around the `invoke()` promise *and* around the next
  matching output event, to tell IPC latency apart from child-process/render
  latency before assuming it's rendering.
  - **Side effect of that patch:** once Linux always uses `postMessage`, IPC
    requests stopped carrying an HTTP `Origin` header at all — instead, wry's
    `attach_ipc_handler` (`webkitgtk/mod.rs`) stamps each request with
    `WebView::uri()` read at message-received time. webkit2gtk can still
    report `about:blank` for a short window after the real page's DOM has
    already loaded (a timing inconsistency between the DOM-ready signal and
    the URI property), so early `invoke()` calls — e.g. `initNotifications()`/
    `loadAll()` on `DOMContentLoaded` in `main.ts` — got ACL-rejected as
    coming from an unrecognized `about:blank` origin, in *both* dev and
    bundled builds. Fixed with a matching one-line patch to a second vendored
    fork, `nnthnn/wry-patched` (tag `wry-v0.55.1`, branch
    `corral-origin-patch`, wired in via the same `[patch.crates-io]` block):
    fall back to the webview's originally-configured target URL
    (`WebViewAttributes::url`, captured before the message handler closure)
    whenever `uri()` reports `about:blank`.
- **Release-only blank terminal when the agent is opencode (Claude works).**
  Vite's *esbuild* minifier used to mangle `@xterm/xterm` 6.0.0's ESM build: its
  `requestMode` enum init (`let r; (V => (V[...] = ...))(r ||= {})`)
  constant-folds into `(void 0 || (i = {}))`, dropping the `let r;`
  declaration. In a minified build that threw `ReferenceError: Can't find
  variable: i` on the first `term.write()` containing a DECRQM mode query
  (`CSI ? Ps $ p`) — the terminal stayed blank. Claude never sends DECRQM, so
  it worked in both dev and release; opencode's attach TUI does, so it broke in
  release only (dev is unminified). The old fix aliased `@xterm/xterm` to its
  CJS build (`lib/xterm.js`), which was also a bigger, webpack-bundled artifact
  (~150 kB larger in the minified chunk). **No longer needed under Vite 8's
  Rolldown/Oxc pipeline**, which preserves the `let n;` declaration (`xterm`'s
  CJS alias was removed in favor of the smaller ESM build). If the minifier
  ever changes again, verify by checking the minified `dist/assets/xterm-*.js`
  for `void 0||(x={})` (or that `requestMode(e,t){let n;(e=>…)(n||={});` is
  intact); re-introducing the CJS alias is the fallback if it regresses.

## Frontend conventions

- **Always use `clsx` for `className`, never string concatenation.** Enforced
  by a Biome GritQL plugin (`biome-plugins/no-classname-concat.grit`) that
  flags `className={... + ...}`.
- **Biome supports custom lint rules via GritQL plugins** (`.grit` files,
  wired in via `biome.json`'s `"plugins"` array). Used for two project-specific
  rules: no string-concat `className` (above), and no gating
  `sendNotification()` behind `isPermissionGranted()` (see the notification
  gotcha above) — `biome-plugins/no-gated-notification.grit`.
- **Biome auto-enables React-specific lint rules** (`useExhaustiveDependencies`,
  `useHookAtTopLevel`, `noArrayIndexKey`) because it detects `react`/`react-dom`
  in `package.json` — no config needed, don't re-enable manually.
- **Component styling: colocate single-owner styles, keep shared ones global.**
  Classes used by only one component go in a sibling `Component.module.css`;
  classes shared across components (`.btn`, `.status-chip`, `.tab`, etc.) stay
  in `src/styles.css` as the app's small in-house design system. A CSS module
  needs `@reference "../styles.css";` at the top to use `@apply`/theme tokens,
  and `:global(.foo)` to reference classes that stayed in the global sheet.
  Requires `"css": { "parser": { "cssModules": true } }` in `biome.json`
  (otherwise Biome flags `:global()` as an unknown pseudo-class) and
  `src/vite-env.d.ts` with `/// <reference types="vite/client" />` (needed
  for TS to recognize `*.module.css` imports — wasn't present until the first
  CSS module was added).
- **Testing CSS Modules classes:** Vitest doesn't hash-process CSS Modules by
  default (`css.modules` off) — it returns a proxy, not the real scoped name.
  Import the same `*.module.css` in the test and assert against
  `styles.xxx`, never a hardcoded class-name string.

## Environment

- Tauri **v2**; needs `webkit2gtk-4.1` + `libsoup-3.0` (present).
- Desktop is **Hyprland (Wayland) + mako** (notification daemon), *not* XFCE as
  the global `~/.claude/CLAUDE.md` says. `notify-send` works.
- Package manager: **pnpm**. Shell: **fish**.
- **Rust compiles are cached across worktrees via `sccache`,** wired in at
  `~/.cargo/config.toml` (`rustc-wrapper = "sccache"`) — global/user-level, not
  repo-tracked, since it applies to every `cargo` invocation on this machine
  regardless of which worktree runs it, and a repo-tracked config would break
  builds on any checkout without sccache installed. Each worktree still has
  its own `target/` (so builds stay independently parallel — no shared
  target-dir lock serializing simultaneous builds across sessions), but
  identical compiler invocations (mainly shared deps like `tauri`/`wry`) hit
  sccache's cache instead of recompiling. Verified: full `cargo clean` +
  `cargo check -p corral-core` dropped from 23.6s cold to 8.1s with a fresh
  cache (50% hit rate). Check `sccache --show-stats` to confirm it's active;
  if a new machine/worktree seems to skip caching, install sccache first
  (`paru -S sccache`).
- **Day-to-day build: use `just build`/`just run`, not `pnpm tauri build`.**
  `pnpm tauri build` (`just bundle`) is the full release bundler —
  `.deb`/`.rpm`/`.AppImage` packaging, slow. `just build` compiles the
  frontend once then does a real (non-cfg(dev)) debug `cargo build` without
  the bundler; `just check` is fastest when you just need compile signal, no
  binary.
- **Focusing a specific window (e.g. to screenshot one of several running
  corral instances) needs Hyprland's new Lua dispatch API** — all `hyprctl
  dispatch` calls on this system must use it, not the legacy string syntax:
  `hyprctl dispatch 'hl.dsp.focus({ window = "address:0x..." })'`. Get the
  address from `hyprctl clients -j`. Multiple corral windows/instances can be
  running at once (e.g. across worktrees) — always focus the intended one
  before screenshotting or sending synthetic input, and be aware that
  focusing a window steals focus from whatever the user is currently looking
  at, which can interrupt them mid-conversation if they're driving a Claude
  Code session from another corral window. Check before doing this if it
  might be disruptive.
  - **Focus and capture must be one shell command, not two tool calls.**
    Hyprland (dwindle layout, no panning plugin) can reflow/reposition
    windows as a side effect of a focus dispatch — a window that reports
    off-screen coordinates (e.g. `at: [1917, 37]` on a 1920px-wide monitor,
    almost entirely clipped) can snap back on-screen the moment it's
    focused. If `hyprctl dispatch` and `grim -g ...`/`hyprctl clients -j`
    run as separate tool calls, anything between them (another tool call,
    a pause) risks capturing stale geometry or an unfocused/blank window.
    Chain them in a single `Bash` call: `hyprctl dispatch 'hl.dsp.focus(...)'
    && grim -g "$(hyprctl clients -j | jq -r '...')" out.png` (re-query
    geometry *after* focusing, in the same command) — never split
    focus-then-screenshot across turns.

## Working agreements

- **Commit only when asked.** No `Co-Authored-By` trailers.
- One feature per branch: `feat/<name>`; fast-forward merge into `main`; delete
  the merged branch. Small housekeeping can go straight to `main`.
- Verify before declaring done: `cargo check --workspace` + `pnpm build`.
- **The user tests and visually inspects UI-touching work themselves before
  it merges or goes to a PR** — don't offer/execute a merge or PR for such
  work until they've had a chance to click around a running instance first.
  When work is ready, run a dev instance (`just run`, backgrounded via the
  agent's own Bash tool) so they can test it, and wait for their go-ahead
  before offering the finishing-a-development-branch options. Automated
  tests passing isn't a substitute for this.
- GitHub remote: `github.com/nnthnn/corral`, with CI (`.github/workflows/ci.yml`)
  running `just ci` on push to `main` and on PRs.

## Status

Bugs, ideas, and follow-up work go in Linear (Dev team → "Corral" project),
not in a repo file. As of the latest work, implemented:

- **Worktree management** (GUI + CLI): create / rename / remove, per-project
  provisioning rules (files to copy, one-shot setup command run via the
  Provision panel/tab), and a git/file-system watcher so external changes
  (e.g. a `corral rename` run from inside an agent's own PTY) show up live.
- **Project creation** (GUI + CLI): "New project" creates a folder, `git init`s
  it with one empty commit, registers it, and opens its first worktree — so the
  agent does all the scaffolding in a normal branch/worktree and every git
  feature (diff, merge, PR) works from the first session. `Add project` also
  accepts an empty folder and initializes it the same way. An optional
  "what are you building?" brief is pasted (not submitted) into the first
  agent's input.
- **Agent sessions**: a harness CLI in a PTY per worktree — Claude Code,
  OpenCode (v2 current, v1 legacy), or Pi, chosen via the harness setting —
  streamed to an embedded xterm.js terminal (GPU-rendered via
  `@xterm/addon-webgl`, `Ctrl/Cmd +/-/0` font zoom, scrollback re-attach),
  auto-`--resume` of the last session id (tagged per harness), real
  waiting-vs-working status (Claude Code hooks; OpenCode's SSE stream via
  `opencode_events.rs`; Pi's bundled extension via `pi.rs`) instead of PTY
  output guessing, and an idle-timeout auto-stop.
- **OpenCode browser tools (embedded)**: each OpenCode v2 session gets an
  `experimental.browser` attachment (`browser_host.rs`), on by default and
  gated by the `browser_enabled` setting (off = no threads, no SSE
  subscription, tools unavailable). The browser itself is a corral-owned
  WebKit window (`browser_window.rs`) with a tab strip + address bar and one
  child page webview per OpenCode tab; tab titles are refreshed by the chrome
  page polling `browser_refresh_titles`. The injected `browser_agent.js`
  provides snapshots/refs, click/fill/select/check/press/scroll, `evaluate`,
  screenshots (WebKit snapshot, bounded full-page height), console capture,
  fetch/XHR network metadata/bodies, and auto-answered JS dialogs. The host is
  torn down by every session-lifecycle path (`kill_session_best_effort`,
  `close_session`, the stale-session prune, and app exit), not just by its own
  threads noticing a dead server. `browser.preview` opens files in the
  existing File tab. Chromium-only operations (`trace.*`, `cpu.*`, `heap.*`,
  `lighthouse`) and downloads/capture files are reported as `unsupported`.
- **3-pane shell**: sidebar (projects → worktrees) with a status dot, exit
  animation on removal, and a fading 🔔 attention cue; center pane with
  Agent / Run / Setup / Diff / File / PR tabs; right-hand branch-diff Changes pane
  with click-to-view diffs. Keyboard-driven: `Ctrl+K` command palette,
  `Ctrl+1–9` worktrees, `Ctrl+Tab` tabs, `Ctrl+[`/`]` prev/next.
- **Run panel**: a long-lived per-worktree "Run" command (e.g. `pnpm dev`)
  in its own tab, with best-effort auto-detection of the project's dev
  command and one-click target chips: the justfile's public recipes when the
  worktree has one, else the `package.json` scripts (lifecycle hooks
  filtered), run through the detected package manager.
- **Files**: a File tab with gitignore-aware tree browsing and image / SVG /
  Markdown previews.
- **Git/PR integration**: fast-forward merge of a worktree's branch into its
  base, and push + open/update a GitHub PR via `gh`, with live CI checks.
- **Linear tickets + attachments**: the header's ticket button (also in the
  command palette) opens a picker on recently-updated tickets, searches Linear
  with a personal API key from Settings, and attaches the chosen ticket's body
  to the active session — written to a temp file via `save_text_attachment` and
  referenced by path, the same pattern the PR/checks "Add to chat" actions use.
  Dropping OS files on the terminal (or pasting a clipboard image) inserts
  their quoted paths.
  The API key is a credential and is handled differently from every other
  setting: it is `#[serde(skip)]`, never crosses the IPC boundary, and the
  `linear_*` commands read it backend-side. `corral-core::secret` stores it in
  the OS credential store (Secret Service via zbus on Linux, Keychain on
  macOS, Credential Manager on Windows) and falls back to a `0600` file in the
  config dir when none is available — which is routine on Linux, so the
  fallback is load-bearing, not an error path. `CORRAL_SECRET_FILE_ONLY=1`
  forces the file backend (tests use it so they never touch a real keychain).
  Support is deliberately Linux/macOS/Windows only; other Unix targets get the
  file fallback.
- **Per-project settings**, per-worktree free-text notes (sidebar subtitle,
  untracked), and desktop notifications (with cowboy flair 🤠). Provisioning
  files and the setup command are configured per project; the setup command
  runs automatically on worktree creation unless the project's
  `auto_setup` toggle is off (Setup tab's Re-run still runs it on demand).
- **Terminal font picker**: Settings lists the machine's monospaced families
  (`commands/fonts.rs`, fontdb-backed, cached per process) in the font-family
  `datalist`, and warns when a configured family isn't installed. Blank still
  means the built-in monospace stack; an unavailable family falls back to it
  instead of WebKit's proportional default, which would break xterm's grid.
