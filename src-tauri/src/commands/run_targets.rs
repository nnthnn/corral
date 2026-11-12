//! Discovery of one-click "run targets" for the Run tab: a justfile's public
//! recipes, else the nearest `package.json`'s scripts. The justfile is treated
//! as the project's front door (its recipes usually wrap the package scripts
//! anyway), so a repo that has one shows only its recipes rather than a
//! duplicate set of chips; projects with no justfile fall back to scripts.
//!
//! `just` itself does the justfile parsing: `just --dump --dump-format json`
//! mirrors exactly what `just --list` would show (imports, modules,
//! `[private]`/`_`-prefixed recipes filtered out, docs and parameters
//! included) instead of corral maintaining a fragile parser of its own. The
//! `package.json` side is read directly — no `npm` spawn, and the script body
//! doubles as the chip's tooltip. If nothing is found, the Run tab just shows
//! no targets and the manual command field still works.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::run_detect::PackageManager;

/// One clickable target, in the shape the Run tab needs to render a button.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunTarget {
    /// Chip text (`build`).
    pub label: String,
    /// What ▶ actually runs (`just build`, `pnpm run build`).
    pub command: String,
    /// Recipe doc comment, or the package script's body. The button tooltip.
    pub doc: Option<String>,
    /// Parameters without defaults (`just build` would fail until they're
    /// supplied). Clicking such a target prefills the command instead of
    /// running it immediately. Empty for package scripts and the common
    /// no-argument recipe.
    pub required_params: Vec<String>,
}

/// The Run tab's whole target row: where the targets came from (the row's
/// little prefix label) plus the targets themselves.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RunTargets {
    /// `"just"`, or the package manager's program name (`"pnpm"`/`"yarn"`/
    /// `"npm"`/`"bun"`). `None` when there's nothing to show.
    pub source: Option<String>,
    pub targets: Vec<RunTarget>,
}

/// Just's justfile candidates, in its own lookup order.
const JUSTFILE_NAMES: &[&str] = &["justfile", "Justfile", ".justfile"];

/// Every clickable target in `cwd`, with the source they came from. Empty when
/// the directory has no justfile or package scripts (or the justfile can't be
/// parsed, or `just` isn't installed).
pub(crate) fn list_targets(cwd: &Path) -> RunTargets {
    if JUSTFILE_NAMES.iter().any(|name| cwd.join(name).is_file()) {
        return RunTargets {
            source: Some("just".into()),
            targets: just_targets(cwd),
        };
    }
    package_scripts(cwd)
}

/// Every public recipe in `cwd`'s justfile, alphabetically (the same order
/// `just --list` uses).
fn just_targets(cwd: &Path) -> Vec<RunTarget> {
    let output = match Command::new("just")
        .args(["--dump", "--dump-format", "json"])
        .current_dir(cwd)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    parse_dump(&output.stdout).unwrap_or_default()
}

#[derive(Deserialize)]
struct Dump {
    #[serde(default)]
    recipes: BTreeMap<String, Recipe>,
}

#[derive(Deserialize)]
struct Recipe {
    #[serde(default)]
    doc: Option<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    parameters: Vec<Parameter>,
}

#[derive(Deserialize)]
struct Parameter {
    #[serde(default)]
    name: String,
    /// `singular`, `star` (zero-or-more), or `plus` (one-or-more).
    #[serde(default)]
    kind: String,
    /// `null` when the parameter has no default. Variadic `*args` is also
    /// `null` but is optional, hence the `kind` check below.
    #[serde(default)]
    default: Option<serde_json::Value>,
}

/// Pull public recipes out of `just --dump --dump-format json` output,
/// preserving the dump's alphabetical key order. Split out from
/// [`just_targets`] so the filtering rules are testable without spawning
/// `just`.
fn parse_dump(json: &[u8]) -> Option<Vec<RunTarget>> {
    let dump: Dump = serde_json::from_slice(json).ok()?;
    Some(
        dump.recipes
            .into_iter()
            .filter(|(_, recipe)| !recipe.private)
            .map(|(name, recipe)| RunTarget {
                command: format!("just {name}"),
                label: name,
                doc: recipe.doc.filter(|doc| !doc.trim().is_empty()),
                required_params: recipe
                    .parameters
                    .into_iter()
                    .filter(|p| p.default.is_none() && p.kind != "star")
                    .map(|p| p.name)
                    .collect(),
            })
            .collect(),
    )
}

/// Every non-lifecycle script in `cwd`'s `package.json`, alphabetically. The
/// command is built with the directory's package manager (pnpm/yarn/npm/bun,
/// detected from its lockfile — same logic the Run tab's auto-detected prefill
/// uses). Empty when there's no `package.json`, no `scripts` object, or every
/// script is a lifecycle hook.
fn package_scripts(cwd: &Path) -> RunTargets {
    let Some(scripts) = read_package_scripts(cwd) else {
        return RunTargets::default();
    };
    let manager = PackageManager::detect(cwd);
    let mut targets: Vec<RunTarget> = scripts
        .iter()
        .filter(|(name, _)| !is_lifecycle_script(name, &scripts))
        .map(|(name, body)| RunTarget {
            label: name.clone(),
            command: manager.run_command(name),
            doc: body
                .as_str()
                .map(str::to_string)
                .filter(|body| !body.trim().is_empty()),
            required_params: Vec::new(),
        })
        .collect();
    targets.sort_by(|a, b| a.label.cmp(&b.label));
    RunTargets {
        source: Some(manager.program().into()),
        targets,
    }
}

fn read_package_scripts(cwd: &Path) -> Option<serde_json::Map<String, serde_json::Value>> {
    let contents = std::fs::read_to_string(cwd.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let scripts = value.get("scripts")?.as_object()?;
    (!scripts.is_empty()).then(|| scripts.clone())
}

/// Scripts npm runs implicitly (on install/publish/version, or as the `pre`/
/// `post` half of another script) rather than on request — noise in a row of
/// one-click buttons, and some are actively surprising to run standalone.
const RESERVED_SCRIPTS: &[&str] = &[
    "dependencies",
    "predependencies",
    "postdependencies",
    "install",
    "preinstall",
    "postinstall",
    "prepare",
    "prepublish",
    "publish",
    "prepublishOnly",
    "postpublish",
    "prepack",
    "postpack",
    "preversion",
    "version",
    "postversion",
    "preuninstall",
    "uninstall",
    "postuninstall",
];

fn is_lifecycle_script(name: &str, scripts: &serde_json::Map<String, serde_json::Value>) -> bool {
    if RESERVED_SCRIPTS.contains(&name) {
        return true;
    }
    // Arbitrary `prefoo`/`postfoo` hooks only count as lifecycle when `foo`
    // is itself a script; `preview` stays a normal target.
    match name
        .strip_prefix("pre")
        .or_else(|| name.strip_prefix("post"))
    {
        Some(base) if !base.is_empty() => scripts.contains_key(base),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn keeps_public_recipes_with_docs_and_drops_private_ones() {
        let json = br#"{
            "recipes": {
                "build": {
                    "doc": "build everything",
                    "private": false,
                    "parameters": []
                },
                "_helper": {
                    "doc": null,
                    "private": true,
                    "parameters": []
                },
                "secret": {
                    "doc": "hidden",
                    "private": true,
                    "parameters": []
                }
            }
        }"#;
        assert_eq!(
            parse_dump(json).unwrap(),
            vec![RunTarget {
                label: "build".into(),
                command: "just build".into(),
                doc: Some("build everything".into()),
                required_params: vec![],
            }]
        );
    }

    #[test]
    fn separates_required_parameters_from_defaulted_and_variadic_ones() {
        let json = br#"{
            "recipes": {
                "deploy": {
                    "doc": null,
                    "private": false,
                    "parameters": [
                        {"name": "env", "kind": "singular", "default": null},
                        {"name": "region", "kind": "singular", "default": "us"},
                        {"name": "rest", "kind": "star", "default": null}
                    ]
                }
            }
        }"#;
        assert_eq!(
            parse_dump(json).unwrap()[0].required_params,
            vec!["env".to_string()]
        );
    }

    #[test]
    fn blank_doc_comments_become_none() {
        let json = br#"{
            "recipes": {
                "dev": {"doc": "   ", "private": false, "parameters": []}
            }
        }"#;
        assert_eq!(parse_dump(json).unwrap()[0].doc, None);
    }

    #[test]
    fn malformed_json_yields_none_instead_of_panicking() {
        assert_eq!(parse_dump(b"not json"), None);
    }

    #[test]
    fn missing_optional_fields_still_parse() {
        let json = br#"{"recipes": {"dev": {}}}"#;
        assert_eq!(
            parse_dump(json).unwrap(),
            vec![RunTarget {
                label: "dev".into(),
                command: "just dev".into(),
                doc: None,
                required_params: vec![],
            }]
        );
    }

    #[test]
    fn no_justfile_or_package_json_means_no_targets_without_spawning_just() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(list_targets(dir.path()), RunTargets::default());
    }

    #[test]
    fn a_justfile_wins_over_package_scripts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("justfile"), "").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "vite"}}"#,
        )
        .unwrap();

        let targets = list_targets(dir.path());
        assert_eq!(targets.source.as_deref(), Some("just"));
        // Whether `just` is installed determines the recipe list, but either
        // way the package script must not leak into the just row.
        assert!(targets.targets.iter().all(|t| t.label != "dev"));
    }

    #[test]
    fn package_scripts_become_chips_with_the_manager_command_and_script_body() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {
                "test": "vitest run",
                "build": "tsc && vite build",
                "dev": "vite"
            }}"#,
        )
        .unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();

        assert_eq!(
            list_targets(dir.path()),
            RunTargets {
                source: Some("pnpm".into()),
                targets: vec![
                    RunTarget {
                        label: "build".into(),
                        command: "pnpm run build".into(),
                        doc: Some("tsc && vite build".into()),
                        required_params: vec![],
                    },
                    RunTarget {
                        label: "dev".into(),
                        command: "pnpm run dev".into(),
                        doc: Some("vite".into()),
                        required_params: vec![],
                    },
                    RunTarget {
                        label: "test".into(),
                        command: "pnpm run test".into(),
                        doc: Some("vitest run".into()),
                        required_params: vec![],
                    },
                ],
            }
        );
    }

    #[test]
    fn lifecycle_scripts_are_filtered_but_ordinary_prefixed_names_survive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {
                "build": "tsc",
                "prebuild": "rimraf dist",
                "postbuild": "echo done",
                "install": "node install.js",
                "preinstall": "node preinstall.js",
                "postinstall": "node postinstall.js",
                "prepare": "husky",
                "prepublishOnly": "npm test",
                "pretest": "node pretest.js",
                "test": "vitest",
                "preview": "vite preview"
            }}"#,
        )
        .unwrap();

        let labels: Vec<String> = list_targets(dir.path())
            .targets
            .into_iter()
            .map(|t| t.label)
            .collect();
        assert_eq!(labels, vec!["build", "preview", "test"]);
    }

    #[test]
    fn falls_back_to_npm_and_detects_bun_files() {
        let npm = tempfile::tempdir().unwrap();
        fs::write(
            npm.path().join("package.json"),
            r#"{"scripts": {"dev": "x"}}"#,
        )
        .unwrap();
        assert_eq!(list_targets(npm.path()).targets[0].command, "npm run dev");

        for lockfile in ["bun.lockb", "bun.lock"] {
            let dir = tempfile::tempdir().unwrap();
            fs::write(
                dir.path().join("package.json"),
                r#"{"scripts": {"dev": "x"}}"#,
            )
            .unwrap();
            fs::write(dir.path().join(lockfile), "").unwrap();
            assert_eq!(list_targets(dir.path()).targets[0].command, "bun run dev");
        }
    }

    #[test]
    fn package_json_without_scripts_yields_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name": "x"}"#).unwrap();
        assert_eq!(list_targets(dir.path()), RunTargets::default());
    }

    #[test]
    fn unreadable_or_malformed_package_json_yields_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "not json").unwrap();
        assert_eq!(list_targets(dir.path()), RunTargets::default());
    }
}
