//! File-tree browsing for the worktree "Files" panel: lists directory
//! entries (gitignore-aware) and reads individual file contents for
//! preview.

use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A single entry (file or directory) returned by [`list_dir`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    /// Path relative to the worktree root, forward-slash separated.
    pub path: String,
    pub is_dir: bool,
}

/// Result of [`read_file`], serialized to the frontend as a discriminated
/// union on `kind`, matching `MergeResult`'s `outcome`-tag convention in
/// `merge.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileContent {
    Text {
        text: String,
    },
    /// A raster image the preview can render directly. `data` is the raw
    /// file bytes, base64-encoded — the frontend wraps it in a
    /// `data:<mime>;base64,` URL. SVG is deliberately *not* here: it's a
    /// text format, so it comes back as [`FileContent::Text`] and the
    /// preview offers a rendered/source toggle instead of a PNG-style dump.
    Image {
        mime: String,
        data: String,
    },
    Binary,
    TooLarge {
        size: u64,
    },
}

/// Resolve `relative_path` under `root`, rejecting any path that escapes it
/// (`..`, an absolute path, or a Windows drive prefix) — `relative_path`
/// arrives from the frontend over the Tauri IPC boundary, so this is
/// defense in depth rather than a trusted internal call.
fn resolve_within(root: &Path, relative_path: &str) -> Result<PathBuf> {
    use std::path::Component;
    let candidate = Path::new(relative_path);
    let escapes = candidate.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if escapes {
        return Err(Error::InvalidName(relative_path.to_string()));
    }
    Ok(root.join(candidate))
}

/// Lists the immediate children of `root.join(relative_path)` (or `root`
/// itself when `relative_path` is empty), gitignore-aware. Mirrors the
/// `ignore::WalkBuilder` usage in `project.rs::scan_for_icon` and
/// `worktree.rs::copy_matching_files`: `require_git(false)` so `.gitignore`
/// rules are honored even when walking a subdirectory that isn't itself a
/// git repo root, and dotfiles are shown (`hidden(false)`) since a general
/// file explorer should surface `.gitignore`/`.github`/etc. The `.git`
/// directory itself is always excluded — it's not swept up by gitignore
/// rules (it's not normally gitignored) but showing its internals is noise,
/// matching `copy_matching_files`'s explicit `.git` skip.
pub fn list_dir(root: &Path, relative_path: &str) -> Result<Vec<FileEntry>> {
    let dir = resolve_within(root, relative_path)?;

    let mut entries = Vec::new();
    let walker = ignore::WalkBuilder::new(&dir)
        .max_depth(Some(1))
        .hidden(false)
        .require_git(false)
        .build();

    for entry in walker {
        let entry = entry.map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
        if entry.depth() == 0 {
            continue; // the directory itself
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        let path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(FileEntry { name, path, is_dir });
    }

    // Dirs first, then natural (numeric-aware) name order. Lowercased to keep
    // the pre-existing case-insensitive ordering; digits are unaffected.
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => crate::natural::compare(&a.name.to_lowercase(), &b.name.to_lowercase()),
    });

    Ok(entries)
}

/// Files larger than this are reported as [`FileContent::TooLarge`] rather
/// than read into memory.
const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

/// Images get a much larger cap than text: a screenshot or design asset
/// routinely runs a few MB, and previewing it is the whole point. Base64
/// inflates this ~1.33x over IPC, so the ceiling stays well inside what the
/// webview can hand back without a noticeable stall.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// How many leading bytes [`is_binary`] inspects for a null byte.
const BINARY_SNIFF_BYTES: usize = 8192;

/// Maps a path's extension to the MIME type of an image the preview can
/// render in an `<img>`. `None` for everything else.
///
/// SVG is intentionally excluded even though it is an image: it's a *text*
/// format, so it flows through the normal text path and the frontend offers
/// a rendered/source toggle on top. Raster formats have no useful source
/// view, so they're decoded here and shown directly.
fn image_mime(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        _ => return None,
    })
}

/// Reads `root.join(relative_path)` for the file preview panel.
///
/// Images ([`image_mime`]-recognized extensions) are read whole and returned
/// base64-encoded for direct rendering, capped at [`MAX_IMAGE_BYTES`].
/// Everything else caps at [`MAX_PREVIEW_BYTES`] (checked via metadata before
/// reading, so an oversized file is never loaded into memory) and sniffs the
/// leading bytes for binary content, matching `git.rs`'s `from_utf8_lossy`
/// convention for anything that does get read as text.
pub fn read_file(root: &Path, relative_path: &str) -> Result<FileContent> {
    let path = resolve_within(root, relative_path)?;
    let metadata = std::fs::metadata(&path)?;

    if let Some(mime) = image_mime(relative_path) {
        if metadata.len() > MAX_IMAGE_BYTES {
            return Ok(FileContent::TooLarge {
                size: metadata.len(),
            });
        }
        let bytes = std::fs::read(&path)?;
        return Ok(FileContent::Image {
            mime: mime.to_string(),
            data: STANDARD.encode(&bytes),
        });
    }

    if metadata.len() > MAX_PREVIEW_BYTES {
        return Ok(FileContent::TooLarge {
            size: metadata.len(),
        });
    }
    let bytes = std::fs::read(&path)?;
    if is_binary(&bytes) {
        return Ok(FileContent::Binary);
    }
    Ok(FileContent::Text {
        text: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_dir_lists_files_and_dirs_sorted_dirs_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let entries = list_dir(dir.path(), "").unwrap();
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["sub", "a.txt", "b.txt"]
        );
        assert!(entries[0].is_dir);
        assert!(!entries[1].is_dir);
    }

    #[test]
    fn list_dir_orders_numbered_files_naturally() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["icon-10.png", "icon-2.png", "icon-1.png"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }

        let entries = list_dir(dir.path(), "").unwrap();
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["icon-1.png", "icon-2.png", "icon-10.png"]
        );
    }

    #[test]
    fn list_dir_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "x").unwrap();
        std::fs::write(dir.path().join("kept.txt"), "x").unwrap();

        let entries = list_dir(dir.path(), "").unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"kept.txt"));
        assert!(!names.contains(&"ignored.txt"));
    }

    #[test]
    fn list_dir_honors_a_root_gitignore_when_listing_a_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(dir.path().join("sub").join("ignored.txt"), "x").unwrap();
        std::fs::write(dir.path().join("sub").join("kept.txt"), "x").unwrap();

        let entries = list_dir(dir.path(), "sub").unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"kept.txt"));
        assert!(
            !names.contains(&"ignored.txt"),
            "root .gitignore should apply to a nested listing, got: {names:?}"
        );
    }

    #[test]
    fn list_dir_skips_the_dot_git_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
        std::fs::write(dir.path().join("readme.md"), "hi").unwrap();

        let entries = list_dir(dir.path(), "").unwrap();
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["readme.md"]
        );
    }

    #[test]
    fn list_dir_lists_a_nested_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("nested.txt"), "x").unwrap();

        let entries = list_dir(dir.path(), "sub").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "sub/nested.txt");
        assert_eq!(entries[0].name, "nested.txt");
    }

    #[test]
    fn list_dir_rejects_path_escaping_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let err = list_dir(dir.path(), "../etc").unwrap_err();
        assert!(matches!(err, Error::InvalidName(_)));
    }

    #[test]
    fn read_file_returns_text_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
        let content = read_file(dir.path(), "a.txt").unwrap();
        assert_eq!(
            content,
            FileContent::Text {
                text: "hello\nworld\n".to_string()
            }
        );
    }

    #[test]
    fn read_file_reads_a_nested_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("nested.txt"), "hi").unwrap();
        let content = read_file(dir.path(), "sub/nested.txt").unwrap();
        assert_eq!(
            content,
            FileContent::Text {
                text: "hi".to_string()
            }
        );
    }

    #[test]
    fn read_file_detects_binary_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        let content = read_file(dir.path(), "bin.dat").unwrap();
        assert_eq!(content, FileContent::Binary);
    }

    #[test]
    fn read_file_reports_too_large_without_reading_it() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b'x'; (MAX_PREVIEW_BYTES + 1) as usize];
        std::fs::write(dir.path().join("big.txt"), &big).unwrap();
        let content = read_file(dir.path(), "big.txt").unwrap();
        assert_eq!(
            content,
            FileContent::TooLarge {
                size: MAX_PREVIEW_BYTES + 1
            }
        );
    }

    #[test]
    fn read_file_rejects_path_escaping_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_file(dir.path(), "../etc/passwd").unwrap_err();
        assert!(matches!(err, Error::InvalidName(_)));
    }

    #[test]
    fn read_file_returns_a_base64_image_for_a_png() {
        let dir = tempfile::tempdir().unwrap();
        // PNG magic bytes — enough to prove the bytes round-trip, since the
        // image branch never sniffs content, only the extension.
        let bytes = [0x89, b'P', b'N', b'G', 0, 1, 2, 3];
        std::fs::write(dir.path().join("logo.png"), bytes).unwrap();

        let content = read_file(dir.path(), "logo.png").unwrap();
        assert_eq!(
            content,
            FileContent::Image {
                mime: "image/png".to_string(),
                data: "iVBORwABAgM=".to_string(),
            }
        );
    }

    #[test]
    fn read_file_maps_common_image_extensions_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        for (name, mime) in [
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
            ("a.bmp", "image/bmp"),
            ("a.ico", "image/x-icon"),
            ("a.avif", "image/avif"),
            ("A.PNG", "image/png"),
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
            match read_file(dir.path(), name).unwrap() {
                FileContent::Image { mime: got, .. } => assert_eq!(got, mime, "for {name}"),
                other => panic!("expected image for {name}, got {other:?}"),
            }
        }
    }

    #[test]
    fn read_file_keeps_svg_as_text_so_the_preview_can_toggle_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("icon.svg"), "<svg/>").unwrap();
        assert_eq!(
            read_file(dir.path(), "icon.svg").unwrap(),
            FileContent::Text {
                text: "<svg/>".to_string()
            }
        );
    }

    #[test]
    fn read_file_reports_an_oversized_image_as_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b'x'; (MAX_IMAGE_BYTES + 1) as usize];
        std::fs::write(dir.path().join("big.png"), &big).unwrap();
        assert_eq!(
            read_file(dir.path(), "big.png").unwrap(),
            FileContent::TooLarge {
                size: MAX_IMAGE_BYTES + 1
            }
        );
    }

    #[test]
    fn read_file_reports_an_oversized_svg_as_too_large_at_the_text_cap() {
        // SVG goes through the text path, so the smaller text cap applies —
        // an SVG just over it shouldn't be read as if it were a raster image.
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b'x'; (MAX_PREVIEW_BYTES + 1) as usize];
        std::fs::write(dir.path().join("big.svg"), &big).unwrap();
        assert_eq!(
            read_file(dir.path(), "big.svg").unwrap(),
            FileContent::TooLarge {
                size: MAX_PREVIEW_BYTES + 1
            }
        );
    }
}
