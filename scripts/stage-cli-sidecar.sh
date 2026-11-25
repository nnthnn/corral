#!/usr/bin/env bash
# Stage the `corral` CLI as a Tauri external binary (sidecar) for the target
# being bundled. Run as `build.beforeBundleCommand`, so it executes after the
# app is compiled but before the bundler resolves `bundle.externalBin`.
#
# Tauri expects `src-tauri/binaries/corral-<target-triple>` and copies it to:
#   - macOS: `Corral.app/Contents/MacOS/corral` (and signs it with the app)
#   - Linux: `/usr/bin/corral` in the .deb / .rpm / .AppImage
#
# The app finds it as a sibling of its own executable (`corral-core::bin`), and
# `start_session` prepends its directory to the agent PTY's PATH.
#
# `TAURI_ENV_TARGET_TRIPLE` is set by the Tauri CLI for beforeBundleCommand
# hooks. For a universal macOS build the bundler wants a fat binary named
# `corral-universal-apple-darwin`, which cargo cannot produce directly, so we
# build both slices and lipo them.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

triple="${TAURI_ENV_TARGET_TRIPLE:-}"
if [[ -z "$triple" ]]; then
  triple="$(rustc -vV | sed -n 's/^host: //p')"
fi

# `--locked` keeps a *release* reproducible from the committed `Cargo.lock`
# (mirroring the workflow's `--frozen-lockfile` for pnpm). Only applied when
# GitHub Actions runs it (`CI` is set there), so a local `just bundle` still
# works while a lockfile is being changed — CI is exactly where an out-of-date
# lockfile should fail loudly instead.
locked=()
if [[ -n "${CI:-}" ]]; then
  locked=(--locked)
fi

profile_dir="release"
cargo_args=(build -p corral-cli --release "${locked[@]}")
if [[ "${TAURI_ENV_DEBUG:-}" == "true" ]]; then
  profile_dir="debug"
  cargo_args=(build -p corral-cli "${locked[@]}")
fi

# The message `src-tauri/build.rs` writes into its compile-time placeholder.
# Stage a real CLI over it below; the check at the end refuses to leave one in
# place, because a bundle carrying the placeholder builds fine but silently
# breaks every Claude Code status hook.
placeholder_marker="this bundle was built without the CLI sidecar"

target_dir="${CARGO_TARGET_DIR:-target}"
mkdir -p src-tauri/binaries
destination="src-tauri/binaries/corral-$triple"

if [[ "$triple" == universal-apple-darwin ]]; then
  if ! command -v lipo >/dev/null 2>&1; then
    echo "error: lipo is required for universal-apple-darwin (macOS only)" >&2
    exit 1
  fi
  slices=()
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo "${cargo_args[@]}" --target "$arch"
    slices+=("$target_dir/$arch/$profile_dir/corral")
  done
  lipo -create "${slices[@]}" -output "$destination"
else
  cargo "${cargo_args[@]}" --target "$triple"
  built="$target_dir/$triple/$profile_dir/corral"
  if [[ ! -x "$built" ]]; then
    echo "error: cargo succeeded but $built is missing; check that" >&2
    echo "       CARGO_TARGET_DIR (${CARGO_TARGET_DIR:-unset}) points at the workspace target dir" >&2
    exit 1
  fi
  cp "$built" "$destination"
fi

chmod +x "$destination"

# A successful build should always have replaced the placeholder, but a stale
# `cp` path or a partial checkout could leave it. Fail loudly here rather than
# shipping hooks that exit 127. `grep` exits 1 for "no match", 2 for an error.
status=0
grep -qF "$placeholder_marker" "$destination" || status=$?
if [[ "$status" -eq 0 ]]; then
  echo "error: $destination is still the build.rs placeholder, not the real CLI" >&2
  exit 1
elif [[ "$status" -ne 1 ]]; then
  echo "error: could not read $destination to verify it" >&2
  exit 1
fi

echo "staged sidecar: $destination"
