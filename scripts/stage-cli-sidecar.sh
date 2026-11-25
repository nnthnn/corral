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
#
# macOS's bash 3.2 aborts on `"${empty_array[@]}"` under `set -u` (`locked[@]:
# unbound variable`) where bash 4.4+ expands it to nothing, so this script has
# to keep running on both. Hence no array for the optional flag, and a `cargo`
# wrapper instead of passing one long array (see `run_cargo` below).
locked=""
if [[ -n "${CI:-}" ]]; then
  locked="--locked"
fi

# `profile_dir` is where cargo puts the binary; `build_profile` is the flag that
# selects it (`--debug` rather than no flag so the call sites stay uniform).
profile_dir="release"
build_profile="--release"
if [[ "${TAURI_ENV_DEBUG:-}" == "true" ]]; then
  profile_dir="debug"
  build_profile="--debug"
fi

run_cargo() {
  if [[ -n "$locked" ]]; then
    cargo build -p corral-cli "$build_profile" "$locked" "$@"
  else
    cargo build -p corral-cli "$build_profile" "$@"
  fi
}

target_dir="${CARGO_TARGET_DIR:-target}"
mkdir -p src-tauri/binaries
destination="src-tauri/binaries/corral-$triple"

if [[ "$triple" == universal-apple-darwin ]]; then
  if ! command -v lipo >/dev/null 2>&1; then
    echo "error: lipo is required for universal-apple-darwin (macOS only)" >&2
    exit 1
  fi
  # Deliberately not an array: `lipo` is fine with the plain positional args,
  # and this keeps the script free of empty-array expansions (see above).
  slice_args=""
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    run_cargo --target "$arch"
    slice_args="$slice_args $target_dir/$arch/$profile_dir/corral"
  done
  # shellcheck disable=SC2086 # word-splitting the two slice paths is intended
  lipo -create $slice_args -output "$destination"
else
  run_cargo --target "$triple"
  built="$target_dir/$triple/$profile_dir/corral"
  if [[ ! -x "$built" ]]; then
    echo "error: cargo succeeded but $built is missing; check that" >&2
    echo "       CARGO_TARGET_DIR (${CARGO_TARGET_DIR:-unset}) points at the workspace target dir" >&2
    exit 1
  fi
  cp "$built" "$destination"
fi

chmod +x "$destination"

# A successful build should always produce a runnable CLI, but a stale path or a
# partially configured target dir could leave an old copy in place — and a broken
# `corral` in the bundle builds fine and only fails later, at every Claude Code
# status hook (or at every `corral` invocation from an AppImage). Refuse to
# stage something that doesn't even report its version.
if ! "$destination" --version >/dev/null 2>&1; then
  echo "error: $destination does not run (a sidecar with the wrong arch?):" >&2
  file "$destination" >&2 || true
  exit 1
fi

echo "staged sidecar: $destination ($("$destination" --version))"
