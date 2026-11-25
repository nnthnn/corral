default:
    @just --list

# Type-check + bundle the frontend only (tsc + vite build)
frontend:
    pnpm build

# `bundle.externalBin` makes tauri-build copy the CLI sidecar next to the binary
# at compile time, and *fail* when it is missing — so a fresh checkout can't
# even `cargo check` until `just bundle` has staged one. Exporting an emptied
# `externalBin` for every recipe here skips that copy (build.rs adds a hint to
# the leftover error), and `build` / `build-release` compile the CLI into the
# same profile dir so `bin::corral_cli_path` finds the real thing as a sibling.
no-sidecar := '{"bundle":{"externalBin":[]}}'
export TAURI_CONFIG := no-sidecar

# Compile-check the whole Rust workspace (no sidecar needed)
check:
    cargo check --workspace

# Format Rust + frontend code in place
fmt:
    cargo fmt --all
    pnpm exec biome check --write .

# Check formatting + assist actions (e.g. import order) without writing (CI)
fmt-check:
    cargo fmt --all -- --check
    pnpm exec biome check .

# Lint Rust (clippy) + frontend (biome), warnings as errors
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    pnpm exec biome lint .

# Run Rust + frontend test suites
test:
    cargo test --workspace
    pnpm test

# Frontend coverage report (text summary + coverage/index.html)
coverage:
    pnpm test:coverage

# Visual test runner in the browser — watch mode, click-to-rerun, module graph
test-ui:
    pnpm test:ui

# Component preview gallery — opens preview.html on the Vite dev server,
# without the Tauri shell (no bootstrap()/invoke() calls)
preview:
    pnpm exec vite --open /preview.html

# Everything CI runs: formatting, lints, type-check, tests
ci: fmt-check lint check frontend test

# One-time: point git at the tracked .githooks/ dir (pre-commit auto-formats
# staged files and folds the changes into the commit, pre-push runs the full
# `ci`). Shared config, so one run covers every worktree of this repo.
install-hooks:
    git config core.hooksPath .githooks
    chmod +x .githooks/*

# Build a real standalone binary fast, skipping the tauri-cli bundler (see CLAUDE.md).
# The CLI is built into the same profile dir, so `bin::corral_cli_path` finds it
# as a sibling of `corral-app` (which is what the session PATH relies on).
build: frontend
    cargo build -p corral-app --features tauri/custom-protocol
    cargo build -p corral-cli

# Same as `build`, but --release
build-release: frontend
    cargo build -p corral-app --release --features tauri/custom-protocol
    cargo build --release -p corral-cli

# Full release build + platform bundling via tauri-cli — slow.
# `build.beforeBundleCommand` (scripts/stage-cli-sidecar.sh) builds the CLI for
# the target triple and stages it as a Tauri sidecar: Contents/MacOS/corral in
# the .app and /usr/bin/corral in the .deb / .rpm / .AppImage.
#
# NO_STRIP: tauri's linuxdeploy ships binutils 2.35, which faults on the
# `.relr.dyn` sections of modern (glibc >= 2.36) system libraries. It reports
# that as a contentless "failed to run linuxdeploy", *after* the .deb / .rpm
# have already been written, so only the AppImage goes missing. The trade-off
# is an unstripped (~115 MB instead of ~35 MB) AppImage; the release workflow
# builds on ubuntu-latest, whose older libraries linuxdeploy still strips.
# Forwarded through the child process, so an explicit NO_STRIP=0 wins.
bundle:
    NO_STRIP=1 pnpm tauri build

# Universal (arm64 + x86_64) macOS .dmg, for the rare Intel Mac still running a
# supported macOS. Not the release default — `bundle` produces a host-arch dmg
# (arm64 on Apple Silicon).
#
# Both the app and the CLI must be fat: a universal app with an arm64-only
# `corral` inside still launches under Rosetta and then fails every hook with
# "bad CPU type in executable". The staging script lipos the CLI; the `lipo`
# check below verifies every nested executable.
universal:
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    pnpm tauri build --target universal-apple-darwin
    @echo "=== architectures per executable (want x86_64 arm64 for each) ==="
    @find target/universal-apple-darwin/release/bundle/macos/Corral.app -type f -perm +111 -print -exec lipo -archs {} \;
    @echo "=== dmg ==="
    @ls -lh target/universal-apple-darwin/release/bundle/dmg/*.dmg

# Build and run the standalone debug binary
run: build
    ./target/debug/corral-app

# Build and run against an isolated dev profile — a separate config + data dir
# (see scripts/run-dev.sh), seeded from the real one on first run, so testing a
# new setting can't touch the live app's files. Pass a name for another
# profile: `just run-dev my-feature`.
run-dev profile="dev": build
    CORRAL_DEV_PROFILE={{profile}} ./scripts/run-dev.sh

# Exercise the `corral` CLI, e.g. `just cli list`
cli *args:
    cargo run -p corral-cli -- {{args}}

# Bump the version and tag the release, e.g. `just bump patch` or
# `just bump minor --no-tag`. See scripts/bump-version.sh and the
# "Releasing" note in AGENTS.md.
bump type *flags:
    ./scripts/bump-version.sh {{type}} {{flags}}
