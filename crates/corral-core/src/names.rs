//! Placeholder feature names, picked automatically so nobody has to name a
//! branch before they know what they're building — rename once the feature
//! has taken shape.

use rand::seq::SliceRandom;

use crate::error::Result;
use crate::worktree;

/// Farm chores, used as placeholder feature/branch names.
const CHORES: &[&str] = &[
    "mucking",
    "haying",
    "fencing",
    "milking",
    "plowing",
    "herding",
    "shearing",
    "planting",
    "weeding",
    "composting",
    "pruning",
    "churning",
    "canning",
    "gathering",
    "mending",
    "baling",
    "feeding",
    "tilling",
    "sowing",
    "reaping",
    "grooming",
    "saddling",
    "hauling",
    "roping",
    "wrangling",
    "raking",
    "cultivating",
    "sorting",
    "doctoring",
    "weaning",
    "foaling",
    "lambing",
    "calving",
    "threshing",
    "winnowing",
    "stacking",
    "chopping",
    "splitting",
    "birthing",
    "harvesting",
    "irrigating",
    "grafting",
    "culling",
    "corralling",
    "branding",
    "shoeing",
    "trailering",
    "loading",
    "grading",
    "spraying",
    "harrowing",
    "disking",
    "combining",
    "cutting",
    "shucking",
    "hulling",
    "brooding",
    "incubating",
    "penning",
    "watering",
];

/// Pick a chore name unused by any existing worktree in `project`. Falls back
/// to a numbered suffix (`haying-2`) once the list is exhausted.
pub fn generate(project: &str) -> Result<String> {
    // Just the feature names already taken — a directory listing, not a full
    // `worktree::list()` (which shells out to `git status` per worktree).
    let used = worktree::list_feature_names(project)?;

    let mut rng = rand::thread_rng();
    let mut available: Vec<&str> = CHORES
        .iter()
        .copied()
        .filter(|n| !used.contains(*n))
        .collect();
    available.shuffle(&mut rng);

    if let Some(name) = available.first() {
        return Ok(name.to_string());
    }

    let mut suffix = 2;
    loop {
        let base = CHORES.choose(&mut rng).expect("CHORES is non-empty");
        let candidate = format!("{base}-{suffix}");
        if !used.contains(&candidate) {
            return Ok(candidate);
        }
        suffix += 1;
    }
}

/// Normalize free-form text (e.g. an LLM's rename suggestion) into a
/// filesystem/branch-safe slug: lowercase words joined by single hyphens.
pub fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut prev_hyphen = true; // swallows leading separators
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    slug.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_hyphenates() {
        assert_eq!(slugify("Fix Login Timeout"), "fix-login-timeout");
    }

    #[test]
    fn slugify_collapses_runs_of_separators() {
        assert_eq!(slugify("fix -- login__timeout"), "fix-login-timeout");
    }

    #[test]
    fn slugify_trims_leading_and_trailing_separators() {
        assert_eq!(slugify("  --churning!! "), "churning");
    }

    #[test]
    fn slugify_of_empty_input_is_empty() {
        assert_eq!(slugify(""), "");
    }
}
