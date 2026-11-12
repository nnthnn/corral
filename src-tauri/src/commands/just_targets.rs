//! Discovery of a `justfile`'s public recipes, used to render one-click
//! target buttons in the Run tab. `just` itself does the parsing: `just
//! --dump --dump-format json` mirrors exactly what `just --list` would show
//! (imports, modules, `[private]`/`_`-prefixed recipes filtered out, docs and
//! parameters included) instead of corral maintaining a fragile justfile
//! parser of its own. If `just` is missing or the justfile doesn't parse,
//! the Run tab just shows no targets — the manual command field still works.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// One public recipe, in the shape the Run tab needs to render a button.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JustTarget {
    pub name: String,
    /// Recipe doc comment (`# ...` above it), shown as the button's tooltip.
    pub doc: Option<String>,
    /// Parameters without defaults (`just build` would fail until they're
    /// supplied). Clicking such a target prefills the command instead of
    /// running it immediately. Empty for the common no-argument recipe.
    pub required_params: Vec<String>,
}

/// Just's justfile candidates, in its own lookup order.
const JUSTFILE_NAMES: &[&str] = &["justfile", "Justfile", ".justfile"];

/// Every public recipe in `cwd`'s justfile, alphabetically (the same order
/// `just --list` uses). Empty when the directory has no justfile, `just`
/// isn't installed, or it can't parse the file.
pub(crate) fn list_targets(cwd: &Path) -> Vec<JustTarget> {
    if !JUSTFILE_NAMES.iter().any(|name| cwd.join(name).is_file()) {
        return Vec::new();
    }
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
/// [`list_targets`] so the filtering rules are testable without spawning
/// `just`.
fn parse_dump(json: &[u8]) -> Option<Vec<JustTarget>> {
    let dump: Dump = serde_json::from_slice(json).ok()?;
    Some(
        dump.recipes
            .into_iter()
            .filter(|(_, recipe)| !recipe.private)
            .map(|(name, recipe)| JustTarget {
                name,
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

#[cfg(test)]
mod tests {
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
            vec![JustTarget {
                name: "build".into(),
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
            vec![JustTarget {
                name: "dev".into(),
                doc: None,
                required_params: vec![],
            }]
        );
    }

    #[test]
    fn no_justfile_means_no_targets_without_spawning_just() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_targets(dir.path()).is_empty());
    }
}
