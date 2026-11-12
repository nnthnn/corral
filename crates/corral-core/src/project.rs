//! Project discovery: the git repos directly under `~/source`.

use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::config;
use crate::error::Result;

/// List every registered project's name, alphabetically. See
/// `crate::project_settings`'s registry — a project exists here iff it has
/// a registry entry.
pub fn list() -> Result<Vec<String>> {
    let mut out: Vec<String> = crate::project_settings::all()?.into_keys().collect();
    out.sort();
    Ok(out)
}

/// Repo-relative candidate paths for a project's icon, checked in order —
/// common web-app favicon/logo spots first, then a bare repo-root fallback.
const ICON_CANDIDATES: &[&str] = &[
    "public/favicon.svg",
    "public/favicon.ico",
    "public/favicon.png",
    "public/logo.svg",
    "public/logo.png",
    "src/assets/icon.svg",
    "src/assets/icon.png",
    "src/assets/logo.svg",
    "src/assets/logo.png",
    "icon.png",
    "logo.png",
];

/// Icon files larger than this are skipped — a real favicon/logo is a few
/// KB; anything bigger is probably a mis-detected asset we don't want to
/// inline as a data URI into every sidebar render.
const MAX_ICON_BYTES: u64 = 256 * 1024;

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        _ => "image/png",
    }
}

/// Best-effort auto-detected icon for `project`, as a `data:` URI ready to
/// use as an `<img src>`. Checked against the *main repo* checkout
/// (`~/source/<project>`), not any individual worktree, since icon assets
/// live in the shared repo history every worktree forks from.
pub fn find_icon(project: &str) -> Option<String> {
    find_icon_in(&config::repo_path(project).ok()?)
}

/// [`find_icon`] with an explicit repo root, so the candidate-priority logic
/// is testable without touching `$HOME`.
fn find_icon_in(repo: &Path) -> Option<String> {
    for candidate in ICON_CANDIDATES {
        let path = repo.join(candidate);
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_ICON_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        return Some(format!(
            "data:{};base64,{}",
            mime_for(&path),
            STANDARD.encode(bytes)
        ));
    }

    let path = scan_for_icon(repo)?;
    let bytes = std::fs::read(&path).ok()?;
    Some(format!(
        "data:{};base64,{}",
        mime_for(&path),
        STANDARD.encode(bytes)
    ))
}

/// How deep [`scan_for_icon`] will descend below the repo root. Kept shallow
/// so it only picks up top-level "project image" spots (`assets/`,
/// `src-tauri/icons/`, etc.), not every image anywhere in the tree.
const SCAN_MAX_DEPTH: usize = 3;

/// Fallback for repos that don't match [`ICON_CANDIDATES`]: walk the repo
/// (gitignore-aware, so `node_modules`/`target`/`dist` are skipped for free)
/// up to [`SCAN_MAX_DEPTH`], and pick the most icon-like image file by
/// filename, then shallowest, then smallest.
///
/// "Mark" outranks "logo" because the two are different assets: a mark is
/// the compact symbol (usually square, so it survives the sidebar's square
/// crop), while a logo is often a wide horizontal lockup. `wordmark` is
/// explicitly excluded — it's the text half of a lockup, not a symbol.
fn scan_for_icon(repo: &Path) -> Option<PathBuf> {
    let mut best: Option<(u8, usize, u64, PathBuf)> = None;

    let walker = ignore::WalkBuilder::new(repo)
        .max_depth(Some(SCAN_MAX_DEPTH))
        .hidden(true)
        .require_git(false)
        .build();

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "svg" | "ico" | "png" | "jpg" | "jpeg"
                )
            });
        if !is_image {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.len() == 0 || meta.len() > MAX_ICON_BYTES {
            continue;
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let score: u8 = if name.contains("icon") {
            0
        } else if name.contains("mark") && !name.contains("wordmark") {
            1
        } else if name.contains("logo") {
            2
        } else {
            3
        };
        let depth = entry.depth();
        let key = (score, depth, meta.len());

        let is_better = match &best {
            None => true,
            Some((bs, bd, bl, _)) => key < (*bs, *bd, *bl),
        };
        if is_better {
            best = Some((score, depth, meta.len(), path.to_path_buf()));
        }
    }

    best.map(|(_, _, _, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, bytes: &[u8]) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn find_icon_in_prefers_public_favicon_over_later_candidates() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "public/favicon.svg", b"<svg/>");
        write(dir.path(), "icon.png", &[0x89, b'P', b'N', b'G']);

        let uri = find_icon_in(dir.path()).unwrap();
        assert!(uri.starts_with("data:image/svg+xml;base64,"));
    }

    #[test]
    fn find_icon_in_falls_back_through_priority_order() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/assets/logo.png", &[0x89, b'P', b'N', b'G']);

        let uri = find_icon_in(dir.path()).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn find_icon_in_returns_none_when_nothing_matches() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_icon_in(dir.path()), None);
    }

    #[test]
    fn find_icon_in_skips_files_over_the_size_cap() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "public/favicon.png",
            &vec![0u8; MAX_ICON_BYTES as usize + 1],
        );

        assert_eq!(find_icon_in(dir.path()), None);
    }

    #[test]
    fn find_icon_in_picks_ico_mime_for_favicon_ico() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "public/favicon.ico", &[0, 0, 1, 0]);

        let uri = find_icon_in(dir.path()).unwrap();
        assert!(uri.starts_with("data:image/x-icon;base64,"));
    }

    #[test]
    fn find_icon_in_falls_back_to_scan_when_no_candidate_matches() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "assets/logo.png", &[0x89, b'P', b'N', b'G']);

        let uri = find_icon_in(dir.path()).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn scan_prefers_icon_named_file_over_logo_named_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "assets/logo.png", &[0x89, b'P', b'N', b'G']);
        write(
            dir.path(),
            "src-tauri/icons/icon.png",
            &[0x89, b'P', b'N', b'G'],
        );

        let path = scan_for_icon(dir.path()).unwrap();
        assert_eq!(path.file_name().unwrap(), "icon.png");
    }

    #[test]
    fn scan_prefers_mark_named_file_over_wide_logo() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "design/logo/brand-logo-on-dark.svg", b"<svg/>");
        write(dir.path(), "design/logo/brand-mark.svg", b"<svg/>");

        let path = scan_for_icon(dir.path()).unwrap();
        assert_eq!(path.file_name().unwrap(), "brand-mark.svg");
    }

    #[test]
    fn scan_does_not_treat_wordmark_as_a_mark() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "assets/brand-wordmark.svg", b"<svg/>");
        write(dir.path(), "assets/brand-logo.svg", b"<svg/>");

        let path = scan_for_icon(dir.path()).unwrap();
        assert_eq!(path.file_name().unwrap(), "brand-logo.svg");
    }

    #[test]
    fn scan_ignores_gitignored_directories() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".gitignore", b"node_modules/\n");
        write(
            dir.path(),
            "node_modules/logo.png",
            &[0x89, b'P', b'N', b'G'],
        );

        assert_eq!(scan_for_icon(dir.path()), None);
    }

    #[test]
    fn scan_does_not_descend_past_max_depth() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a/b/c/d/logo.png", &[0x89, b'P', b'N', b'G']);

        assert_eq!(scan_for_icon(dir.path()), None);
    }

    #[test]
    fn scan_returns_none_for_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(scan_for_icon(dir.path()), None);
    }
}
