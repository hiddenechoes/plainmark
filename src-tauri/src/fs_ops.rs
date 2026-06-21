// SPDX-License-Identifier: GPL-3.0-or-later
//! All filesystem I/O for the vault lives here (SPEC §7.1). The webview never
//! touches the FS directly — it calls the Tauri commands in `main.rs`, which
//! delegate to these functions.
//!
//! The single most safety-critical primitive is [`atomic_write`]: every note
//! write goes through it. It writes a temp file in the *same* directory and
//! renames it over the target, so a note is never partially written in place.

use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

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

/// Vault-local settings (`.plainmark/settings.json`). Phase 1 only reads the
/// attachments folder; the settings UI and the rest of these keys arrive later
/// (SPEC §7, Phase 5). A missing or corrupt file falls back to defaults.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultSettings {
    #[serde(default = "default_attachments_dir")]
    attachments_dir: String,
}

fn default_attachments_dir() -> String {
    "Attachments".to_string()
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            attachments_dir: default_attachments_dir(),
        }
    }
}

fn load_vault_settings(vault_root: &Path) -> VaultSettings {
    let path = vault_root.join(".plainmark").join("settings.json");
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => VaultSettings::default(),
    }
}

/// The result of saving a pasted/dropped attachment: a vault-relative,
/// forward-slash path suitable for an `![[...]]` embed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAttachment {
    pub relative_path: String,
}

/// Restrict a configured attachments folder to a *relative* subpath of the
/// vault — reject absolute paths and any `..`/`.`/root component so a hostile
/// `.plainmark/settings.json` can't redirect writes outside the vault.
fn safe_subdir(name: &str) -> String {
    let trimmed = name.trim();
    let all_normal = !trimmed.is_empty()
        && Path::new(trimmed)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if all_normal {
        trimmed.replace('\\', "/")
    } else {
        default_attachments_dir()
    }
}

/// Reduce an extension to a safe lowercase alphanumeric token, defaulting to
/// `png` (pasted clipboard images are almost always PNG).
fn sanitize_ext(ext: &str) -> String {
    let cleaned: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if cleaned.is_empty() {
        "png".to_string()
    } else {
        cleaned.to_lowercase()
    }
}

/// A short, stable hex tag derived from the bytes, to keep names collision-safe
/// even when two pastes land in the same millisecond.
fn short_hash(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:08x}", hasher.finish() & 0xffff_ffff)
}

/// Write image `bytes` into the vault's attachments folder under a
/// collision-safe `{timestamp}-{hash}.{ext}` name, atomically (§7.1). Returns
/// the vault-relative path for the inserted embed. `now_millis` is injected so
/// the naming is testable.
pub fn save_attachment(
    vault_root: &Path,
    bytes: &[u8],
    ext: &str,
    now_millis: u128,
) -> AppResult<SavedAttachment> {
    let dir_name = safe_subdir(&load_vault_settings(vault_root).attachments_dir);
    let dir = vault_root.join(&dir_name);
    let ext = sanitize_ext(ext);
    let hash = short_hash(bytes);

    // Bump a counter only if the candidate name is already taken on disk.
    let mut counter = 0u32;
    let file_name = loop {
        let name = if counter == 0 {
            format!("{now_millis}-{hash}.{ext}")
        } else {
            format!("{now_millis}-{hash}-{counter}.{ext}")
        };
        if !dir.join(&name).exists() {
            break name;
        }
        counter += 1;
    };

    atomic_write(&dir.join(&file_name), bytes)?;
    Ok(SavedAttachment {
        relative_path: format!("{dir_name}/{file_name}"),
    })
}

/// Read an image and return it as a `data:` URL, so the webview can render it
/// without any direct filesystem access. The caller must have already scoped
/// `path` to the active vault.
pub fn read_image_data_url(path: &Path) -> AppResult<String> {
    let bytes = fs::read(path)?;
    let mime = mime_for(path);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
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

/// Recursively collect every `.md` file under `root`, skipping hidden entries
/// (`.plainmark/`, `.git/`, …) the same way [`build_tree`] does. Used by the
/// indexer to build the link graph on vault open.
pub fn list_md_files(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_md_files(root, &mut out)?;
    Ok(out)
}

fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) -> AppResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_md_files(&path, out)?;
        } else if file_type.is_file() && has_md_extension(&path) {
            out.push(path);
        }
    }
    Ok(())
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

    // §7.1 / §8.9: pasting an image is the webview inserting an `![[...]]` line
    // into the (LF-normalized) content, then save_note restoring the original
    // EOL/BOM. Only the inserted region must change — every other byte is
    // preserved exactly, including the CRLFs and the BOM.
    #[test]
    fn paste_into_crlf_bom_note_changes_only_the_inserted_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("doc.md");
        let original: Vec<u8> = {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(b"# Title\r\n\r\nbody line\r\n");
            v
        };
        fs::write(&path, &original).unwrap();

        let note = read_note(&path).unwrap();
        assert_eq!(note.content, "# Title\n\nbody line\n");

        // Editor inserts the embed at the cursor (here: start of line 3).
        let edited = note
            .content
            .replace("body line", "![[Attachments/x.png]]\nbody line");
        save_note(&path, &edited, &note.eol, note.bom).unwrap();

        // Expected bytes: original, with exactly the embed line spliced in as
        // CRLF, BOM intact, nothing else touched.
        let expected: Vec<u8> = {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(b"# Title\r\n\r\n![[Attachments/x.png]]\r\nbody line\r\n");
            v
        };
        assert_eq!(fs::read(&path).unwrap(), expected);
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

    // §8.9: a saved attachment lands under `Attachments/`, round-trips its bytes,
    // resolves inside the vault, and gets a collision-safe name.
    #[test]
    fn save_attachment_writes_under_attachments_and_round_trips() {
        let dir = tempdir().unwrap();
        let vault = dir.path();
        let bytes = b"\x89PNG\r\n\x1a\nfake-image-bytes";

        let saved = save_attachment(vault, bytes, "PNG", 1_700_000_000_000).unwrap();

        assert!(
            saved.relative_path.starts_with("Attachments/"),
            "got {}",
            saved.relative_path
        );
        assert!(saved.relative_path.ends_with(".png"));
        let on_disk = vault.join(&saved.relative_path);
        assert_eq!(fs::read(&on_disk).unwrap(), bytes);
        // The returned path stays inside the vault.
        assert!(ensure_within(vault, &on_disk).is_ok());
    }

    #[test]
    fn save_attachment_generates_distinct_names_for_same_instant() {
        let dir = tempdir().unwrap();
        let vault = dir.path();
        let bytes = b"collide";

        let a = save_attachment(vault, bytes, "png", 42).unwrap();
        let b = save_attachment(vault, bytes, "png", 42).unwrap();

        assert_ne!(a.relative_path, b.relative_path);
        assert!(vault.join(&a.relative_path).exists());
        assert!(vault.join(&b.relative_path).exists());
    }

    #[test]
    fn save_attachment_honors_vault_local_attachments_dir() {
        let dir = tempdir().unwrap();
        let vault = dir.path();
        fs::create_dir_all(vault.join(".plainmark")).unwrap();
        fs::write(
            vault.join(".plainmark/settings.json"),
            br#"{"attachmentsDir": "media/pics"}"#,
        )
        .unwrap();

        let saved = save_attachment(vault, b"x", "png", 1).unwrap();
        assert!(saved.relative_path.starts_with("media/pics/"));
        assert!(vault.join(&saved.relative_path).exists());
    }

    #[test]
    fn save_attachment_rejects_escaping_attachments_dir() {
        let dir = tempdir().unwrap();
        let vault = dir.path().join("vault");
        fs::create_dir_all(vault.join(".plainmark")).unwrap();
        fs::write(
            vault.join(".plainmark/settings.json"),
            br#"{"attachmentsDir": "../escape"}"#,
        )
        .unwrap();

        let saved = save_attachment(&vault, b"x", "png", 1).unwrap();
        // Falls back to the default folder instead of escaping the vault.
        assert!(saved.relative_path.starts_with("Attachments/"));
        assert!(!dir.path().join("escape").exists());
    }

    #[test]
    fn read_image_data_url_encodes_mime_and_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pic.png");
        fs::write(&path, b"abc").unwrap();

        let url = read_image_data_url(&path).unwrap();
        // base64("abc") == "YWJj"
        assert_eq!(url, "data:image/png;base64,YWJj");
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
