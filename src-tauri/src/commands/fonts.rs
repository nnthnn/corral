//! Enumerate the system's monospaced font families for the terminal font
//! picker in Settings.
//!
//! There's no portable OS command for this: Linux has `fc-list :spacing=mono`
//! (fast and authoritative), but macOS and Windows have nothing equivalent
//! from a shell — macOS needs Core Text (or a very slow `system_profiler`),
//! Windows needs DirectWrite/registry digging, and neither reports
//! monospace-ness from the registry anyway. Rather than three separate
//! platform code paths (and extra system-library dependencies), this uses
//! [`fontdb`], which scans the platform's standard font directories, parses
//! each font file with a pure-Rust TTF/OTF parser, and exposes a
//! `monospaced` flag from the font's `post.isFixedPitch` field.
//!
//! The scan is **best-effort and advisory**: it feeds a `<datalist>`, so a
//! missed or extra family costs nothing (the picker is still free text), and
//! the frontend falls back to a short hardcoded list if this fails or returns
//! empty. Results are cached for the process lifetime — a cold scan of this
//! machine's ~1,100 font faces takes ~90ms, and nothing changes between
//! settings opens short of the user installing a font (which needs an app
//! restart to show up anyway).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use fontdb::Database;

/// Family-name substrings that are technically "monospaced" but useless as a
/// terminal font — symbol/emoji fonts report a fixed advance width, and
/// Windows ships a few icon fonts, but none have ordinary text glyphs.
const NON_TEXT_FAMILIES: &[&str] = &[
    "emoji",
    "symbol",
    "dingbat",
    "icon",
    "webdings",
    "wingdings",
    "font awesome",
    "material icons",
];

static CACHE: OnceLock<Vec<String>> = OnceLock::new();

/// Monospaced font families installed on this machine, sorted
/// case-insensitively and de-duplicated. Never fails: an enumeration problem
/// just yields an empty list (the picker then shows its built-in fallbacks).
#[tauri::command]
pub async fn list_monospace_fonts() -> Vec<String> {
    if let Some(fonts) = CACHE.get() {
        return fonts.clone();
    }
    // Parsing a few hundred font files shouldn't run on the GTK main loop.
    let enumerated = tauri::async_runtime::spawn_blocking(enumerate)
        .await
        .unwrap_or_default();
    CACHE.get_or_init(|| enumerated).clone()
}

fn enumerate() -> Vec<String> {
    let mut db = Database::new();
    db.load_system_fonts();
    collect_families(db.faces())
}

/// Pure part of [`enumerate`], split out so it can be tested without touching
/// the real system font database.
fn collect_families<'a>(faces: impl Iterator<Item = &'a fontdb::FaceInfo>) -> Vec<String> {
    // Keyed by the lowercased name so families that differ only in case (or
    // appear once per face — Bold, Italic, ...) collapse to one entry, while
    // the original spelling is what gets returned.
    let mut by_lower: BTreeMap<String, String> = BTreeMap::new();
    for face in faces.filter(|f| f.monospaced) {
        // The first family is the English one by fontdb's convention.
        let Some((family, _)) = face.families.first() else {
            continue;
        };
        let family = family.trim();
        if family.is_empty() || is_non_text(family) {
            continue;
        }
        by_lower
            .entry(family.to_lowercase())
            .or_insert_with(|| family.to_string());
    }
    by_lower.into_values().collect()
}

fn is_non_text(family: &str) -> bool {
    let lower = family.to_lowercase();
    NON_TEXT_FAMILIES
        .iter()
        .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(family: &str, monospaced: bool) -> fontdb::FaceInfo {
        fontdb::FaceInfo {
            id: fontdb::ID::dummy(),
            source: fontdb::Source::Binary(std::sync::Arc::new(Vec::new())),
            index: 0,
            families: vec![(family.to_string(), fontdb::Language::English_UnitedStates)],
            post_script_name: String::new(),
            style: fontdb::Style::Normal,
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            monospaced,
        }
    }

    #[test]
    fn keeps_only_monospaced_families() {
        let faces = [face("JetBrains Mono", true), face("Inter", false)];
        assert_eq!(collect_families(faces.iter()), ["JetBrains Mono"]);
    }

    #[test]
    fn de_duplicates_faces_of_the_same_family_case_insensitively() {
        let faces = [face("Hack", true), face("Hack", true), face("hack", true)];
        assert_eq!(collect_families(faces.iter()), ["Hack"]);
    }

    #[test]
    fn sorts_case_insensitively_and_ignores_blank_names() {
        let faces = [
            face("ubuntu Mono", true),
            face("  ", true),
            face("Adwaita Mono", true),
        ];
        assert_eq!(
            collect_families(faces.iter()),
            ["Adwaita Mono", "ubuntu Mono"]
        );
    }

    #[test]
    fn skips_symbol_and_emoji_fonts() {
        let faces = [
            face("Noto Color Emoji", true),
            face("Symbols Nerd Font Mono", true),
            face("DejaVu Sans Mono", true),
        ];
        assert_eq!(collect_families(faces.iter()), ["DejaVu Sans Mono"]);
    }
}
