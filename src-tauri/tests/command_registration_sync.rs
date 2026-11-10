//! Every `#[tauri::command]` must be registered in three places — see
//! CLAUDE.md's "Every Tauri command must be registered" gotcha:
//!   1. `src/lib.rs`'s `tauri::generate_handler![...]` list
//!   2. `build.rs`'s `AppManifest::commands(&[...])` list
//!   3. `capabilities/default.json`'s `allow-<command>` permissions
//!
//! A command missing from any one of these fails at runtime ("not allowed.
//! Plugin not found"), in both dev and bundled builds, rather than at
//! compile time. This test parses all three sources and asserts they name
//! the same set of commands, so a forgotten registration fails `cargo test`
//! instead of surfacing as a runtime IPC error.

use std::collections::BTreeSet;
use std::path::Path;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Pull the identifiers out of `tauri::generate_handler![commands::a::b, ...]`
/// in `lib.rs`, keeping just the final path segment (the fn name).
fn commands_from_lib_rs() -> BTreeSet<String> {
    let src = std::fs::read_to_string(manifest_dir().join("src/lib.rs")).unwrap();
    let start = src
        .find("generate_handler![")
        .expect("generate_handler![ not found in lib.rs")
        + "generate_handler![".len();
    let end = src[start..]
        .find(']')
        .expect("no closing ] for generate_handler![")
        + start;
    src[start..end]
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.rsplit("::").next().unwrap().to_string())
        .collect()
}

/// Pull the string literals out of `build.rs`'s `.commands(&["a", "b", ...])`.
fn commands_from_build_rs() -> BTreeSet<String> {
    let src = std::fs::read_to_string(manifest_dir().join("build.rs")).unwrap();
    let start = src
        .find(".commands(&[")
        .expect(".commands(&[ not found in build.rs")
        + ".commands(&[".len();
    let end = src[start..]
        .find(']')
        .expect("no closing ] for .commands(&[")
        + start;
    src[start..end]
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_matches('"').to_string())
        .collect()
}

/// Pull the `allow-<command>` permissions out of `capabilities/default.json`,
/// converting `allow-kebab-case` back to `snake_case` to compare against the
/// function names used in `lib.rs`/`build.rs`.
fn commands_from_capabilities() -> BTreeSet<String> {
    let src = std::fs::read_to_string(manifest_dir().join("capabilities/default.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&src).unwrap();
    json["permissions"]
        .as_array()
        .expect("capabilities/default.json: \"permissions\" is not an array")
        .iter()
        .filter_map(|p| p.as_str())
        .filter_map(|p| p.strip_prefix("allow-"))
        // Plugin-namespaced permissions (e.g. "clipboard-manager:allow-...")
        // aren't corral's own commands — only bare "allow-*" entries are.
        .filter(|p| !p.contains(':'))
        .map(|p| p.replace('-', "_"))
        .collect()
}

#[test]
fn command_registrations_are_in_sync_across_all_three_places() {
    let lib_rs = commands_from_lib_rs();
    let build_rs = commands_from_build_rs();
    let capabilities = commands_from_capabilities();

    assert_eq!(
        lib_rs,
        build_rs,
        "lib.rs's generate_handler! and build.rs's AppManifest::commands disagree.\n\
         only in lib.rs: {:?}\nonly in build.rs: {:?}",
        lib_rs.difference(&build_rs).collect::<Vec<_>>(),
        build_rs.difference(&lib_rs).collect::<Vec<_>>(),
    );
    assert_eq!(
        lib_rs,
        capabilities,
        "lib.rs's generate_handler! and capabilities/default.json disagree.\n\
         only in lib.rs: {:?}\nonly in capabilities: {:?}",
        lib_rs.difference(&capabilities).collect::<Vec<_>>(),
        capabilities.difference(&lib_rs).collect::<Vec<_>>(),
    );
}
