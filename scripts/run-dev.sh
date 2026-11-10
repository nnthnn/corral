#!/usr/bin/env bash
# Run a corral binary against an isolated "dev profile".
#
# A profile is just a separate pair of config/data dirs, wired up through
# CORRAL_CONFIG_DIR / CORRAL_DATA_DIR (see corral-core::paths). Its whole point
# is testing a new setting — or any persistence change — in a dev instance
# without the live app's files ever being touched: the running app keeps
# using ~/.config/corral and ~/.local/share/corral, the dev instance gets
# ~/.config/corral/profiles/<name> and ~/.local/share/corral/profiles/<name>.
#
# On first run the profile's settings.json and projects.json are seeded from
# the real config, so the dev instance starts with your projects, worktrees,
# and current settings — you tweak just the new field on the isolated copy.
# Delete the profile dir to start over, or pass a different name for a
# second profile.
#
# Usage:
#   CORRAL_DEV_PROFILE=dev ./scripts/run-dev.sh            # the desktop app
#   CORRAL_DEV_PROFILE=dev CORRAL_DEV_BIN=./target/debug/corral \
#       ./scripts/run-dev.sh list                          # the CLI, same profile
set -euo pipefail

profile="${CORRAL_DEV_PROFILE:-dev}"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"

real_config_dir="$config_home/corral"
profile_config_dir="$config_home/corral/profiles/$profile"
profile_data_dir="$data_home/corral/profiles/$profile"

mkdir -p "$profile_config_dir" "$profile_data_dir"

seed() {
    local name="$1"
    if [ ! -e "$profile_config_dir/$name" ] && [ -e "$real_config_dir/$name" ]; then
        cp "$real_config_dir/$name" "$profile_config_dir/$name"
        echo "run-dev: seeded $profile_config_dir/$name"
    fi
}
seed settings.json
seed projects.json

export CORRAL_CONFIG_DIR="$profile_config_dir"
export CORRAL_DATA_DIR="$profile_data_dir"

echo "run-dev: profile '$profile'"
echo "run-dev:   CORRAL_CONFIG_DIR=$CORRAL_CONFIG_DIR"
echo "run-dev:   CORRAL_DATA_DIR=$CORRAL_DATA_DIR"

exec "${CORRAL_DEV_BIN:-./target/debug/corral-app}" "$@"
