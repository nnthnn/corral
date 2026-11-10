#!/usr/bin/env bash
# Bump corral's version across the three files that carry it, refresh
# Cargo.lock, commit the result, and tag the release.
#
#   ./scripts/bump-version.sh patch   # 0.1.0 -> 0.1.1
#   ./scripts/bump-version.sh minor   # 0.1.0 -> 0.2.0
#   ./scripts/bump-version.sh major   # 0.1.0 -> 1.0.0
#
# Flags (after the bump type):
#   --no-commit   edit the files + refresh Cargo.lock, but don't commit
#   --no-tag      commit (unless --no-commit) but skip tagging
#
# The version lives in exactly three places, and this script refuses to run if
# they've drifted apart (see AGENTS.md "Releasing"):
#   Cargo.toml                     [workspace.package].version
#   package.json                   "version"
#   src-tauri/tauri.conf.json      "version"
#
# Tags are annotated and named vX.Y.Z — the format the release workflow
# (.github/workflows/release.yml) triggers on.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

usage() {
  echo "usage: $0 <patch|minor|major> [--no-commit] [--no-tag]" >&2
  exit 2
}

[[ $# -ge 1 ]] || usage
bump_type="$1"; shift

case "$bump_type" in
  patch|minor|major) ;;
  *) usage ;;
esac

no_commit=0
no_tag=0
for arg in "$@"; do
  case "$arg" in
    --no-commit) no_commit=1 ;;
    --no-tag) no_tag=1 ;;
    *) usage ;;
  esac
done

cargo_toml="Cargo.toml"
pkg_json="package.json"
tauri_conf="src-tauri/tauri.conf.json"

# --- read the current version (source of truth: the workspace package version) ---
current="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' "$cargo_toml" | head -n1)"
if [[ -z "$current" ]]; then
  echo "error: couldn't read version from $cargo_toml" >&2
  exit 1
fi
if [[ ! "$current" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: unsupported version '$current' (expected plain semver x.y.z)" >&2
  exit 1
fi

json_version() {
  sed -n 's/^  "version": "\([^"]*\)".*/\1/p' "$1" | head -n1
}

# --- fail loudly if the three files have drifted apart ---
for f in "$pkg_json" "$tauri_conf"; do
  v="$(json_version "$f")"
  if [[ "$v" != "$current" ]]; then
    echo "error: version drift — $f has '$v' but $cargo_toml has '$current'" >&2
    exit 1
  fi
done

# --- compute the next version ---
IFS='.' read -r major minor patch <<< "$current"
case "$bump_type" in
  major) major=$((major + 1)); minor=0; patch=0 ;;
  minor) minor=$((minor + 1)); patch=0 ;;
  patch) patch=$((patch + 1)) ;;
esac
next="$major.$minor.$patch"
tag="v$next"

echo "bumping $current -> $next"

# --- edit the three files ---
# Cargo.toml: bump only the workspace.package version (the first `version =`
# line at the top of the file is it; dependencies use `version.workspace`).
sed -i "0,/^version = \"$current\"$/s//version = \"$next\"/" "$cargo_toml"
sed -i "s/\"version\": \"$current\"/\"version\": \"$next\"/" "$pkg_json" "$tauri_conf"

# --- refresh Cargo.lock (cargo metadata rewrites the three workspace-member
# version entries so a `cargo build --locked` on the tag stays reproducible) ---
cargo metadata --format-version 1 >/dev/null
if ! grep -q "^version = \"$next\"$" "Cargo.lock"; then
  echo "error: Cargo.lock does not reflect $next after refresh" >&2
  exit 1
fi

if (( no_commit )); then
  echo "done — files edited and Cargo.lock refreshed; not committing (--no-commit)"
  exit 0
fi

git add "$cargo_toml" "$pkg_json" "$tauri_conf" "Cargo.lock"
git commit -m "chore: bump version to $tag"

if (( no_tag )); then
  echo "committed; not tagging (--no-tag)"
  exit 0
fi

git tag -a "$tag" -m "corral $tag"

echo
echo "bumped $current -> $next, committed, and tagged $tag"
echo "push with:"
echo "  git push origin HEAD"
echo "  git push origin $tag"
