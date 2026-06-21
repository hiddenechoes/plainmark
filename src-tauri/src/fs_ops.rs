// SPDX-License-Identifier: GPL-3.0-or-later
//! All filesystem I/O for the vault lives here (SPEC §7.1). The webview never
//! touches the FS directly — it calls the Tauri commands in `main.rs`, which
//! delegate to these functions.
//!
//! The single most safety-critical primitive is [`atomic_write`]: every note
//! write goes through it. It writes a temp file in the *same* directory and
//! renames it over the target, so a note is never partially written in place.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// UTF-8 byte-order mark. Some Windows editors prepend it; we preserve it.
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// A note loaded for editing. `content` is LF-normalized for CodeMirror; `eol`
/// and `bom` record the on-disk style so a save restores the exact bytes.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFile {
    pub content: String,
    /// `"lf"` or `"crlf"`.
    pub eol: String,
    pub bom: bool,
}

/// One entry in the file tree. Directories carry `children`; files don't.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

/// Atomically write `bytes` to `path`.
///
/// Strategy: create a temp file in the *same directory* (so the final rename
/// stays on one filesystem and is atomic), write + flush + fsync it, then rename
/// it over the target. On error the temp file is cleaned up. `std::fs::rename`
/// replaces an existing target on Windows, macOS, and Linux.
///
/// This helper is byte-exact: it changes nothing about the bytes it is given.
/// Round-trip fidelity (line endings, BOM, encoding) is the caller's concern —
/// see [`save_note`].
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target path has no parent directory",
        )
    })?;
    fs::create_dir_all(dir)?;

    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("note");
    let tmp_path = dir.join(format!(".{file_name}.plainmark.tmp"));

    {
        let mut tmp = File::create(&tmp_path)?;
        tmp.write_all(bytes)?;
        tmp.flush()?;
        // Ensure bytes hit disk before the rename so a crash can't leave a
        // truncated note behind the rename.
        tmp.sync_all()?;
    }

    match fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

/// Read a note for editing: strip a UTF-8 BOM if present, detect the line-ending
/// style, and normalize content to LF for the editor.
pub fn read_note(path: &Path) -> AppResult<NoteFile> {
    let raw = fs::read(path)?;
    let (bom, body) = if raw.starts_with(&BOM) {
        (true, &raw[BOM.len()..])
    } else {
        (false, &raw[..])
    };
    let text = String::from_utf8(body.to_vec())
        .map_err(|e| AppError::InvalidPath(format!("note is not valid UTF-8: {e}")))?;
    let eol = if text.contains("\r\n") { "crlf" } else { "lf" };
    let content = text.replace("\r\n", "\n");
    Ok(NoteFile {
        content,
        eol: eol.to_string(),
        bom,
    })
}

/// Save editor `content` (LF-normalized) back to disk, restoring the original
/// `eol` style and `bom` so only the intended text changes (SPEC §7.1).
pub fn save_note(path: &Path, content: &str, eol: &str, bom: bool) -> AppResult<()> {
    let normalized = content.replace("\r\n", "\n");
    let bodied = if eol == "crlf" {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    };

    let mut bytes = Vec::with_capacity(bodied.len() + BOM.len());
    if bom {
        bytes.extend_from_slice(&BOM);
    }
    bytes.extend_from_slice(bodied.as_bytes());

    atomic_write(path, &bytes)?;
    Ok(())
}

/// Build the markdown file tree rooted at `root`, recursively. Hidden entries
/// (including the vault-local `.plainmark/` dir) are skipped, and folders are
/// only included when they contain markdown somewhere beneath them.
pub fn build_tree(root: &Path) -> AppResult<Vec<TreeNode>> {
    build_dir(root)
}

fn build_dir(dir: &Path) -> AppResult<Vec<TreeNode>> {
    let mut dirs: Vec<TreeNode> = Vec::new();
    let mut files: Vec<TreeNode> = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip dotfiles and dot-dirs (covers `.plainmark/`, `.git/`, etc.).
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            let children = build_dir(&path)?;
            if !children.is_empty() {
                dirs.push(TreeNode {
                    name,
                    path: path.to_string_lossy().to_string(),
                    is_dir: true,
                    children,
                });
            }
        } else if file_type.is_file() && has_md_extension(&path) {
            files.push(TreeNode {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: false,
                children: Vec::new(),
            });
        }
    }

    // Folders first, then files; each group sorted case-insensitively.
    dirs.sort_by_key(|n| n.name.to_lowercase());
    files.sort_by_key(|n| n.name.to_lowercase());
    dirs.extend(files);
    Ok(dirs)
}

fn has_md_extension(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

/// Ensure `target` resolves to a location inside `vault`, defending against
/// `..` traversal. Returns the canonical path to use. Works for both existing
/// files and not-yet-created files (canonicalizes the nearest existing parent).
pub fn ensure_within(vault: &Path, target: &Path) -> AppResult<PathBuf> {
    let canonical_vault = vault
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(e.to_string()))?;
    let canonical_target = canonical_for_check(target)?;
    if canonical_target.starts_with(&canonical_vault) {
        Ok(canonical_target)
    } else {
        Err(AppError::OutsideVault)
    }
}

fn canonical_for_check(p: &Path) -> AppResult<PathBuf> {
    if p.exists() {
        return p
            .canonicalize()
            .map_err(|e| AppError::InvalidPath(e.to_string()));
    }
    let parent = p
        .parent()
        .ok_or_else(|| AppError::InvalidPath("path has no parent".into()))?;
    let file = p
        .file_name()
        .ok_or_else(|| AppError::InvalidPath("path has no file name".into()))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(e.to_string()))?;
    Ok(canonical_parent.join(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // §7.1 guarantee: a CRLF file round-trips through the atomic-write helper
    // byte-for-byte. Input is a raw byte literal so git normalization can't
    // silently turn the CRLFs into LFs.
    #[test]
    fn atomic_write_preserves_crlf_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("crlf.md");
        let input: &[u8] = b"a\r\nb\r\nc\r\n";
        atomic_write(&path, input).unwrap();
        assert_eq!(fs::read(&path).unwrap(), input);
    }

    // §7.1 guarantee: a UTF-8 file with a BOM round-trips unchanged. The BOM
    // bytes (EF BB BF) are written as a raw literal.
    #[test]
    fn atomic_write_preserves_utf8_bom_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bom.md");
        let mut input: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
        input.extend_from_slice(b"hello\nworld\n");
        atomic_write(&path, &input).unwrap();
        assert_eq!(fs::read(&path).unwrap(), input);
    }

    #[test]
    fn atomic_write_overwrites_existing_target() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"a longer second value").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"a longer second value");
    }

    // The full read -> edit-nothing -> save cycle must reproduce the original
    // bytes for a BOM + CRLF file.
    #[test]
    fn save_note_round_trips_bom_and_crlf() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("doc.md");
        let original: Vec<u8> = {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(b"line1\r\nline2\r\n");
            v
        };
        fs::write(&path, &original).unwrap();

        let note = read_note(&path).unwrap();
        assert_eq!(note.content, "line1\nline2\n");
        assert_eq!(note.eol, "crlf");
        assert!(note.bom);

        save_note(&path, &note.content, &note.eol, note.bom).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn save_note_plain_lf_no_bom_stays_plain() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plain.md");
        fs::write(&path, b"a\nb\n").unwrap();

        let note = read_note(&path).unwrap();
        assert_eq!(note.eol, "lf");
        assert!(!note.bom);

        save_note(&path, "a\nb\nc\n", &note.eol, note.bom).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"a\nb\nc\n");
    }

    #[test]
    fn ensure_within_rejects_traversal() {
        let dir = tempdir().unwrap();
        let vault = dir.path().join("vault");
        fs::create_dir_all(&vault).unwrap();
        let outside = dir.path().join("secret.md");
        fs::write(&outside, b"x").unwrap();

        // A path that escapes the vault via `..` must be rejected.
        let escaping = vault.join("../secret.md");
        assert!(matches!(
            ensure_within(&vault, &escaping),
            Err(AppError::OutsideVault)
        ));
    }

    #[test]
    fn ensure_within_accepts_inside_path() {
        let dir = tempdir().unwrap();
        let vault = dir.path().join("vault");
        fs::create_dir_all(&vault).unwrap();
        let note = vault.join("note.md");
        fs::write(&note, b"x").unwrap();
        assert!(ensure_within(&vault, &note).is_ok());
    }

    #[test]
    fn build_tree_lists_md_and_skips_hidden() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("b.md"), b"b").unwrap();
        fs::write(root.join("a.md"), b"a").unwrap();
        fs::write(root.join("notes.txt"), b"ignore me").unwrap();
        fs::create_dir_all(root.join(".plainmark")).unwrap();
        fs::write(root.join(".plainmark/settings.json"), b"{}").unwrap();
        fs::create_dir_all(root.join("Projects")).unwrap();
        fs::write(root.join("Projects/p.md"), b"p").unwrap();
        fs::create_dir_all(root.join("Empty")).unwrap();

        let tree = build_tree(root).unwrap();
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        // Folder with md first, then md files alphabetically; no `.plainmark`,
        // no `.txt`, no empty folder.
        assert_eq!(names, vec!["Projects", "a.md", "b.md"]);
        assert!(tree[0].is_dir);
        assert_eq!(tree[0].children.len(), 1);
    }
}
