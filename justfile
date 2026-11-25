default:
    @just --list

# Type-check + bundle the frontend only (tsc + vite build)
frontend:
    pnpm build

# Compile-check the whole Rust workspace
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

# Build a real standalone binary fast, skipping the tauri-cli bundler (see CLAUDE.md)
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
bundle:
    pnpm tauri build

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
