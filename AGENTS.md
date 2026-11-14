# AGENTS.md — corral

> Project context for coding agents, committed to the repo. opencode reads
> this natively; Claude Code picks it up via the committed `CLAUDE.md` shim
> (`@AGENTS.md`). `IDEAS.md` stays git-ignored as a personal backlog. Since
> this file is tracked, keep it free of secrets and anything personal.

## What corral is

A **Linux-native Tauri desktop app** for running multiple parallel coding-agent
sessions, each isolated in its own git worktree. Each agent is a real CLI
process (Claude Code, OpenCode, or Pi) running in a PTY, shown in an embedded
terminal. There's also a `corral` CLI.

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
│   │   ├── checks.rs               # GitHub check-run status, PR comments, merge via `gh`
│   │   ├── caffeinate.rs           # systemd-inhibit while any session is running
│   │   ├── onboarding.rs           # system-prompt briefing for a freshly started agent
│   │   ├── names.rs                # auto-picked placeholder feature names
│   │   ├── settings.rs             # user app preferences (disk-persisted)
│   │   ├── bin.rs                  # resolve/validate agent CLI executables on PATH
│   │   ├── process.rs              # sum RSS across the process tree (from /proc)
│   │   ├── natural.rs              # numeric-aware string ordering
│   │   ├── json_store.rs           # shared JSON-blob-on-disk primitives
│   │   ├── keyed_store.rs          # generic keyed JSON store (~/.local/share/corral)
│   │   ├── watch.rs                # which fs events matter for a watched worktree
│   │   ├── log_err.rs              # "log at warn, continue" ext trait
│   │   └── error.rs
│   └── corral-cli/                # `corral` binary (clap): new / list / open / done /
│                                   #   rename / config / project add / awake / notify-hook
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

The `.github/workflows/release.yml` workflow runs on `v*` tag pushes, verifies
the tag matches the Cargo.toml version, builds the `.deb`/`.rpm`/`.AppImage`
bundles, and publishes them as a GitHub Release. Use `--no-commit` /
`--no-tag` on the script to stop short (e.g. `just bump patch --no-tag`).

## Gotchas / hard-won lessons

- **Long-running background work must use `corral awake`.** Corral's own
  sleep inhibitor follows the in-memory session status, so a worker detached
  from the harness (`setsid`, `nohup`, a background shell that outlives a
  restart) is invisible to it. The onboarding prompt tells agents to wrap the
  long-running command itself in `corral awake -- <command>`, which holds
  `systemd-inhibit` on Linux, `caffeinate` on macOS, or
  `SetThreadExecutionState` on Windows for the worker's lifetime. Wrap the
  worker, not a launcher that exits immediately.
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

See `IDEAS.md` for the backlog. As of the latest work, implemented:

- **Worktree management** (GUI + CLI): create / rename / remove, per-project
  provisioning rules (files to copy, one-shot setup command run via the
  Provision panel/tab), and a git/file-system watcher so external changes
  (e.g. a `corral rename` run from inside an agent's own PTY) show up live.
- **Agent sessions**: a harness CLI in a PTY per worktree — Claude Code,
  OpenCode (v2 current, v1 legacy), or Pi, chosen via the harness setting —
  streamed to an embedded xterm.js terminal (GPU-rendered via
  `@xterm/addon-webgl`, `Ctrl/Cmd +/-/0` font zoom, scrollback re-attach),
  auto-`--resume` of the last session id (tagged per harness), real
  waiting-vs-working status (Claude Code hooks; OpenCode's SSE stream via
  `opencode_events.rs`; Pi's bundled extension via `pi.rs`) instead of PTY
  output guessing, and an idle-timeout auto-stop.
- **OpenCode browser tools (embedded)**: each OpenCode v2 session gets an
  `experimental.browser` attachment (`browser_host.rs`). The browser itself is
  a corral-owned WebKit window (`browser_window.rs`) with a tab strip + address
  bar and one child page webview per OpenCode tab. The injected
  `browser_agent.js` provides snapshots/refs, click/fill/select/check/press/
  scroll, `evaluate`, screenshots (WebKit snapshot), console capture, and
  fetch/XHR network metadata/bodies. `browser.preview` opens files in the
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
- **Per-project settings**, per-worktree free-text notes (sidebar subtitle,
  untracked), and desktop notifications (with cowboy flair 🤠).
