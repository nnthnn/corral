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

profile_dir="release"
cargo_args=(build -p corral-cli --release)
if [[ "${TAURI_ENV_DEBUG:-}" == "true" ]]; then
  profile_dir="debug"
  cargo_args=(build -p corral-cli)
fi

target_dir="${CARGO_TARGET_DIR:-target}"
mkdir -p src-tauri/binaries
destination="src-tauri/binaries/corral-$triple"

if [[ "$triple" == universal-apple-darwin ]]; then
  slices=()
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo "${cargo_args[@]}" --target "$arch"
    slices+=("$target_dir/$arch/$profile_dir/corral")
  done
  lipo -create "${slices[@]}" -output "$destination"
else
  cargo "${cargo_args[@]}" --target "$triple"
  cp "$target_dir/$triple/$profile_dir/corral" "$destination"
fi

chmod +x "$destination"
echo "staged sidecar: $destination"
